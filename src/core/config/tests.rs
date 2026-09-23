use super::*;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

// ── 默认值 ────────────────────────────────────────────────────

#[test]
fn defaults_are_sane() {
    let c = AppConfig::default();
    assert_eq!(c.basic.theme, ThemeMode::System, "默认应跟随系统主题");
    assert_eq!(c.basic.language, Language::Zh);
    assert_eq!(c.basic.icon_size, 16);
    assert_eq!(c.basic.tree_font.size, 14.0);
    assert_eq!(c.basic.tree_font.line_height, 1.5);

    assert_eq!(c.editor.default_mode, EditorMode::Source);
    assert!(c.editor.auto_save, "默认应开启自动保存");
    // 对齐 REQUIREMENTS.md R2.2 的"停止输入约 300ms 后写入"
    assert_eq!(c.editor.auto_save_delay_ms, 300);
    assert!(!c.editor.indent.use_tabs, "默认用空格缩进");
    assert_eq!(c.editor.indent.tab_size, 4);
    assert!(c.editor.keymap.is_empty());

    assert_eq!(c.preview.code_theme, "base16-ocean.dark");
    assert!(c.preview.sync_scroll, "默认应开启同步滚动");
    assert_eq!(c.preview.image_display, ImageDisplay::FitWidth);
    assert_eq!(c.preview.link_open, LinkOpen::InApp);
}

#[test]
fn editor_default_font_is_monospaced() {
    assert_eq!(FontConfig::mono().family, ".SystemMonospacedFont");
    assert_eq!(
        AppConfig::default().editor.font.family,
        ".SystemMonospacedFont"
    );
}

// ── 兼容性：缺字段 / 多字段 ────────────────────────────────────

#[test]
fn empty_json_falls_back_to_defaults() {
    let c: AppConfig = serde_json::from_str("{}").expect("空对象应能解析");
    assert_eq!(c, AppConfig::default());
}

#[test]
fn partial_json_keeps_specified_values_and_defaults_the_rest() {
    // 模拟老版本配置文件：只有少数字段
    let json = r#"{"basic":{"icon_size":24}}"#;
    let c: AppConfig = serde_json::from_str(json).expect("缺字段应能解析");
    assert_eq!(c.basic.icon_size, 24, "显式指定的值应保留");
    assert_eq!(c.basic.theme, ThemeMode::System, "未指定的应回落默认");
    assert_eq!(c.editor, EditorConfig::default());
    assert_eq!(c.preview, PreviewConfig::default());
}

#[test]
fn unknown_fields_are_ignored() {
    let json = r#"{"basic":{"icon_size":20},"future_section":{"x":1}}"#;
    let c: AppConfig = serde_json::from_str(json).expect("未知字段不应导致解析失败");
    assert_eq!(c.basic.icon_size, 20);
}

#[test]
fn enum_values_are_lowercase_in_json() {
    let json = serde_json::to_string(&AppConfig::default()).unwrap();
    assert!(
        json.contains("\"theme\":\"system\""),
        "主题应为小写: {json}"
    );
    assert!(json.contains("\"sync_scroll\":true"));
    assert!(
        json.contains("\"image_display\":\"fit_width\""),
        "多词枚举用 snake_case: {json}"
    );
}

// ── 序列化往返 ────────────────────────────────────────────────

#[test]
fn roundtrip_preserves_values() {
    let mut c = AppConfig::default();
    c.basic.theme = ThemeMode::Dark;
    c.basic.tree_font.size = 18.5;
    c.editor.auto_save = false;
    c.editor.indent.use_tabs = true;
    c.editor.indent.tab_size = 2;
    c.editor.keymap.insert("bold".into(), "cmd-b".into());
    c.preview.code_theme = "InspiredGitHub".into();
    c.preview.sync_scroll = false;

    let text = serde_json::to_string(&c).unwrap();
    let back: AppConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, c);
    assert_eq!(
        back.editor.keymap.get("bold").map(String::as_str),
        Some("cmd-b")
    );
}

