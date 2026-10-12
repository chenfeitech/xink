use super::*;

#[test]
fn detects_editable_and_temp_paths() {
    assert!(is_visible_file(Path::new("a.md")));
    assert!(is_visible_file(Path::new("a.json")));
    assert!(is_visible_file(Path::new("a.yaml")));
    assert!(!is_visible_file(Path::new("a.txt")));
    assert!(is_temp(Path::new("a.md.tmp")));
    assert!(!is_temp(Path::new("a.md")));
}

#[test]
fn watcher_starts_and_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let watcher = VaultWatcher::new(dir.path()).expect("监听应能启动");
    // 刚启动时不应有任何变更
    assert!(watcher.poll().is_empty());
}
