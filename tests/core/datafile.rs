use std::path::Path;

use super::*;

#[test]
fn data_kind_matches_config_extensions() {
    assert_eq!(data_kind(Path::new("a.json")), Some(DataKind::Json));
    assert_eq!(data_kind(Path::new("a.yaml")), Some(DataKind::Yaml));
    assert_eq!(data_kind(Path::new("a.yml")), Some(DataKind::Yaml));
    assert_eq!(data_kind(Path::new("a.toml")), Some(DataKind::Toml));
    // 大小写不敏感
    assert_eq!(data_kind(Path::new("a.JSON")), Some(DataKind::Json));
    assert_eq!(data_kind(Path::new("a.YAML")), Some(DataKind::Yaml));
}

#[test]
fn data_kind_rejects_non_config_files() {
    // 只看最后一个扩展名：备份文件不能被当成配置打开
    assert_eq!(data_kind(Path::new("a.json.bak")), None);
    assert_eq!(data_kind(Path::new("a.md")), None);
    assert_eq!(data_kind(Path::new("a.png")), None);
    assert_eq!(data_kind(Path::new("a")), None);
    assert!(!is_data_file(Path::new("a.md")));
}

#[test]
fn json_pretty_uses_configured_indent() {
    let text = r#"{"a":1,"b":[1,2]}"#;

    let two = parse(text, DataKind::Json, 2)
        .formatted()
        .unwrap()
        .to_string();
    assert!(two.contains("\n  \"a\": 1"), "缩进 2：{two:?}");

    let four = parse(text, DataKind::Json, 4)
        .formatted()
        .unwrap()
        .to_string();
    assert!(four.contains("\n    \"a\": 1"), "缩进 4：{four:?}");
}

#[test]
fn json_error_reports_zero_based_position() {
    let parsed = parse(r#"{"a": 1,}"#, DataKind::Json, 2);
    let error = parsed.error().unwrap();

    // serde_json 报 line 1 column 9（1-based），这里必须是 0-based
    assert_eq!(error.line, 0);
    assert_eq!(error.column, 8);
    assert!(!error.message.is_empty());
}

#[test]
fn yaml_formatting_drops_comments() {
    let parsed = parse("# 说明\nname: myapp\n", DataKind::Yaml, 2);
    let formatted = parsed.formatted().unwrap().to_string();

    // 固化"有损"这个已知行为：注释确实会丢，所以格式化结果只进预览。
    // `noyalib::to_string` 不追加结尾换行。
    assert_eq!(formatted, "name: myapp");
    assert!(!formatted.contains('#'));
}

#[test]
fn yaml_error_reports_zero_based_position() {
    let parsed = parse("a: 1\n  b: 2\n", DataKind::Yaml, 2);
    let error = parsed.error().unwrap();

    // noyalib 报 line 2 column 4（1-based）
    assert_eq!(error.line, 1);
    assert_eq!(error.column, 3);
    assert!(!error.message.is_empty());
}

#[test]
fn yaml_multi_document_is_rejected() {
    let parsed = parse("a: 1\n---\nb: 2\n", DataKind::Yaml, 2);
    let error = parsed.error().unwrap();

    assert_eq!(error.line, 1, "分隔符所在行");
    assert!(error.message.contains("多文档"), "{}", error.message);
}

#[test]
fn yaml_leading_document_marker_is_accepted() {
    // 首行的 --- 是文档起始标记，不是多文档分隔
    assert!(parse("---\na: 1\n", DataKind::Yaml, 2).is_ok());
}

#[test]
fn toml_formats_and_reports_position() {
    let formatted = parse("a = 1\n[t]\nx = 2\n", DataKind::Toml, 2)
        .formatted()
        .unwrap()
        .to_string();
    assert!(formatted.contains("[t]"), "{formatted:?}");

    let error = parse("[]", DataKind::Toml, 2).error().unwrap().clone();
    // toml 只给字节区间 1..2，换算后是第 0 行第 1 列
    assert_eq!((error.line, error.column), (0, 1));
}

#[test]
fn blank_content_is_valid_for_every_kind() {
    for kind in [DataKind::Json, DataKind::Yaml, DataKind::Toml] {
        for text in ["", "   \n\n"] {
            let parsed = parse(text, kind, 2);
            assert!(parsed.is_ok(), "{kind:?} 空内容不应报错: {parsed:?}");
            assert_eq!(parsed.formatted(), Some(""));
        }
    }
}

#[test]
fn offset_to_position_counts_lines_and_columns() {
    let text = "ab\ncd\n";
    assert_eq!(offset_to_position(text, 0), (0, 0));
    assert_eq!(offset_to_position(text, 2), (0, 2));
    assert_eq!(offset_to_position(text, 3), (1, 0));
    // 越界按末尾处理，绝不 panic
    assert_eq!(offset_to_position(text, 99), (2, 0));
}

#[test]
fn long_messages_are_truncated() {
    let long: String = "x".repeat(500);
    let short = shorten(&long);
    assert!(short.ends_with('…'));
    assert_eq!(short.chars().count(), MAX_MESSAGE_LEN + 1);
}

#[test]
fn yaml_message_drops_duplicated_location_prefix() {
    // 行列我们自己展示，消息里不再重复一遍
    assert_eq!(
        strip_location_prefix("YAML parse error at line 2, column 4: bad indent"),
        "bad indent"
    );
    assert_eq!(strip_location_prefix("other error"), "other error");
}

#[test]
fn kind_metadata_exposes_lang_and_notice() {
    assert_eq!(DataKind::Json.lang(), "json");
    assert_eq!(DataKind::Yaml.lang(), "yaml");
    assert_eq!(DataKind::Toml.label(), "TOML");
    // 只有 YAML 的格式化是有损的，必须有说明
    assert!(DataKind::Yaml.notice().is_some());
    assert!(DataKind::Json.notice().is_none());
}
