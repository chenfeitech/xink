//! 快照命名与写入

use crate::core::ai::snapshot::{snapshot_name, write_snapshot};

#[test]
fn 名字带时间戳与原文件名() {
    let name = snapshot_name("note.md");
    assert!(name.ends_with("-note.md"));
    assert_eq!(name.len(), "2026-10-26-104500-note.md".len());
}

#[test]
fn 写入内容等于原文() {
    let dir = std::env::temp_dir().join("xink-snapshot-test");
    let _ = std::fs::remove_dir_all(&dir);
    let path = write_snapshot(&dir, "note.md", "hello").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    // 同一秒内再次快照不应互相覆盖
    let path2 = write_snapshot(&dir, "note.md", "world").unwrap();
    assert_ne!(path, path2);
    let _ = std::fs::remove_dir_all(&dir);
}
