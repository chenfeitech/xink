//! 改动块（hunk）：把"AI 的改后文本"切成可逐块审阅 / 应用的单元
//!
//! 设计见 `docs/ai/ai-review-hunks-design.md` §3.1。
//!
//! 与 `diff.rs` 的分工：`diff.rs` 只负责算行级差异（`Vec<DiffLine>`），
//! 这里负责**分组、定位、局部应用**——后者才是"接受修订只改这一段"的关键。
//!
//! **定位按内容、不按行号**：接受第 3 块后前两块的行号全变了，行号方案必然错位；
//! 而每块自带它的原文行，逐行匹配能在实际文本里精确找到位置。
//! 匹配到多处或找不到就**不应用**，绝不猜。

use super::diff::{DiffLine, diff_lines};

/// hunk 前后各带几行上下文
pub const DEFAULT_CONTEXT: usize = 2;
/// 评审条最多展示多少块：再多就只能提示用户缩小改动范围
pub const MAX_HUNKS: usize = 50;

/// 定位结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Located {
    /// 唯一匹配：可以直接替换的字节区间
    Unique(std::ops::Range<usize>),
    /// 笔记里找不到这段原文（多半是被手改过）
    NotFound,
    /// 这段原文出现了 `usize` 次，无法确定是哪一处
    Ambiguous(usize),
}

/// 应用失败
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    NotFound,
    Ambiguous(usize),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "原文已改动，这段修订定位不上了"),
            Self::Ambiguous(n) => write!(f, "这段原文出现 {n} 次，无法确定是哪一处"),
        }
    }
}

/// 一个改动块
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// 原文中的起始行号（0-based，仅用于展示）
    pub start_line: usize,
    /// 这块覆盖的**原文**行（Same + Del 的原文侧）
    pub orig_lines: Vec<String>,
    /// 替换**之后**的行（Same + Add 的新侧）
    pub new_lines: Vec<String>,
    /// 展示用序列：上下文 + 改动 + 上下文
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
}

/// 把 before → after 的差异切成若干改动块
///
/// 相邻改动之间隔了 ≤ `2 * context` 行未变行就并成一块——像 `git diff` 那样，
/// 免得一行一改拆出十几个块。
pub fn hunks(before: &str, after: &str, context: usize) -> Vec<Hunk> {
    let lines = diff_lines(before, after);
    // 每条 diff 行对应的原文 / 新文行号
    let mut orig_no = Vec::with_capacity(lines.len());
    let mut new_no = Vec::with_capacity(lines.len());
    let (mut o, mut n) = (0usize, 0usize);
    for line in &lines {
        orig_no.push(o);
        new_no.push(n);
        match line {
            DiffLine::Same(_) => {
                o += 1;
                n += 1;
            }
            DiffLine::Del(_) => o += 1,
            DiffLine::Add(_) => n += 1,
        }
    }

    // 改动行的下标
    let changed: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !matches!(line, DiffLine::Same(_)))
        .map(|(ix, _)| ix)
        .collect();

    let mut out = Vec::new();
    let mut group_start = 0usize;
    while group_start < changed.len() {
        let mut group_end = group_start;
        // 与下一处改动间隔够近就继续并进来
        while group_end + 1 < changed.len()
            && changed[group_end + 1] - changed[group_end] <= 2 * context + 1
        {
            group_end += 1;
        }
        let first = changed[group_start];
        let last = changed[group_end];

        let ctx_start = first.saturating_sub(context);
        let ctx_end = (last + 1 + context).min(lines.len());

        let mut added = 0;
        let mut removed = 0;
        let mut orig_lines = Vec::new();
        let mut new_lines = Vec::new();
        for line in &lines[first..=last] {
            match line {
                DiffLine::Same(t) => {
                    orig_lines.push(t.clone());
                    new_lines.push(t.clone());
                }
                DiffLine::Del(t) => {
                    orig_lines.push(t.clone());
                    removed += 1;
                }
                DiffLine::Add(t) => {
                    new_lines.push(t.clone());
                    added += 1;
                }
            }
        }

        out.push(Hunk {
            start_line: orig_no[first],
            orig_lines,
            new_lines,
            lines: lines[ctx_start..ctx_end].to_vec(),
            added,
            removed,
        });

        if out.len() >= MAX_HUNKS {
            break;
        }
        group_start = group_end + 1;
    }

    out
}

/// 每行的字节区间 `(起始, 结束)`；结束不含换行符
fn line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut offset = 0usize;
    for part in text.split_inclusive('\n') {
        // 去掉换行符本身（可能是 \n 或 \r\n）
        let end = offset + part.trim_end_matches(['\n', '\r']).len();
        spans.push((offset, end));
        offset += part.len();
    }
    spans
}

/// 在笔记原文里定位这个 hunk 的字节区间
///
/// 比较时忽略行尾空白（`trim_end`），但返回的区间用的是笔记里的**实际**行，
/// 因此替换后前后文逐字不变。
pub fn locate_range(note: &str, hunk: &Hunk) -> Located {
    if hunk.orig_lines.is_empty() {
        // 纯新增：没有原文可锚定，交给调用方走"插入到光标处"
        return Located::NotFound;
    }
    let spans = line_spans(note);
    let note_lines: Vec<&str> = spans.iter().map(|(s, e)| &note[*s..*e]).collect();
    let needle = &hunk.orig_lines;
    if needle.len() > note_lines.len() {
        return Located::NotFound;
    }

    let mut found: Option<std::ops::Range<usize>> = None;
    let mut count = 0usize;
    for start in 0..=note_lines.len() - needle.len() {
        let hit = needle
            .iter()
            .zip(note_lines[start..].iter())
            .all(|(want, got)| want.trim_end() == got.trim_end());
        if !hit {
            continue;
        }
        count += 1;
        if count == 1 {
            let byte_start = spans[start].0;
            let last = start + needle.len() - 1;
            let mut byte_end = spans[last].1;
            // 整块被删掉时把行尾换行一起带走，不留空行
            if hunk.new_lines.is_empty()
                && byte_end < note.len()
                && note[byte_end..].starts_with('\n')
            {
                byte_end += 1;
            }
            found = Some(byte_start..byte_end);
        }
        if count > 1 {
            return Located::Ambiguous(count);
        }
    }

    found.map(Located::Unique).unwrap_or(Located::NotFound)
}

/// 只替换这个 hunk 覆盖的那一段，其余内容逐字不动
pub fn apply_hunk(note: &str, hunk: &Hunk) -> Result<String, PatchError> {
    let range = match locate_range(note, hunk) {
        Located::Unique(range) => range,
        Located::NotFound => return Err(PatchError::NotFound),
        Located::Ambiguous(n) => return Err(PatchError::Ambiguous(n)),
    };
    let mut out = String::with_capacity(note.len() + hunk.new_lines.len() * 32);
    out.push_str(&note[..range.start]);
    out.push_str(&hunk.new_lines.join("\n"));
    out.push_str(&note[range.end..]);
    Ok(out)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/patch.rs"]
mod tests;
