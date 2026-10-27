//! AI 会话持久化的测试
//!
//! 只测纯逻辑（裁剪 / 表操作），文件读写靠人工验收——`app_data_dir()` 在测试里
//! 指向真实的用户目录，不该被测试碰。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::apply_note;

use super::{MAX_BLOCKS, MAX_SESSIONS, StoredBlock, trim};

fn block(ix: usize) -> StoredBlock {
    StoredBlock::User {
        text: format!("第 {ix} 句"),
    }
}

fn store_of(path: &str, count: usize) -> HashMap<PathBuf, Vec<StoredBlock>> {
    let mut all = HashMap::new();
    all.insert(
        PathBuf::from(path),
        (0..count).map(block).collect::<Vec<_>>(),
    );
    all
}

#[test]
fn trim_keeps_every_block_under_the_cap() {
    let mut all = store_of("a.md", MAX_BLOCKS + 5);
    trim(&mut all);
    let kept = &all[&PathBuf::from("a.md")];
    assert_eq!(kept.len(), MAX_BLOCKS);
    // 丢的是最旧的，保留的是最新的
    assert_eq!(kept.first(), Some(&block(5)));
    assert_eq!(kept.last(), Some(&block(MAX_BLOCKS + 4)));
}

#[test]
fn trim_leaves_short_sessions_alone() {
    let mut all = store_of("a.md", 3);
    trim(&mut all);
    assert_eq!(all[&PathBuf::from("a.md")].len(), 3);
}

#[test]
fn trim_caps_the_number_of_sessions() {
    let mut all: HashMap<PathBuf, Vec<StoredBlock>> = (0..MAX_SESSIONS + 7)
        .map(|ix| (PathBuf::from(format!("note-{ix:03}.md")), vec![block(ix)]))
        .collect();
    trim(&mut all);
    assert_eq!(all.len(), MAX_SESSIONS, "会话数应被裁到上限");
    // 裁剪结果必须稳定：同样的输入跑两次结果一致
    let mut again: HashMap<PathBuf, Vec<StoredBlock>> = (0..MAX_SESSIONS + 7)
        .map(|ix| (PathBuf::from(format!("note-{ix:03}.md")), vec![block(ix)]))
        .collect();
    trim(&mut again);
    assert_eq!(all, again);
}

// ── 表操作（不碰真实目录） ────────────────────────────────────
//
// `save_note` 本身写在 `app_data_dir()`，测试不能碰，因此把"空列表等于删除"
// 这条规则抽成了 `apply_note` 在这里覆盖。

fn table() -> HashMap<PathBuf, Vec<StoredBlock>> {
    let mut all = HashMap::new();
    all.insert(PathBuf::from("/vault/a.md"), vec![block(1)]);
    all
}

#[test]
fn apply_note_overwrites_an_existing_session() {
    let mut all = table();
    apply_note(&mut all, Path::new("/vault/a.md"), &[block(2)]);
    assert_eq!(all.len(), 1, "不该多出一条");
    assert_eq!(all[Path::new("/vault/a.md")], vec![block(2)]);
}

#[test]
fn apply_note_adds_a_new_session() {
    let mut all = table();
    apply_note(&mut all, Path::new("/vault/b.md"), &[block(3)]);
    assert_eq!(all.len(), 2);
}

#[test]
fn apply_note_with_no_blocks_deletes_the_session() {
    // 清空对话 = 删掉这一篇的记录，而不是留一条空会话
    let mut all = table();
    apply_note(&mut all, Path::new("/vault/a.md"), &[]);
    assert!(all.is_empty(), "空块列表等于删除");

    // 删一篇不存在的：不能把空条目插进去
    apply_note(&mut all, Path::new("/vault/nope.md"), &[]);
    assert!(all.is_empty());
}
