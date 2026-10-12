use super::*;
use std::fs;
use tempfile::TempDir;

fn touch(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"x").expect("写入测试文件");
    path
}

#[test]
fn unique_name_keeps_name_when_free() {
    let dir = TempDir::new().unwrap();
    assert_eq!(unique_name(dir.path(), "a.png"), dir.path().join("a.png"));
}

#[test]
fn unique_name_appends_index_on_conflict() {
    let dir = TempDir::new().unwrap();
    touch(dir.path(), "a.png");
    assert_eq!(unique_name(dir.path(), "a.png"), dir.path().join("a-1.png"));
}

#[test]
fn unique_name_increments_without_stacking() {
    let dir = TempDir::new().unwrap();
    touch(dir.path(), "a.png");
    touch(dir.path(), "a-1.png");
    // 不应产生 a-1-1.png
    assert_eq!(unique_name(dir.path(), "a.png"), dir.path().join("a-2.png"));
}

#[test]
fn unique_name_keeps_multi_dot_extension() {
    let dir = TempDir::new().unwrap();
    touch(dir.path(), "a.b.png");
    assert_eq!(
        unique_name(dir.path(), "a.b.png"),
        dir.path().join("a.b-1.png")
    );
}

#[test]
fn unique_name_handles_no_extension() {
    let dir = TempDir::new().unwrap();
    touch(dir.path(), "笔记");
    assert_eq!(unique_name(dir.path(), "笔记"), dir.path().join("笔记-1"));
}

#[test]
fn unique_name_applies_to_directories() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("assets")).unwrap();
    assert_eq!(
        unique_name(dir.path(), "assets"),
        dir.path().join("assets-1")
    );
}

#[test]
fn import_paths_copy_keeps_source() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();
    let file = touch(src.path(), "a.png");

    let results = import_paths(&[file.clone()], dst.path(), ImportMode::Copy);
    assert!(results[0].is_ok(), "复制应成功: {:?}", results[0]);
    assert!(file.exists(), "复制模式必须保留源文件");
    assert!(dst.path().join("a.png").exists());
}

#[test]
fn import_paths_move_removes_source() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();
    let file = touch(src.path(), "b.md");

    let results = import_paths(&[file.clone()], dst.path(), ImportMode::Move);
    assert!(results[0].is_ok(), "移动应成功: {:?}", results[0]);
    assert!(!file.exists(), "移动模式应移除源文件");
    assert!(dst.path().join("b.md").exists());
}

#[test]
fn import_paths_renames_on_conflict() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();
    let file = touch(src.path(), "a.png");
    touch(dst.path(), "a.png");

    let results = import_paths(&[file], dst.path(), ImportMode::Copy);
    assert!(results[0].is_ok());
    assert!(dst.path().join("a-1.png").exists(), "冲突时应改名导入");
}

#[test]
fn move_path_rejects_existing_target_and_keeps_source() {
    let dir = TempDir::new().unwrap();
    let from = touch(dir.path(), "x.md");
    let to = touch(dir.path(), "y.md");

    assert!(move_path(&from, &to).is_err(), "目标已存在应失败");
    assert!(from.exists(), "失败时源文件必须保留");
}

#[test]
fn copy_path_copies_directory_recursively() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();
    fs::create_dir(src.path().join("sub")).unwrap();
    touch(&src.path().join("sub"), "inner.md");

    let target = dst.path().join("sub");
    assert!(copy_path(&src.path().join("sub"), &target).is_ok());
    assert!(target.join("inner.md").exists(), "目录应递归复制");
}

#[test]
fn write_new_file_writes_bytes_and_avoids_conflict() {
    let dir = TempDir::new().unwrap();

    let first = write_new_file(dir.path(), "pasted", "png", b"aaa").unwrap();
    assert_eq!(first, dir.path().join("pasted.png"));
    assert_eq!(fs::read(&first).unwrap(), b"aaa");

    // 同名再写一次应递增，而不是覆盖
    let second = write_new_file(dir.path(), "pasted", "png", b"bbb").unwrap();
    assert_eq!(second, dir.path().join("pasted-1.png"));
    assert_eq!(fs::read(&second).unwrap(), b"bbb");
    assert_eq!(fs::read(&first).unwrap(), b"aaa", "原文件不应被覆盖");
}

#[test]
fn write_new_file_handles_empty_extension() {
    let dir = TempDir::new().unwrap();
    let path = write_new_file(dir.path(), "note", "", b"x").unwrap();
    assert_eq!(path, dir.path().join("note"));
}
