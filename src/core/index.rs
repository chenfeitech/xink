//! 全库索引：把库内每篇笔记的事实（标题 / 链接 / 双链 / 标签）集中解析一次，
//! 供双链、反向链接、标签与全文搜索复用。
//!
//! 设计见 `docs/note-index-design.md`，派生表部分见
//! `docs/navigation/index-derived-tables-design.md`。三条原则：
//! 1. **磁盘是真值**：索引只存派生数据，不落盘，启动后台重建（规则 04）；
//! 2. **主表 + 增量维护的派生表**：`notes` 是唯一真值，反链 / 标签 / 标题
//!    另建派生表，所有写操作统一走 `detach` + `attach` 一对方法。
//!    原设计是"查询时实时反查"（`note-index-design.md` §3），实测 `tags()`
//!    在 3000 篇库上单次 345ms、且每次自动保存都会触发，故改为派生表；
//! 3. **只收笔记**：`.md` 才参与双链与标签，数据文件不进索引。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::EventEmitter;

use crate::core::markdown::{DocFacts, parse_facts};
use crate::core::vault::{FileNode, Vault};

/// 一篇笔记在索引里的条目
#[derive(Debug, Clone)]
// 索引的消费者在后续任务里：T8 大纲（headings）、T9 双链（resolve_wikilink）、
// T10 反链（backlinks）、T11 标签（tags）、T12 搜索（notes）。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct IndexedNote {
    pub path: PathBuf,
    /// 标题：第一个 H1 → 否则第一个标题 → 否则文件名（去扩展名）
    pub title: String,
    /// 解析出来的事实（含原始行号，供跳转）
    pub facts: DocFacts,
}

/// 索引内容发生变化（构建完成 / 单篇重解析 / 删除）。
///
/// 面板订阅它来决定"什么时候重绘"，自己不去轮询。
#[derive(Debug, Clone, Copy)]
pub struct IndexChanged;

/// 一条引用（构成反向链接）
#[derive(Debug, Clone, PartialEq, Eq)]
// 索引的消费者在后续任务里：T8 大纲（headings）、T9 双链（resolve_wikilink）、
// T10 反链（backlinks）、T11 标签（tags）、T12 搜索（notes）。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct LinkRef {
    /// 引用来源笔记
    pub from: PathBuf,
    /// 引用所在行（0-based）
    pub line: usize,
    /// 双链里写的原始目标文本
    pub target: String,
}

/// 双链目标的解析结果
#[derive(Debug, Clone, PartialEq, Eq)]
// 索引的消费者在后续任务里：T8 大纲（headings）、T9 双链（resolve_wikilink）、
// T10 反链（backlinks）、T11 标签（tags）、T12 搜索（notes）。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub enum Resolution {
    /// 唯一命中
    Hit(PathBuf),
    /// 多个候选（同名笔记），由调用方决定如何提示
    Ambiguous(Vec<PathBuf>),
    /// 目标不存在（可能是尚未创建的笔记）
    Missing,
}

/// 全库索引
#[derive(Debug, Default)]
pub struct NoteIndex {
    /// 当前库根；未打开库时为 `None`
    root: Option<PathBuf>,
    /// 唯一真值：路径 → 笔记
    notes: HashMap<PathBuf, IndexedNote>,
    /// 文件名（小写、不含扩展名）→ 路径，用于双链解析
    by_name: HashMap<String, Vec<PathBuf>>,
    /// 相对库根的路径（小写）→ 绝对路径，同上
    by_rel_path: HashMap<String, PathBuf>,

    // ── 派生表：由 detach / attach 维护，查询时只读 ──────────
    /// 笔记标题（小写）→ 路径。`resolve_wikilink` 第 3 步（按标题匹配）用
    by_title: HashMap<String, Vec<PathBuf>>,
    /// 标签（小写）→ 带该标签的笔记。用 `HashSet` 而非 `Vec`：
    /// 旧实现用 `Vec` + `contains` 去重，是 O(标签下的笔记数²)
    by_tag: HashMap<String, HashSet<PathBuf>>,
    /// 双链引用的**未解析**桶：归一化后的双链目标 → 写过它的条目（含来源与行号）
    ///
    /// 存"写了什么"而不是"指向谁"，因为后者取决于建表那一刻库里有哪些笔记：
    /// 先写的笔记引用了一篇后建的笔记，那份引用就永远补不回来（除非每次
    /// 新增都全库重解析一遍）。改成按目标文本分桶后，`backlinks` 在查询时
    /// 才用目标的 [`link_targets_of`] 取桶，谁先建谁后建都不影响。
    link_refs: HashMap<String, Vec<LinkRef>>,
}

