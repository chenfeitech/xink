//! `state::workspace` 的持久化测试
//!
//! 库注册表提供了"带路径参数"的内部版本（`load_vaults_at`），
//! 因此测试可以在临时目录里跑，不会碰到真实的应用数据目录。

use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

/// 造一个真实存在的库目录
fn lib_dir(dir: &Path) -> PathBuf {
    let lib = dir.join("我的笔记");
    fs::create_dir_all(&lib).unwrap();
    lib
}

#[test]
fn registry_roundtrip() {
    let dir = tmp();
    let lib = lib_dir(dir.path());
    let vaults_file = dir.path().join("vaults.json");
    let legacy = dir.path().join("vault.json");

    let entry = VaultEntry::new(lib, "我的笔记");
    let reg = VaultRegistry {
        vaults: vec![entry.clone()],
        active: Some(entry.id.clone()),
    };

    save_vaults_at(&vaults_file, &reg).unwrap();
    let loaded = load_vaults_at(&vaults_file, &legacy);

    assert_eq!(loaded, reg);
    assert_eq!(
        loaded.active_entry().map(|e| e.name.as_str()),
        Some("我的笔记"),
        "应能取回当前库"
    );
}

#[test]
fn missing_file_yields_empty_registry() {
    let dir = tmp();
    let loaded = load_vaults_at(
        &dir.path().join("vaults.json"),
        &dir.path().join("vault.json"),
    );
    assert_eq!(loaded, VaultRegistry::default(), "文件缺失应得到空注册表");
    assert!(loaded.active_entry().is_none());
}

#[test]
fn broken_json_yields_empty_registry() {
    // 宽容策略：损坏一律回落默认值，绝不阻塞启动
    let dir = tmp();
    let vaults_file = dir.path().join("vaults.json");
    fs::write(&vaults_file, "{ 这不是 JSON").unwrap();

    let loaded = load_vaults_at(&vaults_file, &dir.path().join("vault.json"));
    assert_eq!(loaded, VaultRegistry::default());
}

#[test]
fn unknown_fields_are_ignored() {
    let dir = tmp();
    let vaults_file = dir.path().join("vaults.json");
    fs::write(
        &vaults_file,
        r#"{"vaults": [], "active": null, "something_new": 42}"#,
    )
    .unwrap();

    let loaded = load_vaults_at(&vaults_file, &dir.path().join("vault.json"));
    assert_eq!(loaded, VaultRegistry::default(), "未知字段应被忽略");
}

#[test]
fn legacy_vault_is_migrated_once() {
    // 旧版本只写了单个 root，升级后应变成一条库记录并写回 vaults.json
    let dir = tmp();
    let lib = lib_dir(dir.path());
    let vaults_file = dir.path().join("vaults.json");
    let legacy = dir.path().join("vault.json");
    write_json(&legacy, &LastVault { root: lib.clone() }).unwrap();

    let loaded = load_vaults_at(&vaults_file, &legacy);
    assert_eq!(loaded.vaults.len(), 1, "应迁移出一条库");
    assert_eq!(loaded.vaults[0].path, lib);
    assert_eq!(loaded.vaults[0].name, "我的笔记", "名称默认取目录名");
    assert!(loaded.active_entry().is_some(), "迁移后应选中该库");
    assert!(vaults_file.exists(), "迁移结果应写回，避免每次启动都迁移");
}

#[test]
fn legacy_vault_with_dead_path_is_ignored() {
    let dir = tmp();
    let legacy = dir.path().join("vault.json");
    write_json(
        &legacy,
        &LastVault {
            root: dir.path().join("早已删除的目录"),
        },
    )
    .unwrap();

    let loaded = load_vaults_at(&dir.path().join("vaults.json"), &legacy);
    assert_eq!(loaded, VaultRegistry::default(), "目录失效则不做迁移");
}

#[test]
fn active_pointing_to_removed_vault_is_treated_as_none() {
    // active 指向已被移除的库时，`active_entry()` 必须返回 None，
    // 调用方据此回落到"第一个可用库"
    let dir = tmp();
    let lib = lib_dir(dir.path());
    let entry = VaultEntry::new(lib, "我的笔记");
    let reg = VaultRegistry {
        vaults: vec![entry],
        active: Some("已经不存在的 id".to_string()),
    };
    assert!(reg.active_entry().is_none());
}

/// 测试专用的保存入口（与 `save_vaults` 等价，只是路径可注入）
fn save_vaults_at(path: &Path, reg: &VaultRegistry) -> Result<()> {
    write_json_atomic(path, reg)
}

// ── 打开中的笔记 ──────────────────────────────────────────────
//
// `save_open_notes` / `load_open_notes` 读写真实的应用数据目录，测试不能碰，
// 因此把"保序去重 + active 必须落在 paths 内"抽成了纯函数在这里覆盖。

fn path(name: &str) -> PathBuf {
    PathBuf::from("/vault").join(name)
}

#[test]
fn open_notes_are_deduped_in_order() {
    let notes = normalize_open_notes(&[path("a.md"), path("b.md"), path("a.md")], None);
    assert_eq!(notes.paths, vec![path("a.md"), path("b.md")]);
}

#[test]
fn open_notes_active_must_be_one_of_the_paths() {
    // active 不在列表里就当没有：否则启动会打开一篇用户根本没开过的笔记
    let notes = normalize_open_notes(&[path("a.md")], Some(&path("gone.md")));
    assert!(notes.active.is_none());

    let notes = normalize_open_notes(&[path("a.md"), path("b.md")], Some(&path("b.md")));
    assert_eq!(notes.active, Some(path("b.md")));
}

#[test]
fn open_notes_with_nothing_open_have_no_active() {
    // 去重后列表为空时，active 也不能留着
    let notes = normalize_open_notes(&[], Some(&path("a.md")));
    assert!(notes.paths.is_empty());
    assert!(notes.active.is_none());
}

#[test]
fn active_within_keeps_only_known_paths() {
    let paths = vec![path("a.md")];
    assert_eq!(
        active_within(Some(path("a.md")), &paths),
        Some(path("a.md"))
    );
    assert_eq!(active_within(Some(path("b.md")), &paths), None);
    assert_eq!(active_within(None, &paths), None);
}
