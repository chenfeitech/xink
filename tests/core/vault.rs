use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

#[test]
fn should_ignore_common_noise() {
    assert!(should_ignore(".git", false));
    assert!(should_ignore("node_modules", false));
    assert!(should_ignore(".DS_Store", false));
    assert!(should_ignore(".hidden", false));
    assert!(should_ignore("draft.md.tmp", false));
    assert!(!should_ignore("note.md", false));
    assert!(!should_ignore("folder", false));
}

#[test]
fn scan_collects_image_files() {
    // 目录树要能看到图片：jpg / png / gif / svg / webp 都应被收集，
    // 其余非笔记文件仍然不进树（避免把库里的杂文件全列出来）
    let dir = tmp();
    let root = dir.path();
    for name in ["shot.png", "photo.jpg", "anim.gif", "icon.svg", "pic.webp"] {
        fs::write(root.join(name), "").unwrap();
    }
    fs::write(root.join("note.md"), "").unwrap();
    fs::write(root.join("readme.txt"), "").unwrap();
    fs::write(root.join("main.rs"), "").unwrap();

    let vault = Vault::open(root.to_path_buf()).unwrap();
    let tree = vault.scan(false).unwrap();

    let names: Vec<String> = tree.children.iter().map(|c| c.name.clone()).collect();
    for name in ["shot.png", "photo.jpg", "anim.gif", "icon.svg", "pic.webp"] {
        assert!(
            names.iter().any(|n| n == name),
            "图片应出现在目录树: {name}"
        );
    }
    assert!(names.iter().any(|n| n == "note.md"));
    assert!(
        !names.iter().any(|n| n == "readme.txt"),
        "非图片非笔记不进树"
    );
    assert!(!names.iter().any(|n| n == "main.rs"), "非图片非笔记不进树");
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
    let tree = vault.scan(false).unwrap();

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

// ── 多库：条目与路径校验 ──────────────────────────────────────

#[test]
fn entry_name_falls_back_to_dir_name() {
    let dir = tmp();
    let lib = dir.path().join("我的笔记");
    fs::create_dir_all(&lib).unwrap();

    // 显式命名
    let named = VaultEntry::new(lib.clone(), "工作库");
    assert_eq!(named.name, "工作库");
    // 空名称回落到目录名
    let auto = VaultEntry::new(lib.clone(), "   ");
    assert_eq!(auto.name, "我的笔记");
    assert_eq!(auto.name, VaultEntry::default_name(&lib));
}

#[test]
fn entry_ids_are_stable_per_instance_and_differ_between_paths() {
    let dir = tmp();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();

    let ea = VaultEntry::new(a.clone(), "a");
    let eb = VaultEntry::new(b.clone(), "b");
    assert_ne!(ea.id, eb.id, "不同路径的 id 应不同");

    // 同一条目的 id 不随克隆改变（路径被"重新定位"时由调用方保留 id）
    let cloned = ea.clone();
    assert_eq!(cloned.id, ea.id);
}

#[test]
fn status_ok_for_existing_dir() {
    let dir = tmp();
    let entry = VaultEntry::new(lib(dir.path()), "lib");
    assert_eq!(entry.status(&[]), VaultStatus::Ok);
}

#[test]
fn status_missing_when_path_gone() {
    let dir = tmp();
    let entry = VaultEntry::new(dir.path().join("不存在"), "lib");
    assert_eq!(entry.status(&[]), VaultStatus::Missing);
    assert!(!entry.status(&[]).is_usable());
}

#[test]
fn status_not_dir_when_path_is_file() {
    let dir = tmp();
    let file = dir.path().join("note.md");
    fs::write(&file, "").unwrap();
    let entry = VaultEntry::new(file, "lib");
    assert_eq!(entry.status(&[]), VaultStatus::NotDir);
}

#[test]
fn status_duplicate_when_same_path_registered_twice() {
    let dir = tmp();
    let lib = lib(dir.path());
    let a = VaultEntry::new(lib.clone(), "第一个");
    let b = VaultEntry::new(lib, "第二个（重复）");
    // 重复是对称判定：两条都会被标成 Duplicate，
    // "保留先注册的那条"是上层的策略，不在状态判定里
    assert_eq!(b.status(&[a.clone()]), VaultStatus::Duplicate);
    assert_eq!(a.status(&[b]), VaultStatus::Duplicate);
}

#[test]
fn status_nested_when_inside_another_vault() {
    let dir = tmp();
    let outer = lib(dir.path());
    let inner = outer.join("子库");
    fs::create_dir_all(&inner).unwrap();

    let o = VaultEntry::new(outer, "外层");
    let i = VaultEntry::new(inner, "内层");
    assert_eq!(i.status(&[o.clone()]), VaultStatus::Nested);
    // 嵌套只是警告，仍然可用
    assert!(i.status(&[o]).is_usable());
    assert_eq!(
        VaultStatus::Nested.message(),
        "被其它库包含，笔记会被重复计入"
    );
}

/// 建一个真实存在的目录作为库根
fn lib(dir: &Path) -> PathBuf {
    let p = dir.join("lib");
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn resolve_active_keeps_choice_when_usable() {
    let dir = tmp();
    let entry = VaultEntry::new(lib(dir.path()), "lib");
    let mut reg = VaultRegistry {
        vaults: vec![entry.clone()],
        active: Some(entry.id.clone()),
    };
    let notes = reg.resolve_active();
    assert!(notes.is_empty(), "可用时不应产生提示: {notes:?}");
    assert_eq!(reg.active.as_deref(), Some(entry.id.as_str()));
}

#[test]
fn resolve_active_falls_back_to_first_usable_vault() {
    let dir = tmp();
    let ok = VaultEntry::new(lib(dir.path()), "可用库");
    let dead = VaultEntry::new(dir.path().join("已删除"), "失效库");
    let mut reg = VaultRegistry {
        vaults: vec![dead.clone(), ok.clone()],
        active: Some(dead.id.clone()),
    };

    let notes = reg.resolve_active();
    assert_eq!(
        reg.active.as_deref(),
        Some(ok.id.as_str()),
        "应回落到可用库"
    );
    assert!(
        notes.iter().any(|n| n.contains("失效库")),
        "应说明失效原因: {notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("已切换到")),
        "应说明已切换: {notes:?}"
    );
}

#[test]
fn resolve_active_clears_choice_when_nothing_usable() {
    let dir = tmp();
    let dead = VaultEntry::new(dir.path().join("已删除"), "失效库");
    let mut reg = VaultRegistry {
        vaults: vec![dead],
        active: Some("任意 id".to_string()),
    };

    let notes = reg.resolve_active();
    assert!(reg.active.is_none(), "全部失效时不应选中任何库");
    assert!(reg.active_root().is_none());
    assert!(
        notes.iter().any(|n| n.contains("所有库路径都不可用")),
        "应提示用户: {notes:?}"
    );
}

#[test]
fn empty_registry_resolves_without_notes() {
    let mut reg = VaultRegistry::default();
    let notes = reg.resolve_active();
    assert!(notes.is_empty(), "空注册表不该产生噪音: {notes:?}");
    assert!(reg.active.is_none());
}

#[test]
fn should_ignore_keeps_noise_dirs_always() {
    // 噪音目录无论开关如何都跳过
    for name in [".git", "node_modules", "target", ".DS_Store"] {
        assert!(should_ignore(name, false), "{name} 应始终跳过");
        assert!(should_ignore(name, true), "{name} 应始终跳过");
    }
}

#[test]
fn should_ignore_hides_dot_files_only_when_disabled() {
    assert!(should_ignore(".hidden", false), "关闭时应跳过点开头");
    assert!(!should_ignore(".hidden", true), "开启时应显示点开头");
    assert!(!should_ignore(".obsidian", true));
    assert!(!should_ignore(".gitignore", true));
}

#[test]
fn should_ignore_always_skips_temp_files() {
    assert!(should_ignore("draft.md.tmp", true));
    assert!(should_ignore("draft.md.tmp", false));
}

#[test]
fn scan_can_include_hidden_entries() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join(".gitignore"), b"x").unwrap();
    std::fs::write(root.join("note.md"), b"x").unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();

    let vault = Vault::open(root.to_path_buf()).unwrap();
    let shown = vault.scan(true).unwrap();
    let hidden = vault.scan(false).unwrap();

    let names =
        |n: &FileNode| -> Vec<String> { n.children.iter().map(|c| c.name.clone()).collect() };
    assert!(
        names(&shown).contains(&".gitignore".to_string()),
        "开启时可见"
    );
    assert!(
        !names(&hidden).contains(&".gitignore".to_string()),
        "关闭时不可见"
    );
    // .git 始终不出现
    assert!(!names(&shown).contains(&".git".to_string()));
}