// 索引的消费者在后续任务里：T8 大纲（headings）、T9 双链（resolve_wikilink）、
// T10 反链（backlinks）、T11 标签（tags）、T12 搜索（notes）。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
impl NoteIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// 索引所属的库根
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// 已索引的笔记数
    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// 构建整库索引（**同步 IO，必须在后台线程调用**）。
    ///
    /// 单个文件读失败只跳过该文件，不中断整库构建。
    pub fn build(root: &Path) -> Result<Self, String> {
        let tree = Vault::open(root.to_path_buf())
            .and_then(|vault| vault.scan(false))
            .map_err(|err| err.to_string())?;

        let mut index = Self {
            root: Some(root.to_path_buf()),
            notes: HashMap::new(),
            by_name: HashMap::new(),
            by_rel_path: HashMap::new(),
            by_title: HashMap::new(),
            by_tag: HashMap::new(),
            link_refs: HashMap::new(),
        };

        let mut files = Vec::new();
        collect_markdown(root, &tree, &mut files);
        for path in files {
            index.reindex_file(&path);
        }
        Ok(index)
    }

    /// 重新解析单个文件（新增、更新、改名都走这里，幂等）。
    ///
    /// 非笔记文件（数据文件等）与读取失败的文件都会被跳过：
    /// 索引里不该出现打不开的条目。
    ///
    /// **例外：文件已经不在磁盘上时要把旧条目摘掉。** 改名事件给的是
    /// `[旧路径, 新路径]`，旧路径读不到，若照旧早退，索引里会永久留下一篇
    /// 并不存在的笔记——它还会继续参与标签计数与双链解析。
    /// 只按"文件不存在"判定：权限不足、非 UTF-8 等情况下文件仍在，
    /// 这时保留旧条目比摘掉更稳妥。
    pub fn reindex_file(&mut self, path: &Path) {
        if !is_note(path) {
            return;
        }
        let Some(note) = read_note(path) else {
            if !path.exists() {
                self.detach(path);
            }
            return;
        };
        // 先摘干净再重新登记：派生表因此不会留下这一篇的旧事实
        self.detach(path);
        self.attach_keys(note);
        self.attach_links(path);
    }

    /// 从索引里移除一个路径
    pub fn remove_file(&mut self, path: &Path) {
        self.detach(path);
    }

    /// 反向链接：哪些笔记引用了 `path`
    pub fn backlinks(&self, path: &Path) -> Vec<LinkRef> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        // 这篇笔记能被写成哪几种形式（`a` / `sub/a.md` / `sub/a`），
        // 桶里就是别人写下的对应引用。桶的大小等于命中数，与库规模无关。
        let mut keys = link_targets_of(path, root);
        keys.sort();
        keys.dedup();

        let mut refs = Vec::new();
        for key in keys {
            let Some(bucket) = self.link_refs.get(&key) else {
                continue;
            };
            // 自引不算反链
            refs.extend(bucket.iter().filter(|link| link.from != path).cloned());
        }
        refs.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.line.cmp(&b.line)));
        refs
    }

    /// 标签聚合：`(标签名, 笔记数)`，按数量降序、同数量按字典序
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut tags: Vec<(String, usize)> = self
            .by_tag
            .iter()
            .map(|(name, paths)| (name.clone(), paths.len()))
            .collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        tags
    }

    /// 带某个标签的笔记
    pub fn notes_with_tag(&self, tag: &str) -> Vec<PathBuf> {
        let tag = tag.trim_start_matches('#').to_lowercase();
        let mut paths: Vec<PathBuf> = self
            .by_tag
            .get(&tag)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default();
        paths.sort();
        paths
    }

    /// 解析双链目标：相对路径 → 文件名 → 标题，均大小写不敏感
    pub fn resolve_wikilink(&self, target: &str) -> Resolution {
        let raw = target.trim();
        if raw.is_empty() {
            return Resolution::Missing;
        }
        let key = raw.to_lowercase();

        // 1) 相对库根路径（写 `notes/a.md` 或 `notes/a` 都认）
        if let Some(path) = self.by_rel_path.get(&key) {
            return Resolution::Hit(path.clone());
        }
        if let Some(path) = self.by_rel_path.get(&format!("{key}.md")) {
            return Resolution::Hit(path.clone());
        }

        // 2) 文件名
        let stem = raw.trim_end_matches(".md").to_lowercase();
        let mut hits: Vec<PathBuf> = self.by_name.get(&stem).cloned().unwrap_or_default();

        // 3) 标题（走派生表，不再全表扫）
        if hits.is_empty() {
            hits = self.by_title.get(&stem).cloned().unwrap_or_default();
        }

        hits.sort();
        match hits.len() {
            0 => Resolution::Missing,
            1 => Resolution::Hit(hits.remove(0)),
            _ => Resolution::Ambiguous(hits),
        }
    }

    /// 解析双链目标，只取"唯一命中"；歧义与缺失都当作解析不到。
    ///
    /// 预览改写双链时用这个版本：宁可显示成普通文本，
    /// 也不要让点击落到"猜一个"的笔记上。
    pub fn resolve_path(&self, target: &str) -> Option<PathBuf> {
        match self.resolve_wikilink(target) {
            Resolution::Hit(path) => Some(path),
            Resolution::Ambiguous(_) | Resolution::Missing => None,
        }
    }

    /// 所有笔记（全文搜索等按需遍历）
    pub fn notes(&self) -> impl Iterator<Item = &IndexedNote> {
        self.notes.values()
    }

    /// 取一条笔记
    pub fn note(&self, path: &Path) -> Option<&IndexedNote> {
        self.notes.get(path)
    }

    /// 把一个路径从**所有**表里摘干净（主表 + 五张派生表）
    ///
    /// 与 [`Self::attach_keys`] / [`Self::attach_links`] 成对使用：新增、
    /// 重解析、删除三种写操作都是"先 detach 再 attach"，因此派生表写错的
    /// 可能只集中在这一对方法里——由 `incremental_updates_match_full_build`
    /// 那条差异测试兜住（全量构建与逐篇增量的结果必须逐项相等）。
    fn detach(&mut self, path: &Path) {
        let Some(note) = self.notes.remove(path) else {
            return;
        };
        let root = self.root.clone();

        if let Some(stem) = file_stem(path) {
            remove_from_list(&mut self.by_name, &stem.to_lowercase(), path);
        }
        if let Some(root) = &root
            && let Ok(rel) = path.strip_prefix(root)
        {
            self.by_rel_path
                .remove(&rel.to_string_lossy().to_lowercase());
        }
        remove_from_list(&mut self.by_title, &note.title.to_lowercase(), path);

        for tag in &note.facts.tags {
            let key = tag.name.to_lowercase();
            let mut empty = false;
            if let Some(set) = self.by_tag.get_mut(&key) {
                set.remove(path);
                empty = set.is_empty();
            }
            if empty {
                self.by_tag.remove(&key);
            }
        }

        // 摘掉它写下的双链引用。分桶键由**旧事实**算出，
        // 因此内容改了也不会漏摘旧桶。
        for link in &note.facts.wiki_links {
            let key = link_key(&link.target);
            let mut empty = false;
            if let Some(bucket) = self.link_refs.get_mut(&key) {
                bucket.retain(|entry| entry.from != path);
                empty = bucket.is_empty();
            }
            if empty {
                self.link_refs.remove(&key);
            }
        }
    }

    /// 登记主表与 `by_title` / `by_tag`（不含它写下的双链引用）
    fn attach_keys(&mut self, note: IndexedNote) {
        let path = note.path.clone();
        let root = self.root.clone();

        if let Some(stem) = file_stem(&path) {
            push_unique(&mut self.by_name, stem.to_lowercase(), path.clone());
        }
        if let Some(root) = &root
            && let Ok(rel) = path.strip_prefix(root)
        {
            self.by_rel_path
                .insert(rel.to_string_lossy().to_lowercase(), path.clone());
        }
        push_unique(&mut self.by_title, note.title.to_lowercase(), path.clone());

        for tag in &note.facts.tags {
            // 大小写归一：`#Rust` 与 `#rust` 是同一个标签
            self.by_tag
                .entry(tag.name.to_lowercase())
                .or_default()
                .insert(path.clone());
        }

        self.notes.insert(path, note);
    }

    /// 把这篇笔记写下的双链引用登记进 `link_refs`
    ///
    /// 只记"它写了什么"，**不解析指向谁**：解析发生在 `backlinks` 查询时
    /// （见该字段的注释）。因此这一步不依赖其它笔记是否已入库，
    /// 新增、重解析、删除三种写操作可以走完全相同的代码路径。
    fn attach_links(&mut self, path: &Path) {
        let Some(note) = self.notes.get(path) else {
            return;
        };
        let links = note.facts.wiki_links.clone();
        for link in links {
            self.link_refs
                .entry(link_key(&link.target))
                .or_default()
                .push(LinkRef {
                    from: path.to_path_buf(),
                    line: link.line,
                    target: link.target,
                });
        }
    }
}

