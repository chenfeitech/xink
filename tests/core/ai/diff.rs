//! 行级 diff

use crate::core::ai::diff::{DiffLine, diff_lines, diff_stats};

#[test]
fn 改一行() {
    let lines = diff_lines("a\nb\nc\n", "a\nB\nc\n");
    assert_eq!(
        lines,
        vec![
            DiffLine::Same("a".into()),
            DiffLine::Del("b".into()),
            DiffLine::Add("B".into()),
            DiffLine::Same("c".into()),
        ]
    );
    assert_eq!(diff_stats(&lines), (1, 1));
}

#[test]
fn 纯新增() {
    let lines = diff_lines("a\n", "a\nb\n");
    assert_eq!(
        lines,
        vec![DiffLine::Same("a".into()), DiffLine::Add("b".into())]
    );
}

#[test]
fn 完全相同() {
    let lines = diff_lines("a\nb\n", "a\nb\n");
    assert!(lines.iter().all(|l| matches!(l, DiffLine::Same(_))));
}

#[test]
fn 整篇替换() {
    let lines = diff_lines("旧\n", "新\n");
    assert_eq!(
        lines,
        vec![DiffLine::Del("旧".into()), DiffLine::Add("新".into())]
    );
}

#[test]
fn 空输入不炸() {
    assert_eq!(diff_lines("", ""), vec![]);
    assert_eq!(diff_lines("", "a\n"), vec![DiffLine::Add("a".into())]);
}

#[test]
fn 超长文本退化为整段替换() {
    // 超过 LCS 上限（1500 行）时不建大表，直接整段替换
    let before: String = (0..1600).map(|i| format!("old{i}\n")).collect();
    let after: String = (0..1600).map(|i| format!("new{i}\n")).collect();
    let (added, removed) = diff_stats(&diff_lines(&before, &after));
    assert_eq!(added, 1600);
    assert_eq!(removed, 1600);
}
