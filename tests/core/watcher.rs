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

// ── 事件种类映射 ──────────────────────────────────────────────
//
// 这个映射决定"哪些改动会触发重扫"：认错会让外部改名不刷新（Renamed），
// 或让自动保存期间持续重扫（Modified）。设计见
// docs/file-tree-incremental-refresh-design.md §3.4。

use notify::EventKind;
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};

#[test]
fn rename_events_are_classified_as_renamed() {
    for mode in [
        RenameMode::Both,
        RenameMode::From,
        RenameMode::To,
        RenameMode::Any,
    ] {
        let kind = EventKind::Modify(ModifyKind::Name(mode));
        assert_eq!(
            classify_kind(&kind),
            Some(WatchKind::Renamed),
            "{mode:?} 应判为重命名"
        );
    }
}

#[test]
fn content_and_metadata_changes_stay_modified() {
    // 自动保存走 Data：归入 Modified 才不会在编辑时反复重扫整棵树
    assert_eq!(
        classify_kind(&EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Any
        ))),
        Some(WatchKind::Modified)
    );
    assert_eq!(
        classify_kind(&EventKind::Modify(ModifyKind::Metadata(
            notify::event::MetadataKind::Any
        ))),
        Some(WatchKind::Modified)
    );
    assert_eq!(
        classify_kind(&EventKind::Modify(ModifyKind::Any)),
        Some(WatchKind::Modified)
    );
}

#[test]
fn create_and_remove_are_unchanged() {
    assert_eq!(
        classify_kind(&EventKind::Create(CreateKind::Any)),
        Some(WatchKind::Created)
    );
    assert_eq!(
        classify_kind(&EventKind::Remove(RemoveKind::Any)),
        Some(WatchKind::Removed)
    );
}

#[test]
fn irrelevant_events_are_dropped() {
    assert_eq!(
        classify_kind(&EventKind::Access(notify::event::AccessKind::Any)),
        None
    );
    assert_eq!(classify_kind(&EventKind::Other), None);
}
