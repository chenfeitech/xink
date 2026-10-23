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
fn take_external_text_ignores_change_with_same_content() {
    // 应用自己写盘的回声正是这个形状：mtime 变了，内容一字未改
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "same").unwrap();

    let mut note = Note::load(path.clone()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&path, "same").unwrap();

    assert!(note.disk_changed(), "mtime 变了应被视为一次改动");
    assert!(
        note.take_external_text().unwrap().is_none(),
        "内容一致不应回填，否则编辑控件会把光标重置到开头"
    );
    assert!(!note.disk_changed(), "记下新 mtime 后不应反复报改动");
}

#[test]
fn take_external_text_returns_new_content_when_changed() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "original").unwrap();

    let mut note = Note::load(path.clone()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&path, "modified by another editor").unwrap();

    let taken = note.take_external_text().unwrap();
    assert_eq!(taken.as_deref(), Some("modified by another editor"));
    assert_eq!(note.text(), "modified by another editor");
    assert!(!note.is_dirty());
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

/// 一串确定性伪随机编辑：把 `Note` 与 `String` 模型逐轮对齐
///
/// 增量编辑的正确性全落在"前后缀切在哪"这一步，人肉挑几个例子盖不住
/// （切在多字节字符中间会直接 panic）。这里用固定种子的 LCG 造出插入 /
/// 删除 / 替换三种改动，逐次与 String 模型比对，任何一处切错都会立刻暴露。
#[test]
fn set_text_matches_string_model_under_many_edits() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "起始内容").unwrap();
    let mut note = Note::load(path).unwrap();

    let alphabet = [
        'a',
        'b',
        '中',
        '\u{6587}',
        '\u{e9}',
        '\u{1f600}',
        '\n',
        '\t',
    ];
    let mut seed: u64 = 0x5DEE_CE66_D0FE_8877;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };

    let mut model = String::from("起始内容");
    let mut expected_version = note.version();

    for round in 0..500 {
        let chars: Vec<char> = model.chars().collect();
        // 以**字符**为单位取位置，别让测试自己先切出非法边界
        let at = next() % (chars.len() + 1);
        let byte_at = chars[..at].iter().map(|c| c.len_utf8()).sum::<usize>();
        let before = model.clone();

        match next() % 3 {
            // 插入一个字符
            0 => {
                let ch = alphabet[next() % alphabet.len()];
                model.insert(byte_at, ch);
            }
            // 删除一个字符
            1 if at < chars.len() => {
                model.replace_range(byte_at..byte_at + chars[at].len_utf8(), "");
            }
            // 替换一个字符
            2 if at < chars.len() => {
                let ch = alphabet[next() % alphabet.len()];
                model.replace_range(byte_at..byte_at + chars[at].len_utf8(), &ch.to_string());
            }
            _ => continue,
        }

        // 替换成同一个字符等于没改：版本不该动
        if model != before {
            expected_version += 1;
        }
        note.set_text(&model);
        assert_eq!(note.text(), model, "第 {round} 轮编辑后内容应与模型一致");
        assert_eq!(
            note.version(),
            expected_version,
            "第 {round} 轮应只递增一次版本"
        );
        assert!(note.is_dirty(), "第 {round} 轮应置脏");
    }
}

#[test]
fn set_text_edits_only_the_changed_middle() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "第一行\n第二行\n第三行").unwrap();
    let mut note = Note::load(path).unwrap();

    // 只动中间一行：前后缀都在，替换段应该只有"第二行"那几个字
    note.set_text("第一行\n第二行改了\n第三行");
    assert_eq!(note.text(), "第一行\n第二行改了\n第三行");
    assert_eq!(note.version(), 1, "一次真实改动只递增一次版本");

    // 内容没变：不动内容、不置脏、不递增
    note.set_text("第一行\n第二行改了\n第三行");
    assert_eq!(note.version(), 1, "内容未变不应递增版本");
}

#[test]
fn set_text_handles_multibyte_and_emoji() {
    let dir = tmp();
    let path = dir.path().join("a.md");
    fs::write(&path, "a\u{e9}\u{1f600}b").unwrap();
    let mut note = Note::load(path).unwrap();

    // 改中间的 emoji：前后缀都跨越多字节字符，
    // 若按字节而不是按 char 求前后缀，这里会 panic
    note.set_text("a\u{e9}\u{1f980}b");
    assert_eq!(note.text(), "a\u{e9}\u{1f980}b");

    // 在开头插中文：前缀为 0，后缀要一路跨过多字节字符
    note.set_text("中文a\u{e9}\u{1f980}b");
    assert_eq!(note.text(), "中文a\u{e9}\u{1f980}b");

    // 清空
    note.set_text("");
    assert_eq!(note.text(), "");
}

#[test]
fn common_affixes_stays_on_char_boundaries() {
    let rope = Rope::from_str("a\u{e9}\u{1f600}b");

    // 只换中间的 emoji：前缀 a(1)+é(2)=3，后缀 b(1)
    let (prefix, suffix) = common_affixes(&rope, "a\u{e9}\u{1f980}b");
    assert_eq!(prefix, 3, "前缀应停在字符边界上（a=1 字节, é=2 字节）");
    assert_eq!(suffix, 1, "后缀应含 b");
    assert!(prefix + suffix < rope.len(), "前后缀不交叉，中间才有得替换");

    // 整篇相同：前缀覆盖全文，后缀为 0（绝不能交叉成负长度）
    let (prefix, suffix) = common_affixes(&rope, "a\u{e9}\u{1f600}b");
    assert_eq!(prefix, rope.len());
    assert_eq!(suffix, 0);

    // 任一侧为空
    let (prefix, suffix) = common_affixes(&Rope::from_str(""), "abc");
    assert_eq!((prefix, suffix), (0, 0));
    let (prefix, suffix) = common_affixes(&rope, "");
    assert_eq!((prefix, suffix), (0, 0));
}
