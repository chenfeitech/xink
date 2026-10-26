//! 结构校验：双链 / frontmatter / tags

use std::path::PathBuf;

use super::{IssueKind, LinkIndex, VerifyIssue, check};
use crate::core::index::Resolution;

/// 只有 `a` 存在、`dup` 有两篇
struct MemIndex;

impl LinkIndex for MemIndex {
    fn resolve(&self, target: &str) -> Resolution {
        match target.trim().to_lowercase().as_str() {
            "a" => Resolution::Hit(PathBuf::from("/vault/a.md")),
            "dup" => Resolution::Ambiguous(vec![
                PathBuf::from("/vault/x/dup.md"),
                PathBuf::from("/vault/y/dup.md"),
            ]),
            _ => Resolution::Missing,
        }
    }
}

fn kinds(issues: &[VerifyIssue]) -> Vec<IssueKind> {
    issues.iter().map(|i| i.kind).collect()
}

#[test]
fn healthy_note_has_no_issues() {
    let text = "# 标题\n\n正文提到了 [[a]]。\n";
    assert!(check(text, Some(&MemIndex)).is_empty());
}

#[test]
fn missing_wikilink_is_reported_with_line() {
    // `[[不存在]]` 在第 3 行
    let text = "# 标题\n\n见 [[不存在]]。\n";
    let issues = check(text, Some(&MemIndex));
    assert_eq!(kinds(&issues), vec![IssueKind::WikiLink]);
    assert_eq!(issues[0].line, 3);
    assert!(issues[0].detail.contains("不存在"), "{:?}", issues[0]);
    // 回灌给模型的文本要带行号，模型才知道改哪
    assert!(issues[0].as_feedback().contains("第 3 行"));
}

#[test]
fn ambiguous_wikilink_is_reported() {
    let text = "见 [[dup]]\n";
    let issues = check(text, Some(&MemIndex));
    assert_eq!(kinds(&issues), vec![IssueKind::WikiLink]);
    assert!(issues[0].detail.contains("歧义"), "{:?}", issues[0]);
}

#[test]
fn wikilink_check_is_skipped_without_index() {
    // 没打开库：双链无从判断，不该报"不存在"
    let text = "见 [[不存在]]\n";
    assert!(check(text, None).is_empty());
}

#[test]
fn valid_frontmatter_passes() {
    let text = "---\ntitle: 标题\ntags:\n  - rust\n  - 笔记\n---\n\n正文\n";
    assert!(check(text, Some(&MemIndex)).is_empty());
}

#[test]
fn broken_yaml_is_reported() {
    let text = "---\ntitle: 标题\n  bad indent: [\n---\n\n正文\n";
    let issues = check(text, Some(&MemIndex));
    assert!(
        kinds(&issues).contains(&IssueKind::Frontmatter),
        "{issues:?}"
    );
}

#[test]
fn unclosed_frontmatter_is_reported() {
    let text = "---\ntitle: 标题\n\n正文，但 --- 没闭合\n";
    let issues = check(text, Some(&MemIndex));
    assert_eq!(kinds(&issues), vec![IssueKind::Frontmatter]);
    assert!(issues[0].detail.contains("没有闭合"), "{:?}", issues[0]);
}

#[test]
fn tags_must_be_a_list() {
    // 写成数字：`tags: 42`
    let text = "---\ntags: 42\n---\n\n正文\n";
    let issues = check(text, Some(&MemIndex));
    assert_eq!(kinds(&issues), vec![IssueKind::Tag]);

    // 列表里有非字符串
    let text = "---\ntags:\n  - rust\n  - 42\n---\n\n正文\n";
    let issues = check(text, Some(&MemIndex));
    assert_eq!(kinds(&issues), vec![IssueKind::Tag]);

    // 单标签是允许的
    let text = "---\ntags: rust\n---\n\n正文\n";
    assert!(check(text, Some(&MemIndex)).is_empty());
}

#[test]
fn no_frontmatter_is_fine() {
    // 正文里的 `---` 分隔线不该被当成 frontmatter：它不在第一行
    let text = "# 标题\n\n---\n\n另一段\n";
    assert!(check(text, Some(&MemIndex)).is_empty());
}
