// 显式导入而非 `use super::*`：本文件顶部有 `use gpui_kit::*`，
// 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性，导致宏展开递归。
//
// 这里只测**不依赖 App 上下文**的纯函数：候选的收集与相对路径，
// 面板的交互（Dialog / Command）按设计文档走人工验收。
use std::path::{Path, PathBuf};

use super::{Candidate, collect_candidates, flatten};
use crate::core::vault::FileNode;

fn node(path: &str, is_dir: bool) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        name: Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        is_dir,
        children: Vec::new(),
    }
}

fn collect(root: &str, tree: &FileNode) -> Vec<Candidate> {
    let mut out = Vec::new();
    flatten(Path::new(root), tree, &mut out);
    out
}

#[test]
fn flatten_collects_editable_files_only() {
    let root = "/vault";
    let mut dir = node("/vault/notes", true);
    dir.children = vec![
        node("/vault/notes/a.md", false),
        node("/vault/notes/config.json", false),
        node("/vault/notes/logo.png", false),
        node("/vault/notes/movie.mp4", false),
    ];

    let candidates = collect(root, &dir);
    let labels: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect();
    assert_eq!(labels, vec!["notes/a.md", "notes/config.json"]);
}

#[test]
fn label_is_relative_to_the_vault_root() {
    let mut dir = node("/vault/sub", true);
    dir.children = vec![node("/vault/sub/deep/note.md", false)];

    let candidates = collect("/vault", &dir);
    assert_eq!(candidates[0].label, "sub/deep/note.md");
    assert_eq!(candidates[0].path, PathBuf::from("/vault/sub/deep/note.md"));
}

#[test]
fn nested_directories_are_walked() {
    let mut inner = node("/vault/a/b", true);
    inner.children = vec![node("/vault/a/b/c.md", false)];
    let mut outer = node("/vault/a", true);
    outer.children = vec![inner, node("/vault/a/top.md", false)];

    assert_eq!(collect("/vault", &outer).len(), 2);
}

#[test]
fn empty_directory_yields_no_candidate() {
    assert!(collect("/vault", &node("/vault/empty", true)).is_empty());
}

#[test]
fn collect_candidates_scans_a_real_vault() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("建子目录");
    std::fs::write(root.join("b.md"), "").unwrap();
    std::fs::write(root.join("notes/a.md"), "").unwrap();
    std::fs::write(root.join("notes/skip.png"), "").unwrap();
    // 隐藏文件与 .git 目录都不该进候选（与目录树一致）
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/config.json"), "{}").unwrap();

    let candidates = collect_candidates(root).expect("扫描成功");
    let labels: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect();

    // 按相对路径排序：空查询时的展示顺序必须稳定
    assert_eq!(labels, vec!["b.md", "notes/a.md"]);
    assert!(candidates[0].path.starts_with(root));
}
