use super::*;
use std::fs;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

/// 临时目录在 macOS 上位于 /var（符号链接到 /private/var），
/// 统一取 canonical 形式，避免断言受运行环境影响。
fn canonical(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().canonicalize().expect("canonicalize 临时目录")
}

// ── tab_key ──────────────────────────────────────────────

#[test]
fn tab_key_normalizes_dot_and_double_slash() {
    let dir = tmp();
    let root = canonical(&dir);
    fs::write(root.join("a.md"), "x").unwrap();

    let plain = root.join("a.md");
    let dotted = root.join(".").join("a.md");
    let doubled = root.join("").join("a.md");

    assert_eq!(tab_key(&plain), tab_key(&dotted), "./ 应被规范化掉");
    assert_eq!(tab_key(&plain), tab_key(&doubled), "重复斜杠应被规范化掉");
}

#[test]
fn tab_key_falls_back_to_original_path_when_file_is_gone() {
    let dir = tmp();
    let path = canonical(&dir).join("missing.md");

    // 文件不存在时 canonicalize 失败，必须仍返回一个可用的键
    assert_eq!(tab_key(&path), path);
}

#[test]
fn tab_key_distinguishes_same_name_in_different_dirs() {
    let dir = tmp();
    let root = canonical(&dir);
    fs::create_dir(root.join("a")).unwrap();
    fs::create_dir(root.join("b")).unwrap();
    fs::write(root.join("a").join("note.md"), "").unwrap();
    fs::write(root.join("b").join("note.md"), "").unwrap();

    assert_ne!(
        tab_key(&root.join("a").join("note.md")),
        tab_key(&root.join("b").join("note.md")),
        "同名不同目录必须是两个标签"
    );
}

#[cfg(unix)]
#[test]
fn tab_key_resolves_symlink_to_real_file() {
    use std::os::unix::fs::symlink;

    let dir = tmp();
    let root = canonical(&dir);
    let real = root.join("real.md");
    fs::write(&real, "x").unwrap();
    let link = root.join("link.md");
    symlink(&real, &link).unwrap();

    assert_eq!(
        tab_key(&link),
        tab_key(&real),
        "软链接与其目标必须判为同一标签，否则会开出两个编辑同一文件的标签"
    );
}

// ── dedup_paths ──────────────────────────────────────────

#[test]
fn dedup_paths_keeps_first_occurrence_and_order() {
    let paths = vec![
        PathBuf::from("/v/b.md"),
        PathBuf::from("/v/a.md"),
        PathBuf::from("/v/b.md"),
        PathBuf::from("/v/c.md"),
        PathBuf::from("/v/a.md"),
    ];

    // 去重依据的是"最终指向同一文件"，此处路径字面量相同即重复
    let deduped = dedup_paths(&paths);
    assert_eq!(
        deduped,
        vec![
            PathBuf::from("/v/b.md"),
            PathBuf::from("/v/a.md"),
            PathBuf::from("/v/c.md"),
        ]
    );
}

#[test]
fn dedup_paths_treats_alias_paths_of_one_file_as_single_tab() {
    let dir = tmp();
    let root = canonical(&dir);
    let real = root.join("a.md");
    fs::write(&real, "x").unwrap();

    let deduped = dedup_paths(&[root.join(".").join("a.md"), real.clone()]);
    assert_eq!(deduped.len(), 1, "同一文件的两种写法只应保留一个标签");
    assert_eq!(tab_key(&deduped[0]), tab_key(&real));
}

#[test]
fn dedup_paths_accepts_empty_input() {
    assert!(dedup_paths(&[]).is_empty());
}

// ── partition_existing ───────────────────────────────────

#[test]
fn partition_existing_separates_deleted_files() {
    let dir = tmp();
    let root = canonical(&dir);
    let alive = root.join("alive.md");
    fs::write(&alive, "x").unwrap();
    let gone = root.join("gone.md");

    let (existing, missing) = partition_existing(vec![alive.clone(), gone.clone()]);
    assert_eq!(existing, vec![alive]);
    assert_eq!(
        missing,
        vec![gone],
        "被删除的笔记必须被剔除，不能恢复成空标签"
    );
}

#[test]
fn partition_existing_treats_directory_as_missing() {
    let dir = tmp();
    let root = canonical(&dir);
    fs::create_dir(root.join("folder.md")).unwrap();

    let (existing, missing) = partition_existing(vec![root.join("folder.md")]);
    assert!(existing.is_empty(), "目录不是笔记，不应被恢复");
    assert_eq!(missing.len(), 1);
}

// ── resolve_active ───────────────────────────────────────

#[test]
fn resolve_active_keeps_active_when_it_survives() {
    let restored = vec![PathBuf::from("/v/a.md"), PathBuf::from("/v/b.md")];

    assert_eq!(
        resolve_active(Some(PathBuf::from("/v/a.md")), &restored),
        Some(PathBuf::from("/v/a.md"))
    );
}

#[test]
fn resolve_active_falls_back_to_last_restored_when_active_is_gone() {
    let restored = vec![PathBuf::from("/v/a.md"), PathBuf::from("/v/b.md")];

    // 活动标签指向的笔记被删了：回落到最后一个，而不是什么都不激活
    assert_eq!(
        resolve_active(Some(PathBuf::from("/v/deleted.md")), &restored),
        Some(PathBuf::from("/v/b.md"))
    );
}

#[test]
fn resolve_active_returns_none_when_nothing_restored() {
    assert_eq!(resolve_active(Some(PathBuf::from("/v/a.md")), &[]), None);
    assert_eq!(resolve_active(None, &[]), None);
}
