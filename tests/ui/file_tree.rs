//! 目录树渲染相关的纯逻辑测试
//!
//! 注意：这里**不能**用 `use super::*`——本模块顶层有 `use gpui_kit::*;`，
//! 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性并导致宏展开递归。

use std::collections::HashMap;
use std::path::PathBuf;

use super::{FileNode, build_tree, collect_dir_flags, to_tree_item};
use gpui_kit::component::tree::TreeItem;

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

// ── 保状态合并（目录树不再整体收缩）──────────────────────────
//
// 设计见 docs/file-tree-incremental-refresh-design.md §7.1。
// 展开状态存在 `TreeItem` 自己的 `Rc<RefCell<_>>` 里，合并必须复用旧项。

/// 把 `path` 处的节点展开（模拟用户点开过），返回展开后的新树。
///
/// 注意：`TreeItem::clone()` 共享 `state` 的 `Rc`，因此展开副本等价于展开原件——
/// 这正是合并能保留展开状态的底层机制（见 `cloned_item_shares_state_…` 用例）。
fn expanded_at(item: &TreeItem, path: &str) -> TreeItem {
    let mut out = item.clone();
    if out.id.as_ref() == path {
        return out.expanded(true);
    }
    out.children = out.children.iter().map(|c| expanded_at(c, path)).collect();
    out
}

fn find<'a>(item: &'a TreeItem, path: &str) -> Option<&'a TreeItem> {
    if item.id.as_ref() == path {
        return Some(item);
    }
    item.children.iter().find_map(|child| find(child, path))
}

fn ids(item: &TreeItem) -> Vec<String> {
    let mut out = vec![item.id.to_string()];
    for child in &item.children {
        out.extend(ids(child));
    }
    out
}

#[test]
fn merge_keeps_sibling_expansion_when_a_child_is_added() {
    let old = build_tree(
        &[],
        &dir("/vault", vec![dir("/vault/a", vec![file("/vault/a/1.md")])]),
    );
    // 用户展开了 A
    let old = vec![expanded_at(&old[0], "/vault/a")];

    let new = dir(
        "/vault",
        vec![
            dir(
                "/vault/a",
                vec![file("/vault/a/1.md"), file("/vault/a/2.md")],
            ),
            file("/vault/b.md"),
        ],
    );
    let merged = build_tree(&old, &new);

    assert!(merged[0].is_expanded(), "根展开状态应保留");
    assert!(
        find(&merged[0], "/vault/a").is_some_and(|item| item.is_expanded()),
        "A 的展开状态必须保留：这是「整体收缩」的直接回归点"
    );
    assert!(find(&merged[0], "/vault/a/2.md").is_some(), "新节点应出现");
    assert!(find(&merged[0], "/vault/b.md").is_some());
}

#[test]
fn merge_keeps_sibling_expansion_when_a_child_is_removed() {
    let old = build_tree(
        &[],
        &dir(
            "/vault",
            vec![
                dir("/vault/a", vec![file("/vault/a/1.md")]),
                dir("/vault/b", vec![file("/vault/b/1.md")]),
            ],
        ),
    );
    let old = vec![expanded_at(&old[0], "/vault/b")];

    // 删掉 a/1.md
    let new = dir(
        "/vault",
        vec![
            dir("/vault/a", vec![]),
            dir("/vault/b", vec![file("/vault/b/1.md")]),
        ],
    );
    let merged = build_tree(&old, &new);

    assert!(
        find(&merged[0], "/vault/a/1.md").is_none(),
        "被删节点应消失"
    );
    assert!(
        find(&merged[0], "/vault/b").is_some_and(|item| item.is_expanded()),
        "未受影响的兄弟目录必须保持原状"
    );
}

#[test]
fn merge_rebuilds_the_renamed_node_only() {
    let old = build_tree(
        &[],
        &dir("/vault", vec![dir("/vault/a", vec![file("/vault/a/1.md")])]),
    );
    let old = vec![expanded_at(&old[0], "/vault/a")];

    // a/1.md → a/2.md（id 变化）
    let new = dir("/vault", vec![dir("/vault/a", vec![file("/vault/a/2.md")])]);
    let merged = build_tree(&old, &new);

    assert!(find(&merged[0], "/vault/a/1.md").is_none());
    assert!(find(&merged[0], "/vault/a/2.md").is_some());
    assert!(
        find(&merged[0], "/vault/a").is_some_and(|item| item.is_expanded()),
        "祖先目录不受影响"
    );
}

