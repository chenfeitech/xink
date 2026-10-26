//! 上下文装配与预算

use crate::core::ai::context::{assemble, estimate_tokens, ContextScope};

#[test]
fn 中文按宽字符计() {
    assert_eq!(estimate_tokens("你好"), 2);
    // 4 个西文字符 ≈ 1 token
    assert_eq!(estimate_tokens("abcd"), 1);
}

#[test]
fn 选中优先且超预算时截断() {
    let prepared = assemble(
        ContextScope::Selection,
        "选中内容",
        "整篇整篇整篇",
        0,
        100,
    );
    assert_eq!(prepared.text, "选中内容");
    assert!(!prepared.truncated);

    let prepared = assemble(ContextScope::Selection, &"字".repeat(50), "整篇", 0, 10);
    assert!(prepared.truncated);
    assert!(prepared.notice.is_some());
    assert_eq!(prepared.text.chars().count(), 10);
}

#[test]
fn 空选中回落整篇() {
    let prepared = assemble(ContextScope::Selection, "   ", "整篇内容", 0, 100);
    assert_eq!(prepared.text, "整篇内容");
}

#[test]
fn 从光标起只取光标之后() {
    let prepared = assemble(ContextScope::FromCursor, "", "abcdef", 3, 100);
    assert_eq!(prepared.text, "def");
}
