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

    /// 把编辑控件的 Rope 同步进来（增量，全程不物化整篇文本）
    ///
    /// 只替换**真正变了的那一段**，不重建整棵 Rope，也不物化整篇文本。
    /// 内容未变则不动内容、不置脏、不递增版本。
    ///
    /// 编辑控件内部就是 Rope（`InputBaseState::text()` 借出，零成本），
    /// 因此这里直接吃 `&Rope`；而 `value()` 的实现是
    /// `SharedString::new(self.text.to_string())`——每次调用都物化整个字符串，
    /// 长笔记上每次按键要多付两次全文拷贝。
    pub fn sync_from_rope(&mut self, rope: &Rope) {
        let mine_len = self.rope.len();
        let (prefix, suffix) = rope_common_affixes(&self.rope, rope);
        // 整篇相同：一个字节都不动，也不置脏
        if prefix == mine_len && prefix == rope.len() {
            return;
        }
        let end = mine_len - suffix;
        if end > prefix {
            self.rope.remove(prefix..end);
        }
        // 只物化"变了的那一段"：打字时是 1~2 个字符，不是整篇
        if rope.len() - suffix > prefix {
            let middle = rope.slice(prefix..rope.len() - suffix).to_string();
            self.rope.insert(prefix, &middle);
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

/// 把"待收敛的量"往回挪到最近的 UTF-8 字符边界（最多回退 3 字节）
///
/// `is_boundary` 由调用方给出：前缀传"下标本身"，后缀传"总长 − 后缀"。
/// 收敛只需判一侧——边界之前（之后）的字节两侧完全相同，
/// 因此在 `text` 上是边界，在 Rope 上也同样是。
fn snap_down_to_boundary(amount: usize, is_boundary: impl Fn(usize) -> bool) -> usize {
    let mut n = amount;
    while n > 0 && !is_boundary(n) {
        n -= 1;
    }
    n
}

/// 两个 Rope 从头起相同的字节数（按块 memcmp，两侧都不物化）
fn rope_prefix_bytes(a: &Rope, b: &Rope) -> usize {
    let mut prefix = 0usize;
    let mut a_chunks = a.chunks();
    let mut b_chunks = b.chunks();
    let mut a_rest: &[u8] = b"";
    let mut b_rest: &[u8] = b"";

    loop {
        // 跳过空块：ropey 可能给出空 chunk，不跳会死循环
        while a_rest.is_empty() {
            match a_chunks.next() {
                Some(chunk) => a_rest = chunk.as_bytes(),
                None => return prefix,
            }
        }
        while b_rest.is_empty() {
            match b_chunks.next() {
                Some(chunk) => b_rest = chunk.as_bytes(),
                None => return prefix,
            }
        }
        let same = first_mismatch(a_rest, b_rest);
        prefix += same;
        if same < a_rest.len().min(b_rest.len()) {
            return prefix;
        }
        a_rest = &a_rest[same..];
        b_rest = &b_rest[same..];
    }
}

/// 两个 Rope 从尾起相同的字节数
fn rope_suffix_bytes(a: &Rope, b: &Rope) -> usize {
    let mut suffix = 0usize;
    let mut a_chunks = a.chunks_at(a.len()).0.reversed();
    let mut b_chunks = b.chunks_at(b.len()).0.reversed();
    let mut a_rest: &[u8] = b"";
    let mut b_rest: &[u8] = b"";

    loop {
        while a_rest.is_empty() {
            match a_chunks.next() {
                Some(chunk) => a_rest = chunk.as_bytes(),
                None => return suffix,
            }
        }
        while b_rest.is_empty() {
            match b_chunks.next() {
                Some(chunk) => b_rest = chunk.as_bytes(),
                None => return suffix,
            }
        }
        let same = last_mismatch(a_rest, b_rest);
        suffix += same;
        if same < a_rest.len().min(b_rest.len()) {
            return suffix;
        }
        let a_keep = a_rest.len() - same;
        let b_keep = b_rest.len() - same;
        a_rest = &a_rest[..a_keep];
        b_rest = &b_rest[..b_keep];
    }
}

/// Rope 版的 [`common_affixes`]：语义与返回值保证完全一致
fn rope_common_affixes(a: &Rope, b: &Rope) -> (usize, usize) {
    let limit = a.len().min(b.len());
    let prefix = rope_prefix_bytes(a, b).min(limit);
    let prefix = snap_down_to_boundary(prefix, |ix| a.is_char_boundary(ix));
    // 后缀不能越过前缀，否则中间段长度变负
    let room = limit - prefix;
    let suffix = rope_suffix_bytes(a, b).min(room);
    let suffix = snap_down_to_boundary(suffix, |s| a.is_char_boundary(a.len() - s));
    (prefix, suffix)
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
