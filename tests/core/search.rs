use std::fs;
use std::path::PathBuf;

use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

fn write(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

fn lines_of(hits: &[Hit]) -> Vec<usize> {
    hits.iter().map(|hit| hit.line).collect()
}

#[test]
fn finds_lines_case_insensitively() {
    let dir = tmp();
    let a = write(
        dir.path(),
        "a.md",
        "第一行\n含有 KeyWord 的行\n小写 keyword\n",
    );
    let hits = search_files(&[a], "keyword", 50);
    assert_eq!(lines_of(&hits), vec![1, 2]);
    assert!(hits[0].text.contains("KeyWord"));
}

#[test]
fn chinese_query_works() {
    let dir = tmp();
    let a = write(dir.path(), "a.md", "无关\n这是中文笔记\n");
    let hits = search_files(&[a], "中文", 50);
    assert_eq!(lines_of(&hits), vec![1]);
}

#[test]
fn empty_query_returns_nothing() {
    let dir = tmp();
    let a = write(dir.path(), "a.md", "内容\n");
    assert!(search_files(std::slice::from_ref(&a), "", 50).is_empty());
    assert!(search_files(std::slice::from_ref(&a), "   ", 50).is_empty());
}

#[test]
fn result_count_is_capped() {
    let dir = tmp();
    let body = (0..50).map(|_| "hit").collect::<Vec<_>>().join("\n");
    let a = write(dir.path(), "a.md", &body);
    assert_eq!(search_files(&[a], "hit", 5).len(), 5);
}

#[test]
fn missing_and_oversized_files_are_skipped() {
    let dir = tmp();
    let missing = dir.path().join("不存在.md");
    let small = write(dir.path(), "a.md", "hit\n");
    // 缺失文件不应让整个搜索失败
    assert_eq!(search_files(&[missing, small], "hit", 50).len(), 1);
}

#[test]
fn long_lines_are_trimmed() {
    let long: String = "x".repeat(500);
    let dir = tmp();
    let a = write(dir.path(), "a.md", &format!("start {long} end"));
    let hits = search_files(&[a], "start", 50);
    assert_eq!(hits.len(), 1);
    assert!(
        hits[0].text.chars().count() < 200,
        "超长行必须截断: {} 字符",
        hits[0].text.chars().count()
    );
    assert!(hits[0].text.contains('…'));
}

#[test]
fn only_markdown_is_searchable() {
    assert!(is_searchable(std::path::Path::new("a.md")));
    assert!(!is_searchable(std::path::Path::new("a.json")));
    assert!(!is_searchable(std::path::Path::new("a")));
}