/// 读一篇笔记并解析出事实；非笔记文件与读失败的文件返回 `None`
///
/// 索引里不该出现打不开的条目（与主表口径一致）。
fn read_note(path: &Path) -> Option<IndexedNote> {
    if !is_note(path) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let facts = parse_facts(&text);
    let title = title_of(&facts, path);
    Some(IndexedNote {
        path: path.to_path_buf(),
        title,
        facts,
    })
}

/// 双链目标在索引里的归一化形式
///
/// 必须与 [`link_targets_of`] 的口径一致（`trim` + 去 `.md` + 小写），
/// 否则"这条双链指向谁"会查错——差异测试覆盖这一点。
fn link_key(target: &str) -> String {
    target.trim().trim_end_matches(".md").to_lowercase()
}

/// 往 `key` 里加一个路径；同一路径不重复登记
///
/// 同一篇笔记可能同时被多种写法命中（如根目录下的 `a.md`，stem 与
/// "相对路径去扩展名"都是 `a`），不去重会算出重复的引用条目。
fn push_unique(map: &mut HashMap<String, Vec<PathBuf>>, key: String, path: PathBuf) {
    let list = map.entry(key).or_default();
    if !list.contains(&path) {
        list.push(path);
    }
}

/// 从 `key` 里摘掉一个路径；摘空了连 key 一起删掉，避免留下空桶
fn remove_from_list(map: &mut HashMap<String, Vec<PathBuf>>, key: &str, path: &Path) {
    let mut empty = false;
    if let Some(list) = map.get_mut(key) {
        list.retain(|p| p.as_path() != path);
        empty = list.is_empty();
    }
    if empty {
        map.remove(key);
    }
}

