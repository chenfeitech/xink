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

    let results = import_paths(std::slice::from_ref(&file), dst.path(), ImportMode::Copy);
    assert!(results[0].is_ok(), "复制应成功: {:?}", results[0]);
    assert!(file.exists(), "复制模式必须保留源文件");
    assert!(dst.path().join("a.png").exists());
}

#[test]
fn import_paths_move_removes_source() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();
    let file = touch(src.path(), "b.md");

    let results = import_paths(std::slice::from_ref(&file), dst.path(), ImportMode::Move);
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

// ── 重命名时的文件名 ──────────────────────────────────────────
//
// 回归点：非 Markdown 文件改名 / 重名时曾被拼上 `.md`（`conf.json` →
// `conf.json.md`），文件类型与内容从此对不上。

fn name(from: &str, new_name: &str) -> String {
    rename_name(Path::new(from), new_name)
}

#[test]
fn markdown_rename_appends_md_when_extension_is_missing() {
    assert_eq!(name("/vault/note.md", "new"), "new.md");
    assert_eq!(name("/vault/note.md", "new.md"), "new.md");
    assert_eq!(name("/vault/NOTE.MD", "new"), "new.MD", "沿用源文件大小写");
}

#[test]
fn non_markdown_rename_never_appends_md() {
    // 核心回归：这些都不该出现 `.md`
    assert_eq!(name("/vault/conf.json", "conf"), "conf.json");
    assert_eq!(name("/vault/conf.json", "conf.json"), "conf.json");
    assert_eq!(name("/vault/app.yaml", "app"), "app.yaml");
    assert_eq!(name("/vault/app.yml", "app"), "app.yml");
    assert_eq!(name("/vault/cfg.toml", "cfg"), "cfg.toml");
    assert_eq!(name("/vault/logo.png", "logo"), "logo.png");
    for result in [
        name("/vault/conf.json", "conf"),
        name("/vault/app.yaml", "app"),
    ] {
        assert!(!result.ends_with(".md"), "{result} 不应带 .md");
    }
}

#[test]
fn explicit_extension_is_respected() {
    // 用户显式改类型是刻意为之：尊重，不追加、不改写
    assert_eq!(name("/vault/conf.json", "conf.yml"), "conf.yml");
    assert_eq!(name("/vault/note.md", "note.txt"), "note.txt");
}

#[test]
fn extensionless_source_stays_extensionless() {
    assert_eq!(name("/vault/README", "README2"), "README2");
    assert_eq!(name("/vault/dir", "assets"), "assets", "目录改名不加后缀");
}

#[test]
fn blank_input_is_passed_through() {
    assert_eq!(name("/vault/a.md", "  "), "", "空输入由调用方拦截");
}

// ── 重名：保留扩展名 + 递增后缀 ───────────────────────────────

#[test]
fn duplicate_rename_keeps_extension_and_adds_index() {
    let dir = TempDir::new().unwrap();
    let from = touch(dir.path(), "a.json");
    touch(dir.path(), "conf.json");

    let target = unique_name(dir.path(), &rename_name(&from, "conf"));
    assert_eq!(target.file_name().unwrap(), "conf-1.json");
    assert!(!target.exists());
    assert!(!target.to_string_lossy().ends_with(".md"));
}

#[test]
fn duplicate_markdown_rename_adds_index_before_extension() {
    let dir = TempDir::new().unwrap();
    let from = touch(dir.path(), "a.md");
    touch(dir.path(), "note.md");
    touch(dir.path(), "note-1.md");

    let target = unique_name(dir.path(), &rename_name(&from, "note"));
    assert_eq!(
        target.file_name().unwrap(),
        "note-2.md",
        "不叠加成 note-1-1.md"
    );
}

// ── 删除 ──────────────────────────────────────────────────────
//
// `remove_path` 是唯一的破坏性入口（拖入导入失败回滚、库内删除都走它），
// 删错了不可恢复，因此把"能删什么、删不动时报什么"钉住。

#[test]
fn remove_path_deletes_a_file_and_a_directory_tree() {
    let dir = TempDir::new().unwrap();
    let file = touch(dir.path(), "a.md");
    remove_path(&file).expect("删文件");
    assert!(!file.exists());

    let nested = dir.path().join("tree");
    fs::create_dir_all(nested.join("sub")).unwrap();
    touch(&nested.join("sub"), "b.md");
    remove_path(&nested).expect("删目录");
    assert!(!nested.exists(), "目录要递归删干净");
}

#[test]
fn remove_path_reports_missing_paths_instead_of_panicking() {
    let dir = TempDir::new().unwrap();
    let gone = dir.path().join("不存在.md");
    let err = remove_path(&gone).expect_err("删不存在的路径应报错");
    assert!(err.contains("删除文件失败"), "{err}");
}

#[test]
fn remove_path_refuses_to_touch_the_vault_root() {
    // `Vault::remove` 在调用它之前就挡住了库根：这里是最后一道护栏，
    // 直接删用户目录会把整个库删掉
    let dir = TempDir::new().unwrap();
    let vault = crate::core::vault::Vault::open(dir.path().to_path_buf()).expect("打开库");
    assert!(
        vault.remove(dir.path()).is_err(),
        "删库根必须被拒绝，否则整库消失"
    );
    assert!(dir.path().exists(), "被拒绝时目录必须还在");
}
