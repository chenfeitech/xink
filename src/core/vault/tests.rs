use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

#[test]
fn should_ignore_common_noise() {
    assert!(should_ignore(".git"));
    assert!(should_ignore("node_modules"));
    assert!(should_ignore(".DS_Store"));
    assert!(should_ignore(".hidden"));
    assert!(should_ignore("draft.md.tmp"));
    assert!(!should_ignore("note.md"));
    assert!(!should_ignore("folder"));
}

#[test]
fn scan_collects_markdown_and_dirs_sorted() {
    let dir = tmp();
    let root = dir.path();
    fs::write(root.join("b.md"), "").unwrap();
    fs::write(root.join("a.md"), "").unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub").join("c.md"), "").unwrap();
    // 噪声应被忽略
    fs::write(root.join("ignored.txt"), "").unwrap();
    fs::create_dir(root.join(".git")).unwrap();

    let vault = Vault::open(root.to_path_buf()).unwrap();
    let tree = vault.scan().unwrap();

    let names: Vec<&str> = tree.children.iter().map(|n| n.name.as_str()).collect();
    // 目录排在前，文件按名称排序
    assert_eq!(names, vec!["sub", "a.md", "b.md"]);
    // 递归进入子目录
    assert_eq!(tree.children[0].children.len(), 1);
}

#[test]
fn create_note_appends_md_suffix() {
    let dir = tmp();
    let vault = Vault::open(dir.path().to_path_buf()).unwrap();
    let path = vault.create_note(dir.path(), "hello").unwrap();
    assert_eq!(path.file_name().unwrap(), "hello.md");
    assert!(path.exists());
}

#[test]
fn create_note_rejects_duplicate() {
    let dir = tmp();
    let vault = Vault::open(dir.path().to_path_buf()).unwrap();
    vault.create_note(dir.path(), "note").unwrap();
    assert!(vault.create_note(dir.path(), "note").is_err());
}

#[test]
fn remove_deletes_file_and_non_empty_dir() {
    let dir = tmp();
    let root = dir.path();
    let vault = Vault::open(root.to_path_buf()).unwrap();

    let note = vault.create_note(root, "a").unwrap();
    vault.remove(&note).unwrap();
    assert!(!note.exists());

    let folder = vault.create_dir(root, "folder").unwrap();
    vault.create_note(&folder, "inner").unwrap();
    vault.remove(&folder).unwrap();
    assert!(!folder.exists());
}

#[test]
fn remove_rejects_vault_root() {
    let dir = tmp();
    let vault = Vault::open(dir.path().to_path_buf()).unwrap();
    assert!(vault.remove(vault.root()).is_err());
}

#[test]
fn rename_and_move_work() {
    let dir = tmp();
    let root = dir.path();
    let vault = Vault::open(root.to_path_buf()).unwrap();

    let note = vault.create_note(root, "old").unwrap();
    let renamed = vault.rename(&note, "new").unwrap();
    assert_eq!(renamed.file_name().unwrap(), "new.md");
    assert!(!note.exists());

    let target = vault.create_dir(root, "target").unwrap();
    let moved = vault.move_to(&renamed, &target).unwrap();
    assert_eq!(moved.parent().unwrap(), target);
    assert!(moved.exists());
}
