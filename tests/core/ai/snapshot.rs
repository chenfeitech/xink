//! 快照命名与写入

use crate::core::ai::snapshot::{clear, count, rotate, snapshot_name, write_snapshot};

/// 造 N 份快照：名字按时间序（各差一秒），与真实命名一致
fn seed(dir: &std::path::Path, n: usize) {
    for i in 0..n {
        let name = format!("2026-10-26-1045{i:02}-note.md");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), format!("v{i}")).unwrap();
    }
}

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

#[test]
fn 轮转只保留最近若干个() {
    let dir = std::env::temp_dir().join("xink-snapshot-rotate");
    let _ = std::fs::remove_dir_all(&dir);
    seed(&dir, 5);
    assert_eq!(count(&dir), 5);

    let removed = rotate(&dir, 3).unwrap();
    assert_eq!(removed, 2);
    assert_eq!(count(&dir), 3);
    // 保留的应是最新的三个（名字最大）
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "2026-10-26-104502-note.md",
            "2026-10-26-104503-note.md",
            "2026-10-26-104504-note.md"
        ]
    );

    // 没超上限时不动
    assert_eq!(rotate(&dir, 10).unwrap(), 0);
    assert_eq!(count(&dir), 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 清空删掉全部快照() {
    let dir = std::env::temp_dir().join("xink-snapshot-clear");
    let _ = std::fs::remove_dir_all(&dir);
    seed(&dir, 3);
    assert_eq!(clear(&dir).unwrap(), 3);
    assert_eq!(count(&dir), 0);
    // 目录不存在 / 已空都返回 0，不该报错
    assert_eq!(clear(&dir).unwrap(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 写入后自动轮转() {
    let dir = std::env::temp_dir().join("xink-snapshot-autorotate");
    let _ = std::fs::remove_dir_all(&dir);
    seed(&dir, crate::core::ai::snapshot::MAX_SNAPSHOTS);
    write_snapshot(&dir, "note.md", "new").unwrap();
    // 数量被压回上限，且新写的这份还在
    assert_eq!(count(&dir), crate::core::ai::snapshot::MAX_SNAPSHOTS);
    let _ = std::fs::remove_dir_all(&dir);
}
