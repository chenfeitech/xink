//! 目录树渲染相关的纯逻辑测试
//!
//! 注意：这里**不能**用 `use super::*`——本模块顶层有 `use gpui_kit::*;`，
//! 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性并导致宏展开递归。

use std::collections::HashMap;
use std::path::PathBuf;

use super::{FileNode, collect_dir_flags};

fn dir(path: &str, children: Vec<FileNode>) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        name: PathBuf::from(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        is_dir: true,
        children,
    }
}

fn file(path: &str) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        name: PathBuf::from(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        is_dir: false,
        children: vec![],
    }
}

fn flags_of(node: &FileNode) -> HashMap<PathBuf, bool> {
    let mut out = HashMap::new();
    collect_dir_flags(node, &mut out);
    out
}

#[test]
fn empty_folder_is_marked_as_directory() {
    // 回归用例：空文件夹没有子节点，若按"有没有子节点"判断会被误判成文件
    let root = dir("/vault", vec![dir("/vault/empty", vec![])]);
    let flags = flags_of(&root);

    assert_eq!(
        flags.get(&PathBuf::from("/vault/empty")),
        Some(&true),
        "空文件夹也必须标记为目录"
    );
}

#[test]
fn file_is_not_marked_as_directory() {
    let root = dir("/vault", vec![file("/vault/a.md")]);
    let flags = flags_of(&root);

    assert_eq!(flags.get(&PathBuf::from("/vault/a.md")), Some(&false));
    assert_eq!(flags.get(&PathBuf::from("/vault")), Some(&true));
}

#[test]
fn nested_nodes_are_all_collected() {
    let root = dir(
        "/vault",
        vec![
            dir("/vault/deep", vec![dir("/vault/deeper/empty", vec![])]),
            file("/vault/note.md"),
        ],
    );
    let flags = flags_of(&root);

    assert_eq!(flags.len(), 4, "根 + 三层节点都应登记，实际: {flags:?}");
    assert_eq!(
        flags.get(&PathBuf::from("/vault/deeper/empty")),
        Some(&true)
    );
    assert_eq!(flags.get(&PathBuf::from("/vault/note.md")), Some(&false));
}

#[test]
fn lookup_matches_the_render_fallback_semantics() {
    // 复现渲染时的取值方式：查表，查不到才访问磁盘
    let root = dir("/vault", vec![dir("/vault/empty", vec![])]);
    let flags = flags_of(&root);

    let path = PathBuf::from("/vault/empty");
    let is_dir = flags.get(&path).copied().unwrap_or_else(|| path.is_dir());
    assert!(is_dir, "渲染时空文件夹应判定为目录，从而显示文件夹图标");

    let missing = PathBuf::from("/vault/not-scanned");
    let fallback = flags
        .get(&missing)
        .copied()
        .unwrap_or_else(|| missing.is_dir());
    assert!(!fallback, "未登记且不存在的路径应回落到 false");
}
