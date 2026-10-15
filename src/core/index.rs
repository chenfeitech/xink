//! 全库索引：把库内每篇笔记的事实（标题 / 链接 / 双链 / 标签）集中解析一次，
//! 供双链、反向链接、标签与全文搜索复用。
//!
//! 设计见 `docs/note-index-design.md`。三条原则：
//! 1. **磁盘是真值**：索引只存派生数据，不落盘，启动后台重建（规则 04）；
//! 2. **主表 + 实时派生**：只维护 `notes`，反链 / 标签在查询时反查，
//!    避免增量更新时把派生表写错（详见设计 §3）；
//! 3. **只收笔记**：`.md` 才参与双链与标签，数据文件不进索引。

use std::collections::HashMap;
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
        };

        let mut files = Vec::new();
        collect_markdown(root, &tree, &mut files);
        for path in files {
            index.reindex_file(&path);
        }
        Ok(index)
    }

    /// 重新解析单个文件（新增与更新都走这里，幂等）。
    ///
    /// 非笔记文件（数据文件等）与读取失败的文件都会被跳过：
    /// 索引里不该出现打不开的条目。
    pub fn reindex_file(&mut self, path: &Path) {
        if !is_note(path) {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let facts = parse_facts(&text);
        let title = title_of(&facts, path);
        self.insert(IndexedNote {
            path: path.to_path_buf(),
            title,
            facts,
        });
    }

    /// 从索引里移除一个路径
    pub fn remove_file(&mut self, path: &Path) {
        if self.notes.remove(path).is_none() {
            return;
        }
        if let Some(stem) = file_stem(path)
            && let Some(list) = self.by_name.get_mut(&stem.to_lowercase())
        {
            list.retain(|p| p != path);
            if list.is_empty() {
                self.by_name.remove(&stem.to_lowercase());
            }
        }
        if let Some(root) = &self.root
            && let Ok(rel) = path.strip_prefix(root)
        {
            self.by_rel_path
                .remove(&rel.to_string_lossy().to_lowercase());
        }
    }

    /// 反向链接：哪些笔记引用了 `path`
    pub fn backlinks(&self, path: &Path) -> Vec<LinkRef> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        let targets = link_targets_of(path, root);

        let mut refs = Vec::new();
        for note in self.notes.values() {
            if note.path == path {
                continue;
            }
            for link in &note.facts.wiki_links {
                let key = link.target.trim().trim_end_matches(".md").to_lowercase();
                if targets.contains(&key) {
                    refs.push(LinkRef {
                        from: note.path.clone(),
                        line: link.line,
                        target: link.target.clone(),
                    });
                }
            }
        }
        refs.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.line.cmp(&b.line)));
        refs
    }

    /// 标签聚合：`(标签名, 笔记数)`，按数量降序、同数量按字典序
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for note in self.notes.values() {
            for tag in &note.facts.tags {
                // 大小写归一：`#Rust` 与 `#rust` 是同一个标签
                let list = counts.entry(tag.name.to_lowercase()).or_default();
                if !list.contains(&note.path) {
                    list.push(note.path.clone());
                }
            }
        }
        let mut tags: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(name, paths)| (name, paths.len()))
            .collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        tags
    }

    /// 带某个标签的笔记
    pub fn notes_with_tag(&self, tag: &str) -> Vec<PathBuf> {
        let tag = tag.trim_start_matches('#').to_lowercase();
        let mut paths: Vec<PathBuf> = self
            .notes
            .values()
            .filter(|note| note.facts.tags.iter().any(|t| t.name.to_lowercase() == tag))
            .map(|note| note.path.clone())
            .collect();
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

        // 3) 标题
        if hits.is_empty() {
            hits = self
                .notes
                .values()
                .filter(|note| note.title.to_lowercase() == stem)
                .map(|note| note.path.clone())
                .collect();
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

    fn insert(&mut self, note: IndexedNote) {
        if let Some(stem) = file_stem(&note.path) {
            let list = self.by_name.entry(stem.to_lowercase()).or_default();
            if !list.contains(&note.path) {
                list.push(note.path.clone());
            }
        }
        if let Some(root) = &self.root
            && let Ok(rel) = note.path.strip_prefix(root)
        {
            self.by_rel_path
                .insert(rel.to_string_lossy().to_lowercase(), note.path.clone());
        }
        self.notes.insert(note.path.clone(), note);
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
        if is_note(&node.path) {
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
