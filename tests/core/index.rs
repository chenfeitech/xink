use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

fn note(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, body).unwrap();
    path
}

fn index_of(root: &Path) -> NoteIndex {
    NoteIndex::build(root).expect("构建索引")
}

// ── 构建范围 ──────────────────────────────────────────────────

#[test]
fn build_collects_nested_markdown_only() {
    let dir = tmp();
    let root = dir.path();
    note(root, "a.md", "# A");
    note(root, "sub/b.md", "# B");
    note(root, "sub/deep/c.md", "# C");
    // 数据文件与图片不进索引
    note(root, "conf.json", "{}");
    note(root, "logo.png", "x");

    let index = index_of(root);
    assert_eq!(index.len(), 3, "只有 .md 进索引");
    assert_eq!(index.root(), Some(root));
}

#[test]
fn build_is_empty_when_no_vault() {
    let index = NoteIndex::new();
    assert!(index.is_empty());
    assert_eq!(index.root(), None);
    assert!(index.backlinks(Path::new("/x/a.md")).is_empty());
    assert!(index.tags().is_empty());
    assert_eq!(index.resolve_wikilink("a"), Resolution::Missing);
}

// ── 标题 ──────────────────────────────────────────────────────

#[test]
fn title_prefers_h1_then_any_heading_then_filename() {
    let dir = tmp();
    let root = dir.path();
    let h1 = note(root, "h1.md", "前言\n# 真正的标题\n## 小节\n");
    let other = note(root, "other.md", "## 只有二级\n");
    let plain = note(root, "plain.md", "没有标题\n");

    let index = index_of(root);
    assert_eq!(index.note(&h1).unwrap().title, "真正的标题");
    assert_eq!(index.note(&other).unwrap().title, "只有二级");
    assert_eq!(index.note(&plain).unwrap().title, "plain", "退化到文件名");
}

// ── 反向链接 ──────────────────────────────────────────────────

#[test]
fn backlinks_list_sources_with_lines() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "target.md", "# 目标\n");
    note(root, "one.md", "见 [[target]]\n尾行\n");
    note(root, "sub/two.md", "也见 [[target|别名]]\n");

    let index = index_of(root);
    let refs = index.backlinks(&target);
    assert_eq!(refs.len(), 2, "{refs:?}");
    assert_eq!(refs[0].from, root.join("one.md"));
    assert_eq!(refs[0].line, 0);
    assert_eq!(refs[0].target, "target");
    assert_eq!(refs[1].from, root.join("sub/two.md"));
    assert_eq!(refs[1].target, "target");
}

#[test]
fn backlinks_ignore_self_and_disappear_after_removal() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "target.md", "自引 [[target]]\n");
    let source = note(root, "source.md", "引用 [[target]]\n");

    let mut index = index_of(root);
    assert!(
        index.backlinks(&target).iter().all(|r| r.from != target),
        "自引不算反链"
    );

    index.remove_file(&source);
    assert!(index.backlinks(&target).is_empty(), "来源被删后反链消失");
}

// ── 标签 ──────────────────────────────────────────────────────

#[test]
fn tags_are_aggregated_case_insensitively() {
    let dir = tmp();
    let root = dir.path();
    note(root, "a.md", "#rust #想法\n");
    note(root, "b.md", "#Rust #想法\n");
    note(root, "c.md", "#日记\n");

    let index = index_of(root);
    let tags = index.tags();
    assert_eq!(
        tags,
        vec![
            // 数量相同按字典序（ASCII 在前）
            ("rust".to_string(), 2),
            ("想法".to_string(), 2),
            ("日记".to_string(), 1)
        ],
        "大小写归一 + 按数量降序：{tags:?}"
    );
    assert_eq!(
        index.notes_with_tag("#RUST"),
        vec![root.join("a.md"), root.join("b.md")]
    );
}

// ── 双链解析 ──────────────────────────────────────────────────

#[test]
fn wikilink_resolves_by_relative_path_filename_and_title() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "sub/target.md", "# 真正的标题\n");

    let index = index_of(root);
    assert_eq!(
        index.resolve_wikilink("sub/target"),
        Resolution::Hit(target.clone())
    );
    assert_eq!(
        index.resolve_wikilink("sub/target.md"),
        Resolution::Hit(target.clone())
    );
    assert_eq!(
        index.resolve_wikilink("target"),
        Resolution::Hit(target.clone())
    );
    assert_eq!(
        index.resolve_wikilink("真正的标题"),
        Resolution::Hit(target)
    );
    assert_eq!(index.resolve_wikilink("不存在"), Resolution::Missing);
    assert_eq!(index.resolve_wikilink(""), Resolution::Missing);
}

#[test]
fn wikilink_reports_ambiguity_for_duplicate_names() {
    let dir = tmp();
    let root = dir.path();
    let a = note(root, "x/a.md", "");
    let b = note(root, "y/a.md", "");

    let mut hits = match index_of(root).resolve_wikilink("a") {
        Resolution::Ambiguous(hits) => hits,
        other => panic!("应为歧义: {other:?}"),
    };
    hits.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(hits, expected);
}

// ── 增量更新 ──────────────────────────────────────────────────

#[test]
fn reindex_after_save_updates_backlinks_and_tags() {
    let dir = tmp();
    let root = dir.path();
    let target = note(root, "target.md", "# 目标\n");
    let source = note(root, "source.md", "草稿\n");

    let mut index = index_of(root);
    assert!(index.backlinks(&target).is_empty());
    assert!(index.tags().is_empty());

    // 编辑并保存：内容与事实都变了
    fs::write(&source, "见 [[target]] #想法\n").unwrap();
    index.reindex_file(&source);

    assert_eq!(index.backlinks(&target).len(), 1, "保存后反链应出现");
    assert_eq!(index.tags(), vec![("想法".to_string(), 1)]);
}

#[test]
fn reindex_is_idempotent() {
    let dir = tmp();
    let root = dir.path();
    let only = note(root, "only.md", "[[x]] #a\n");

    let mut index = index_of(root);
    let before = index.len();
    for _ in 0..3 {
        index.reindex_file(&only);
    }
    assert_eq!(index.len(), before, "重复重解析不应产生重复条目");
    assert_eq!(index.tags(), vec![("a".to_string(), 1)]);
}

#[test]
fn reindex_ignores_data_files_and_missing_paths() {
    let dir = tmp();
    let root = dir.path();
    let data = note(root, "conf.json", "{}");

    let mut index = index_of(root);
    index.reindex_file(&data);
    assert!(index.is_empty(), "数据文件不进索引");

    index.reindex_file(&root.join("不存在.md"));
    assert!(index.is_empty(), "读不到的文件不进索引");
}
