use super::*;

fn tmp() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

#[test]
fn load_reads_content_and_clears_dirty() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "# hello").unwrap();

    let note = Note::load(path).unwrap();
    assert_eq!(note.text(), "# hello");
    assert!(!note.is_dirty());
    assert_eq!(note.name(), "a.md");
}

#[test]
fn set_text_marks_dirty_only_on_real_change() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "same").unwrap();

    let mut note = Note::load(path).unwrap();
    note.set_text("same");
    assert!(!note.is_dirty(), "内容未变不应置脏");

    note.set_text("changed");
    assert!(note.is_dirty());
    assert_eq!(note.text(), "changed");
}

#[test]
fn save_writes_disk_and_clears_dirty() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "old").unwrap();

    let mut note = Note::load(path.clone()).unwrap();
    note.set_text("new content");
    note.save().unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
    assert!(!note.is_dirty());
}

#[test]
fn save_leaves_no_temp_file() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "x").unwrap();

    let mut note = Note::load(path.clone()).unwrap();
    note.set_text("y");
    note.save().unwrap();

    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "不应残留临时文件: {leftovers:?}");
}

#[test]
fn save_does_not_report_self_as_external_change() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "x").unwrap();

    let mut note = Note::load(path).unwrap();
    note.set_text("y");
    note.save().unwrap();

    assert!(!note.disk_changed(), "自己的保存不应被判为外部修改");
}

#[test]
fn detects_external_change_and_reload() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "original").unwrap();

    let mut note = Note::load(path.clone()).unwrap();
    assert!(!note.disk_changed());

    // 模拟外部编辑器修改：写入不同长度的内容以确保 mtime 变化
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&path, "modified by another editor").unwrap();

    assert!(note.disk_changed(), "应检测到外部修改");

    note.reload().unwrap();
    assert_eq!(note.text(), "modified by another editor");
    assert!(!note.disk_changed());
}

#[test]
fn is_markdown_checks_extension() {
    assert!(is_markdown(Path::new("a.md")));
    assert!(is_markdown(Path::new("a.MD")));
    assert!(!is_markdown(Path::new("a.txt")));
    assert!(!is_markdown(Path::new("a")));
}

#[test]
fn line_byte_range_covers_each_line_exactly_once() {
    // 滚动同步靠它去问编辑控件"这一行现在画在哪"，区间必须首尾相接且不重叠
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("range.md");
    std::fs::write(&path, "第一行\n第二行\n\n第四行").unwrap();
    let note = Note::load(path.clone()).unwrap();

    let all = (0..4)
        .map(|line| note.line_byte_range(line).expect("每行都应有字节区间"))
        .collect::<Vec<_>>();
    for pair in all.windows(2) {
        assert_eq!(pair[0].1, pair[1].0, "相邻行的区间应首尾相接: {all:?}");
    }
    // 第一行的区间取出来自"第一行"
    let (start, end) = all[0];
    assert_eq!(&note.text()[start..end], "第一行\n");
}

#[test]
fn line_byte_range_out_of_range_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.md");
    std::fs::write(&path, "只有一行").unwrap();
    let note = Note::load(path).unwrap();

    assert!(note.line_byte_range(0).is_some());
    assert!(note.line_byte_range(1).is_none(), "越界应为 None");
    assert!(note.line_byte_range(999).is_none());
}
