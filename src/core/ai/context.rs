//! 上下文装配：决定"发给模型的是什么"
//!
//! 三档（选中 / 整篇 / 从光标起）+ 字符预算。不做精确 tokenizer——
//! 中文按 1 字 ≈ 1 token、西文 4 字符 ≈ 1 token 折算，够用来截断即可。

/// 上下文的作用范围
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextScope {
    /// 选中文本；为空时回落整篇
    Selection,
    /// 整篇
    WholeNote,
    /// 从光标到文末
    FromCursor,
}

/// 装配结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedContext {
    /// 实际发给模型的文本
    pub text: String,
    /// 是否被截断
    pub truncated: bool,
    /// 给用户看的说明（如「已截断：第 12–40 行」）
    pub notice: Option<String>,
}

/// 粗估 token 数：宽字符（CJK 等）按 1，其余按 4 字符 1 token
pub fn estimate_tokens(text: &str) -> usize {
    let wide = text.chars().filter(|c| is_wide(*c)).count();
    let narrow = text.chars().count() - wide;
    wide + (narrow + 3) / 4
}

fn is_wide(ch: char) -> bool {
    matches!(ch,
        '\u{1100}'..='\u{115F}'
        | '\u{2E80}'..='\u{A4CF}'
        | '\u{AC00}'..='\u{D7A3}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FE30}'..='\u{FE6F}'
        | '\u{FF00}'..='\u{FF60}'
        | '\u{FFE0}'..='\u{FFE6}'
        | '\u{20000}'..='\u{3FFFD}'
    )
}

/// 装配上下文
pub fn assemble(
    scope: ContextScope,
    selection: &str,
    note: &str,
    cursor: usize,
    budget_chars: usize,
) -> PreparedContext {
    let body = match scope {
        ContextScope::Selection if !selection.trim().is_empty() => selection.to_string(),
        ContextScope::FromCursor => slice_from(note, cursor),
        _ => note.to_string(),
    };

    if body.chars().count() <= budget_chars {
        return PreparedContext { text: body, truncated: false, notice: None };
    }

    let (text, start, end) = window(&body, budget_chars);
    PreparedContext {
        text,
        truncated: true,
        notice: Some(format!("内容超过预算，本次只发送第 {}–{} 行", start + 1, end + 1)),
    }
}

fn slice_from(note: &str, cursor: usize) -> String {
    let offset = note
        .char_indices()
        .nth(cursor.min(note.chars().count()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    note[offset..].to_string()
}

/// 按字符预算取前 N 个字符，返回（文本，起始行，结束行）
///
/// 从开头取而不是从中间取：笔记的开头通常就是标题与要点；
/// `FromCursor` 传入的 body 本身已经从光标截断了，因此同样是取前缀。
fn window(text: &str, budget: usize) -> (String, usize, usize) {
    let take = budget.min(text.chars().count());
    let end_byte = text
        .char_indices()
        .nth(take)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let slice = &text[..end_byte];
    let end_line = slice.matches('\n').count() + 1;
    (slice.to_string(), 0, end_line)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/context.rs"]
mod tests;