// ── 区间校正 ──────────────────────────────────────────────────

#[test]
fn sanitize_clamps_out_of_range_values() {
    let mut c = AppConfig::default();
    c.basic.tree_font.size = 999.0;
    c.basic.tree_font.line_height = 0.01;
    c.basic.icon_size = 0;
    c.editor.auto_save_delay_ms = 0;
    c.editor.indent.tab_size = 100;
    c.preview.max_width = 999_999;

    c.sanitize();

    assert_eq!(c.basic.tree_font.size, 72.0, "字号上限");
    assert_eq!(c.basic.tree_font.line_height, 0.8, "行高下限");
    assert_eq!(c.basic.icon_size, 12, "图标下限");
    assert_eq!(c.editor.auto_save_delay_ms, 200, "保存延迟下限");
    assert_eq!(c.editor.indent.tab_size, 16, "缩进上限");
    assert_eq!(c.preview.max_width, 4_096);
}

#[test]
fn sanitize_leaves_valid_values_untouched() {
    let mut c = AppConfig::default();
    c.basic.tree_font.size = 16.0;
    c.editor.auto_save_delay_ms = 2_500;
    c.editor.indent.tab_size = 2;
    let expected = c.clone();

    c.sanitize();
    assert_eq!(c, expected, "合法值不应被改动");
}

#[test]
fn sanitize_repairs_nan_font_size() {
    let mut c = AppConfig::default();
    c.basic.tree_font.size = f32::NAN;
    c.sanitize();
    assert_eq!(c.basic.tree_font.size, 8.0, "NaN 应回落到下限而不是传播");
}

#[test]
fn sanitize_restores_empty_font_family_and_code_theme() {
    let mut c = AppConfig::default();
    c.basic.tree_font.family = "   ".into();
    c.preview.code_theme = String::new();

    c.sanitize();
    assert_eq!(c.basic.tree_font.family, ".SystemUIFont");
    assert_eq!(c.preview.code_theme, "base16-ocean.dark");
}

// ── 导入 / 导出 ───────────────────────────────────────────────

#[test]
fn export_and_import_roundtrip() {
    let dir = tmpdir();
    let file = dir.path().join("backup").join("config.json");

    let mut c = AppConfig::default();
    c.basic.theme = ThemeMode::Light;
    c.preview.sync_scroll = false;

    c.export_to(&file).expect("导出应成功");
    assert!(file.exists());

    let imported = AppConfig::import_from(&file).expect("导入应成功");
    assert_eq!(imported, c, "导出的配置应能原样导入");
}

#[test]
fn import_from_missing_file_reports_error() {
    let dir = tmpdir();
    let err = AppConfig::import_from(&dir.path().join("nope.json"));
    assert!(err.is_err(), "文件不存在应报错而不是 panic");
}

#[test]
fn import_from_broken_json_reports_error() {
    let dir = tmpdir();
    let file = dir.path().join("broken.json");
    fs::write(&file, "{ not json").unwrap();
    assert!(AppConfig::import_from(&file).is_err(), "损坏文件应报错");
}

#[test]
fn import_from_foreign_json_falls_back_to_defaults() {
    let dir = tmpdir();
    let file = dir.path().join("foreign.json");
    fs::write(&file, r#"{"something":"else"}"#).unwrap();
    let imported = AppConfig::import_from(&file).expect("无关字段不影响解析");
    assert_eq!(imported, AppConfig::default());
}

// ── 辅助方法 ──────────────────────────────────────────────────

#[test]
fn font_weight_values_are_valid_css_weights() {
    assert_eq!(FontWeight::Regular.value(), 400.0);
    assert_eq!(FontWeight::Medium.value(), 500.0);
    assert_eq!(FontWeight::Semibold.value(), 600.0);
    assert_eq!(FontWeight::Bold.value(), 700.0);
}