#[test]
fn merge_preserves_deep_expansion() {
    let old = build_tree(
        &[],
        &dir(
            "/vault",
            vec![dir(
                "/vault/l1",
                vec![dir("/vault/l1/l2", vec![dir("/vault/l1/l2/l3", vec![])])],
            )],
        ),
    );
    // 逐层展开到 l3
    fn expand_all(item: &TreeItem) -> TreeItem {
        let mut out = item.clone();
        out.children = out.children.iter().map(expand_all).collect();
        out.expanded(true)
    }
    let old = vec![expand_all(&old[0])];

    let new = dir(
        "/vault",
        vec![dir(
            "/vault/l1",
            vec![dir(
                "/vault/l1/l2",
                vec![dir("/vault/l1/l2/l3", vec![file("/vault/l1/l2/l3/x.md")])],
            )],
        )],
    );
    let merged = build_tree(&old, &new);

    for path in ["/vault/l1", "/vault/l1/l2", "/vault/l1/l2/l3"] {
        assert!(
            find(&merged[0], path).is_some_and(|item| item.is_expanded()),
            "{path} 的展开状态必须保留"
        );
    }
}

#[test]
fn different_root_id_rebuilds_the_whole_tree() {
    let old = build_tree(&[], &dir("/vault", vec![file("/vault/a.md")]));
    let new = dir("/other", vec![file("/other/b.md")]);
    let merged = build_tree(&old, &new);

    assert_eq!(merged[0].id.as_ref(), "/other");
    assert!(merged[0].is_expanded(), "新树要展开根（首次加载 / 切库）");
    assert!(find(&merged[0], "/vault/a.md").is_none());
}

#[test]
fn merge_keeps_the_scan_order() {
    let old = build_tree(&[], &dir("/vault", vec![file("/vault/a.md")]));
    let new = dir(
        "/vault",
        vec![
            file("/vault/b.md"),
            file("/vault/a.md"),
            file("/vault/c.md"),
        ],
    );
    let merged = build_tree(&old, &new);

    let labels: Vec<String> = merged[0]
        .children
        .iter()
        .map(|item| item.id.to_string())
        .collect();
    assert_eq!(
        labels,
        vec!["/vault/b.md", "/vault/a.md", "/vault/c.md"],
        "顺序以扫描结果为准，不继承旧顺序"
    );
}

#[test]
fn cloned_item_shares_state_so_replacing_children_is_safe() {
    // 锁定 merge 依赖的底层语义：clone 共享 state，改副本的 children 不影响原件
    let item = to_tree_item(&dir("/vault", vec![file("/vault/a.md")])).expanded(true);
    let mut copy = item.clone();
    copy.children = vec![to_tree_item(&file("/vault/b.md"))];

    assert!(item.is_expanded(), "原件状态不受影响");
    assert_eq!(item.children.len(), 1, "原件的子节点未被改写");
    assert_eq!(copy.children.len(), 1);
    assert!(copy.is_expanded(), "副本继承展开状态");
}

#[test]
fn merge_updates_label_after_rename() {
    let old = build_tree(&[], &dir("/vault", vec![file("/vault/Note.md")]));
    let old = vec![old[0].clone()];

    // 仅大小写变化：label 必须以磁盘为准
    let mut renamed = file("/vault/Note.md");
    renamed.name = "note.md".to_string();
    let new = dir("/vault", vec![renamed]);

    let merged = build_tree(&old, &new);
    let child = find(&merged[0], "/vault/Note.md").expect("路径未变，应复用旧项");
    assert_eq!(
        child.label.as_ref(),
        "note.md",
        "label 应更新为磁盘上的名字"
    );
    assert!(ids(&merged[0]).contains(&"/vault/Note.md".to_string()));
}
