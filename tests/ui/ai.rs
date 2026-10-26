//! AI 对话条分块的纯逻辑测试
//!
//! 注意：这里**不能**用 `use super::*`——本模块顶层有 `use gpui_kit::*;`，
//! 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性并导致宏展开递归。
//!
//! 只测不依赖窗口的那部分：代码围栏的转义、折叠状态的初值与切换。
//! "能不能选中 / 复制"是框架 `TextView` 的行为，只能人工验收。

use super::{Block, code_fence};

#[test]
fn code_fence_wraps_text_in_three_backticks() {
    let fenced = code_fence("一行结果");
    assert!(fenced.starts_with("```\n"), "{fenced:?}");
    assert!(fenced.ends_with("\n```"), "{fenced:?}");
    assert!(fenced.contains("一行结果"));
}

#[test]
fn code_fence_grows_past_content_backticks() {
    // 结果里出现 ``` 时，围栏必须比它长，否则代码块会提前闭合
    let fenced = code_fence("a\n```\nb");
    assert!(
        fenced.starts_with("````\n"),
        "围栏应长于内容里的反引号串：{fenced:?}"
    );
}

#[test]
fn code_fence_handles_empty_output() {
    assert_eq!(code_fence(""), "```\n\n```");
}

#[test]
fn process_blocks_start_collapsed() {
    // 过程性信息默认收起：只留一行摘要，不看细节时不占地方
    assert!(Block::tool("read_note", "a.md").is_collapsed());
    assert!(Block::verify("第 3 行：[[不存在]]".into()).is_collapsed());
    // 对话正文默认展开
    assert!(!Block::user("问一句".into()).is_collapsed());
    assert!(!Block::assistant("答一句".into()).is_collapsed());
}

#[test]
fn toggle_flips_collapsed_back_and_forth() {
    let mut block = Block::tool("search_notes", "关键词");
    block.toggle();
    assert!(!block.is_collapsed());
    block.toggle();
    assert!(block.is_collapsed());
}

#[test]
fn only_assistant_accepts_streamed_text() {
    let mut block = Block::assistant(String::new());
    assert!(block.as_assistant_mut().is_some());
    let mut block = Block::tool("outline", "a.md");
    assert!(block.as_assistant_mut().is_none());
}
