//! 双链补全：写入 `[[...]]` 时目标文本的选取规则
//!
//! 只测 `link_target` 这个纯函数：候选排序依赖 `Entity<Index>`，
//! 浮层生命周期依赖 `Window`，都测不了（全书见 `docs/TESTS.md`）。

use std::path::{Path, PathBuf};

use super::link_target;

#[test]
fn 库内文件写相对路径() {
    let root = Path::new("/vault");
    let path = PathBuf::from("/vault/notes/a.md");
    assert_eq!(link_target(Some(root), &path, "标题"), "notes/a.md");
}

#[test]
fn 库根目录里的文件没有前缀() {
    let root = Path::new("/vault");
    let path = PathBuf::from("/vault/a.md");
    assert_eq!(link_target(Some(root), &path, "标题"), "a.md");
}

#[test]
fn 没有库根时退回文件名() {
    let path = PathBuf::from("/vault/notes/a.md");
    // 没有库根就没法算相对路径，写 `stem` 最短且够用
    assert_eq!(link_target(None, &path, "标题"), "a");
}

#[test]
fn 路径不在库内时退回文件名() {
    let root = Path::new("/vault");
    let path = PathBuf::from("/elsewhere/notes/a.md");
    assert_eq!(link_target(Some(root), &path, "标题"), "a");
}

#[test]
fn 没有文件名时退回标题() {
    // `..` 没有 file_stem，只能拿标题顶上
    let path = PathBuf::from("/vault/notes/..");
    assert_eq!(link_target(None, &path, "我的标题"), "我的标题");
}

#[test]
fn 相对路径带分隔符能区分同名文件() {
    let root = Path::new("/vault");
    let one = PathBuf::from("/vault/a/note.md");
    let two = PathBuf::from("/vault/b/note.md");
    // 这是"一律写相对路径"而不是"优先文件名"的理由
    assert_ne!(
        link_target(Some(root), &one, "n"),
        link_target(Some(root), &two, "n")
    );
}