// ── 重命名：后缀与重名策略 ────────────────────────────────────

#[test]
fn rename_non_markdown_keeps_its_extension() {
    let dir = tmp();
    let root = dir.path();
    let vault = Vault::open(root.to_path_buf()).unwrap();

    let path = root.join("conf.json");
    fs::write(&path, "{}").unwrap();
    let renamed = vault.rename(&path, "conf2").unwrap();
    assert_eq!(renamed.file_name().unwrap(), "conf2.json");
    assert!(!renamed.to_string_lossy().ends_with(".md"));
}

#[test]
fn rename_non_markdown_on_conflict_adds_index_not_md() {
    let dir = tmp();
    let root = dir.path();
    let vault = Vault::open(root.to_path_buf()).unwrap();

    let source = root.join("a.json");
    fs::write(&source, "{}").unwrap();
    fs::write(root.join("conf.json"), "{}").unwrap();

    let renamed = vault.rename(&source, "conf").unwrap();
    assert_eq!(
        renamed.file_name().unwrap(),
        "conf-1.json",
        "重名应保留扩展名并递增，而不是拼 .md 绕过冲突"
    );
    assert!(renamed.exists());
    assert!(!root.join("conf.json.md").exists(), "绝不能产生 .json.md");
}

#[test]
fn rename_markdown_on_conflict_adds_index() {
    let dir = tmp();
    let root = dir.path();
    let vault = Vault::open(root.to_path_buf()).unwrap();

    let note = vault.create_note(root, "a").unwrap();
    vault.create_note(root, "note").unwrap();

    let renamed = vault.rename(&note, "note").unwrap();
    assert_eq!(renamed.file_name().unwrap(), "note-1.md");
}

#[test]
fn rename_of_a_file_without_extension_adds_nothing() {
    let dir = tmp();
    let root = dir.path();
    let vault = Vault::open(root.to_path_buf()).unwrap();

    let path = root.join("README");
    fs::write(&path, "x").unwrap();
    let renamed = vault.rename(&path, "README2").unwrap();
    assert_eq!(renamed.file_name().unwrap(), "README2");
}
