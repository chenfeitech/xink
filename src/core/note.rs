//! Note（笔记）：一篇 Markdown 文件的内存表示
//!
//! 磁盘上的文件是唯一真值。本模块负责加载、编辑标记与原子保存。

use anyhow::{Context as _, Result};
use ropey::{LineType, Rope};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Note {
    path: PathBuf,
    rope: Rope,
    /// 有未保存的修改
    dirty: bool,
    /// 加载/保存时记录的磁盘 mtime，用于检测外部修改
    disk_mtime: Option<SystemTime>,
    /// 内容版本号，每次内容变化递增。
    /// UI 用它判断是否需要重新生成派生数据，避免每帧做 Rope → String 全量拷贝。
    version: u64,
}

impl Note {
    pub fn load(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取笔记失败: {}", path.display()))?;
        let mtime = mtime_of(&path).ok();

        Ok(Self {
            path,
            rope: Rope::from_str(&content),
            dirty: false,
            disk_mtime: mtime,
            version: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 内容版本号。内容没变则不变，UI 可据此跳过重算。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 行数。基于 Rope 直接统计，不产生字符串拷贝（编辑时滚动同步需要高频调用）。
    pub fn line_count(&self) -> usize {
        self.rope.len_lines(LineType::LF).max(1)
    }

    /// 第 `line` 行（0-based）的字节区间 `[start, end)`
    ///
    /// 滚动同步要用它去问编辑控件"这一行现在画在哪"（`range_to_bounds` 只认字节区间）。
    pub fn line_byte_range(&self, line: usize) -> Option<(usize, usize)> {
        if line >= self.rope.len_lines(LineType::LF) {
            return None;
        }
        let start = self.rope.line_to_byte_idx(line, LineType::LF);
        // 末行没有"下一行行首"可取，退到文本末尾
        let end = if line + 1 < self.rope.len_lines(LineType::LF) {
            self.rope.line_to_byte_idx(line + 1, LineType::LF)
        } else {
            self.rope.len()
        };
        if start > end {
            return None;
        }
        Some((start, end))
    }

    /// 编辑器内容变更时调用。内容未变则不置脏，避免无意义的写盘。
    ///
    /// 只替换**真正变了的那一段**，不重建整棵 Rope。
    /// 旧做法是 `self.text()`（Rope → String 全量拷贝）比对之后再 `Rope::from_str`
    /// 重建：一次按键就是三次全文分配加一次建树，长笔记上直接表现为输入延迟。
    /// 而编辑区每次 `Change` 都调这里，按字符输入时相邻两帧只差一两个字符。
    pub fn set_text(&mut self, text: &str) {
        let rope_len = self.rope.len();
        let (prefix, suffix) = common_affixes(&self.rope, text);
        // 整篇相同：一个字节都不动，也不置脏
        if prefix == rope_len && prefix == text.len() {
            return;
        }
        let end = rope_len - suffix;
        if end > prefix {
            self.rope.remove(prefix..end);
        }
        let middle = &text[prefix..text.len() - suffix];
        if !middle.is_empty() {
            self.rope.insert(prefix, middle);
        }
        self.dirty = true;
        self.version += 1;
    }

    /// 原子保存：先写同目录临时文件，再 rename 替换。
    /// 这样崩溃时不会留下半截笔记。
    pub fn save(&mut self) -> Result<()> {
        atomic_write(&self.path, &self.text())?;
        self.dirty = false;
        // 更新 mtime，避免把自己的写入误判成外部修改
        self.disk_mtime = mtime_of(&self.path).ok();
        Ok(())
    }

    /// 磁盘上的文件是否被外部改动过（M5 由 watcher 使用）
    #[allow(dead_code)]
    pub fn disk_changed(&self) -> bool {
        match (self.disk_mtime, mtime_of(&self.path).ok()) {
            (Some(recorded), Some(current)) => current != recorded,
            _ => false,
        }
    }

    /// 取回磁盘内容；与内存一致时返回 `None`（**不改动任何内容**）。
    ///
    /// 这是"正打字却被甩回文章开头"的第一道防线：应用自己写盘也会被
    /// `notify` 报成一次改动（mtime 变了），但内容本来就是同一份。
    /// 若照旧把文本回填给编辑控件，控件的 `set_value` 会把光标重置到 0
    /// （gpui-base `input/base/state.rs` 的 `set_value` → `reset_selection`）。
    /// 内容一致就只把新 mtime 记下来，一个字节都不碰。
    ///
    /// 返回 `Some(内容)` 表示确实被外部改过，调用方才需要回填。
    pub fn take_external_text(&mut self) -> Result<Option<String>> {
        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("读取笔记失败: {}", self.path.display()))?;
        let mtime = mtime_of(&self.path).ok();
        if content == self.text() {
            // 只有 mtime 变了：记下它，避免下一次事件又进到这里
            self.disk_mtime = mtime;
            return Ok(None);
        }
        self.rope = Rope::from_str(&content);
        self.dirty = false;
        self.disk_mtime = mtime;
        self.version += 1;
        Ok(Some(content))
    }

    /// 放弃内存修改，重新从磁盘加载（M5 外部变更提示时使用）
    #[allow(dead_code)]
    pub fn reload(&mut self) -> Result<()> {
        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("重新加载失败: {}", self.path.display()))?;
        self.rope = Rope::from_str(&content);
        self.dirty = false;
        self.disk_mtime = mtime_of(&self.path).ok();
        self.version += 1;
        Ok(())
    }
}

/// 求 Rope 与文本的公共前缀 / 后缀**字节**长度
///
/// 按**块**（`chunks()`）比较，不逐字符走：ropey 的 `chars()` 每前进一步都要
/// 走一次树游标，实测在 200KB 文档上比 `Rope::from_str` 全量重建还慢三倍；
/// 而按块做的是 memcmp，GB/s 级，成本可以忽略。
///
/// 返回值保证两件事，调用方可以直接拿去切中间段：
/// - `prefix + suffix` 不超过任一侧长度（中间段不会算出负长度）；
/// - 两端都落在 **UTF-8 字符边界**上——`Rope::remove` 对非边界区间会 panic。
fn common_affixes(rope: &Rope, text: &str) -> (usize, usize) {
    let limit = rope.len().min(text.len());

    let prefix = snap_down_to_boundary(text, prefix_bytes(rope, text).min(limit));
    // 后缀不能越过前缀，否则中间段长度变负
    let room = limit - prefix;
    let suffix = snap_down_to_tail_boundary(text, suffix_bytes(rope, text).min(room));

    (prefix, suffix)
}

/// 从头开始，Rope 与文本相同的字节数
fn prefix_bytes(rope: &Rope, text: &str) -> usize {
    let mut prefix = 0usize;
    let mut rest = text.as_bytes();
    for chunk in rope.chunks() {
        if rest.is_empty() {
            break;
        }
        let bytes = chunk.as_bytes();
        let same = first_mismatch(bytes, rest);
        prefix += same;
        // 这一段里就出现了差异：后面不必再看
        if same < bytes.len().min(rest.len()) {
            break;
        }
        rest = &rest[same..];
    }
    prefix
}

/// 从尾开始，Rope 与文本相同的字节数
fn suffix_bytes(rope: &Rope, text: &str) -> usize {
    let mut suffix = 0usize;
    let mut rest = text.as_bytes();
    // `chunks_at` 返回 `(迭代器, 首块内偏移)`；从末尾开始再翻转方向。
    // 注意 ropey 的 `reversed()` 只翻转方向、不移动游标，所以必须先 `chunks_at(len)`。
    for chunk in rope.chunks_at(rope.len()).0.reversed() {
        if rest.is_empty() {
            break;
        }
        let bytes = chunk.as_bytes();
        let same = last_mismatch(bytes, rest);
        suffix += same;
        if same < bytes.len().min(rest.len()) {
            break;
        }
        rest = &rest[..rest.len() - same];
    }
    suffix
}

/// 两个字节切片从头起相同的长度
///
/// 8 字节步长快进（切片相等会编译成单条宽比较），末尾再逐字节收尾。
fn first_mismatch(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0usize;
    while i + 8 <= n && a[i..i + 8] == b[i..i + 8] {
        i += 8;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// 两个字节切片从尾起相同的长度
///
/// 同样按 8 字节步长快进，只是从尾部往回取切片。
fn last_mismatch(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0usize;
    while i + 8 <= n && a[a.len() - 8 - i..a.len() - i] == b[b.len() - 8 - i..b.len() - i] {
        i += 8;
    }
    while i < n && a[a.len() - 1 - i] == b[b.len() - 1 - i] {
        i += 1;
    }
    i
}

/// 把字节下标往回挪到最近的字符边界（UTF-8 最多回退 3 字节）
///
/// 前缀之前的字节两侧完全相同，因此在 `text` 上是边界，在 Rope 上同样是。
fn snap_down_to_boundary(text: &str, ix: usize) -> usize {
    let mut ix = ix.min(text.len());
    while ix > 0 && !text.is_char_boundary(ix) {
        ix -= 1;
    }
    ix
}

/// 把"公共后缀长度"往回挪，使切点落在字符边界上
fn snap_down_to_tail_boundary(text: &str, suffix: usize) -> usize {
    let mut suffix = suffix.min(text.len());
    while suffix > 0 && !text.is_char_boundary(text.len() - suffix) {
        suffix -= 1;
    }
    suffix
}

/// 原子写盘：同目录临时文件 + rename（同一文件系统内 rename 是原子的）
pub fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);

    fs::write(&tmp, content).with_context(|| format!("写临时文件失败: {}", tmp.display()))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            // 清理临时文件，避免残留
            let _ = fs::remove_file(&tmp);
            Err(err).with_context(|| format!("替换文件失败: {}", path.display()))
        }
    }
}

fn mtime_of(path: &Path) -> Result<SystemTime> {
    Ok(fs::metadata(path)?.modified()?)
}

/// 路径是否指向 Markdown 文件
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/note.rs"]
mod tests;