/// 一条笔记在双链里可能被写成的几种形式（小写）
// 索引的消费者在后续任务里：T8 大纲（headings）、T9 双链（resolve_wikilink）、
// T10 反链（backlinks）、T11 标签（tags）、T12 搜索（notes）。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
fn link_targets_of(path: &Path, root: &Path) -> Vec<String> {
    let mut targets = Vec::new();
    if let Some(stem) = file_stem(path) {
        targets.push(stem.to_lowercase());
    }
    if let Ok(rel) = path.strip_prefix(root) {
        let rel = rel.to_string_lossy().to_lowercase();
        targets.push(rel.clone());
        targets.push(rel.trim_end_matches(".md").to_string());
    }
    targets
}

/// 笔记标题：第一个 H1 → 第一个标题 → 文件名
fn title_of(facts: &DocFacts, path: &Path) -> String {
    if let Some(first) = facts.headings.iter().find(|h| h.level == 1) {
        return first.text.clone();
    }
    if let Some(first) = facts.headings.first() {
        return first.text.clone();
    }
    file_stem(path).unwrap_or_default().to_string()
}

/// 文件名（不含扩展名）
fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
}

/// 是否参与索引：只收 Markdown
fn is_note(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// 展平目录树，只留 Markdown 文件
fn collect_markdown(root: &Path, node: &FileNode, out: &mut Vec<PathBuf>) {
    if !node.is_dir {
        // 目录层已经跳过 `.xink`（`vault::should_ignore`），这里再挡一道：
        // 索引的语义是"人的笔记"，AI 的 skills 之类不该进来
        if is_note(&node.path) && !crate::core::media::ai_excluded(&node.path) {
            out.push(node.path.clone());
        }
        return;
    }
    let _ = root;
    for child in &node.children {
        collect_markdown(root, child, out);
    }
}

impl EventEmitter<IndexChanged> for NoteIndex {}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/index.rs"]
mod tests;
