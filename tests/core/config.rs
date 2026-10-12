use super::*;
use crate::core::datafile::{DEFAULT_INDENT, DEFAULT_MAX_FORMAT_BYTES};
use crate::core::highlight::default_theme;

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

    assert!(c.editor.auto_save, "默认应开启自动保存");
    // 对齐 REQUIREMENTS.md R2.2 的"停止输入约 300ms 后写入"
    assert_eq!(c.editor.auto_save_delay_ms, 300);
    assert!(!c.editor.indent.use_tabs, "默认用空格缩进");
    assert_eq!(c.editor.indent.tab_size, 4);
    assert!(c.editor.keymap.is_empty());

    assert_eq!(c.preview.code_theme, AUTO_CODE_THEME);
    assert!(c.preview.sync_scroll, "默认应开启同步滚动");
    assert_eq!(c.preview.image_display, ImageDisplay::FitWidth);
    assert_eq!(c.preview.image_width_percent, 100, "默认占满预览区宽度");
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
    assert_eq!(c.preview.code_theme, AUTO_CODE_THEME);
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
fn sanitize_clamps_image_width_percent() {
    let mut c = AppConfig::default();
    c.preview.image_width_percent = 150;
    c.sanitize();
    assert_eq!(c.preview.image_width_percent, 100, "超出上限应夹取");

    c.preview.image_width_percent = 5;
    c.sanitize();
    assert_eq!(c.preview.image_width_percent, 20, "低于下限应夹取");
}

#[test]
fn legacy_image_display_value_does_not_break_config() {
    // 已移除的"原始尺寸"若出现在旧 config.json 里，不能让整个文件解析失败
    // （否则会静默回落默认值，用户所有设置丢失）
    let json = r#"{"preview":{"image_display":"original"}}"#;
    let mut c: AppConfig = serde_json::from_str(json).expect("旧值应能解析");
    assert!(
        matches!(c.preview.image_display, ImageDisplay::Legacy),
        "未知选项应落在 Legacy 变体"
    );
    c.sanitize();
    assert!(
        matches!(c.preview.image_display, ImageDisplay::FitWidth),
        "Legacy 应被夹回适应宽度"
    );
}

#[test]
fn font_weight_values_are_valid_css_weights() {
    assert_eq!(FontWeight::Regular.value(), 400.0);
    assert_eq!(FontWeight::Medium.value(), 500.0);
    assert_eq!(FontWeight::Semibold.value(), 600.0);
    assert_eq!(FontWeight::Bold.value(), 700.0);
}

// ── 配置修改 ──────────────────────────────────────────────────
//
// 这一组覆盖"用户改了某一项之后会怎样"：先按 `update_config` 的纯逻辑部分
// （改值 → sanitize → 序列化）走一遍，再反序列化回来，确认改动既没被吞掉
// 也没被错误地改写成别的值。UI 层（需要 App 上下文）无法单测，故只覆盖到这里。

#[test]
fn code_theme_auto_follows_appearance() {
    let dark = resolve_code_theme(AUTO_CODE_THEME, true);
    let light = resolve_code_theme(AUTO_CODE_THEME, false);

    assert_eq!(dark, default_theme(true), "深色下解析为深色高亮主题");
    assert_eq!(light, default_theme(false), "浅色下解析为浅色高亮主题");
    assert_ne!(dark, light, "「跟随主题」必须随明暗变化，否则等于没接");
}

#[test]
fn code_theme_explicit_value_is_not_overridden() {
    // 指定具体主题名时不随明暗变化
    assert_eq!(resolve_code_theme("InspiredGitHub", true), "InspiredGitHub");
    assert_eq!(
        resolve_code_theme("InspiredGitHub", false),
        "InspiredGitHub"
    );

    // 未知主题名原样传出：由高亮层负责回落，配置层不吞掉用户的值
    assert_eq!(resolve_code_theme("no-such-theme", true), "no-such-theme");
}

#[test]
fn modified_wired_settings_survive_sanitize_and_reload() {
    let mut c = AppConfig::default();
    // 逐个覆盖本轮接线的配置项
    c.editor.show_line_numbers = true;
    c.editor.indent.use_tabs = true;
    c.editor.indent.tab_size = 8;
    c.editor.font.weight = FontWeight::Bold;
    c.preview.font.weight = FontWeight::Semibold;
    c.preview.code_theme = AUTO_CODE_THEME.into();
    c.preview.image_display = ImageDisplay::Hidden;
    c.preview.image_width_percent = 60;
    c.preview.link_open = LinkOpen::External;
    c.preview.sync_scroll = false;

    // 模拟 update_config 的纯逻辑部分：改完 → 校正 → 落盘 → 再载入
    c.sanitize();
    let text = serde_json::to_string(&c).unwrap();
    let back: AppConfig = serde_json::from_str(&text).unwrap();

    assert_eq!(back, c, "改动不应被校正或序列化吞掉");
    assert!(back.editor.show_line_numbers);
    assert!(back.editor.indent.use_tabs);
    assert_eq!(back.editor.indent.tab_size, 8);
    assert_eq!(back.editor.font.weight, FontWeight::Bold);
    assert_eq!(back.preview.font.weight, FontWeight::Semibold);
    assert_eq!(back.preview.code_theme, AUTO_CODE_THEME);
    assert_eq!(back.preview.image_display, ImageDisplay::Hidden);
    assert_eq!(back.preview.image_width_percent, 60);
    assert_eq!(back.preview.link_open, LinkOpen::External);
    assert!(!back.preview.sync_scroll);
}

#[test]
fn modified_out_of_range_values_are_clamped_not_reverted() {
    let mut c = AppConfig::default();
    // 越界值：夹到边界，而不是回到默认值
    c.editor.indent.tab_size = 100;
    c.preview.image_width_percent = 5;
    c.basic.icon_size = 999;
    // 合法改动：必须原样保留
    c.editor.show_line_numbers = true;
    c.editor.font.weight = FontWeight::Medium;

    c.sanitize();

    assert_eq!(c.editor.indent.tab_size, limit::TAB_SIZE.1, "夹到上限");
    assert_eq!(
        c.preview.image_width_percent,
        limit::IMAGE_WIDTH_PERCENT.0,
        "夹到下限"
    );
    assert_eq!(c.basic.icon_size, limit::ICON_SIZE.1, "夹到上限");
    // 越界的校正不能波及未越界的改动
    assert!(c.editor.show_line_numbers, "合法改动不应被重置");
    assert_eq!(c.editor.font.weight, FontWeight::Medium);
}

#[test]
fn modified_config_keeps_unmodified_fields_at_defaults() {
    let mut c = AppConfig::default();
    let before = c.clone();
    // 只改一项
    c.editor.show_line_numbers = true;

    c.sanitize();
    let text = serde_json::to_string(&c).unwrap();
    let back: AppConfig = serde_json::from_str(&text).unwrap();

    let mut expected = before;
    expected.editor.show_line_numbers = true;
    assert_eq!(back, expected, "只改一项时，其余字段必须保持原样");
}

#[test]
fn preview_density_defaults_to_normal() {
    let c = AppConfig::default();
    assert_eq!(c.preview.density, PreviewDensity::Normal, "默认应为适中");
}

#[test]
fn preview_density_is_lowercase_in_json() {
    let mut c = AppConfig::default();
    c.preview.density = PreviewDensity::Relaxed;
    let text = serde_json::to_string(&c).unwrap();
    assert!(text.contains("\"density\":\"relaxed\""), "实际: {text}");
}

#[test]
fn preview_density_survives_roundtrip() {
    let mut c = AppConfig::default();
    c.preview.density = PreviewDensity::Compact;
    let text = serde_json::to_string(&c).unwrap();
    let back: AppConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back.preview.density, PreviewDensity::Compact);
}

#[test]
fn density_layouts_are_ordered_by_looseness() {
    let c = PreviewDensity::Compact.layout();
    let n = PreviewDensity::Normal.layout();
    let r = PreviewDensity::Relaxed.layout();

    assert!(
        c.paragraph_gap < n.paragraph_gap && n.paragraph_gap < r.paragraph_gap,
        "段间距应递增"
    );
    assert!(c.pad_x < n.pad_x && n.pad_x < r.pad_x, "左右留白应递增");
    assert!(
        c.max_width > n.max_width && n.max_width > r.max_width,
        "越宽松正文越窄"
    );
}

#[test]
fn density_value_roundtrip_and_fallback() {
    for d in [
        PreviewDensity::Compact,
        PreviewDensity::Normal,
        PreviewDensity::Relaxed,
    ] {
        assert_eq!(PreviewDensity::from_value(&d.value()), d);
    }
    assert_eq!(
        PreviewDensity::from_value("unknown"),
        PreviewDensity::Normal,
        "未知取值应回落到适中"
    );
}

// ── 数据文件（json / yaml / toml）配置 ────────────────────────

#[test]
fn data_file_defaults_match_datafile_module() {
    let c = AppConfig::default();
    assert_eq!(c.editor.data_file.indent, DEFAULT_INDENT);
    assert_eq!(
        c.editor.data_file.max_format_bytes,
        DEFAULT_MAX_FORMAT_BYTES
    );
}

#[test]
fn data_file_values_survive_roundtrip() {
    let mut c = AppConfig::default();
    c.editor.data_file.indent = 4;
    c.editor.data_file.max_format_bytes = 2_000_000;

    let text = serde_json::to_string(&c).unwrap();
    let back: AppConfig = serde_json::from_str(&text).unwrap();

    assert_eq!(back.editor.data_file.indent, 4);
    assert_eq!(back.editor.data_file.max_format_bytes, 2_000_000);
}

#[test]
fn data_file_missing_in_old_config_falls_back_to_defaults() {
    // 老配置文件里没有 data_file 段，应自动取默认值
    let json = r#"{"editor":{"auto_save":false}}"#;
    let c: AppConfig = serde_json::from_str(json).expect("缺字段应能解析");
    assert_eq!(c.editor.data_file, DataFileConfig::default());
}

#[test]
fn data_file_out_of_range_values_are_clamped() {
    let mut c = AppConfig::default();
    c.editor.data_file.indent = 100;
    c.editor.data_file.max_format_bytes = usize::MAX;

    c.sanitize();

    assert_eq!(c.editor.data_file.indent, 8);
    assert_eq!(c.editor.data_file.max_format_bytes, 20_000_000);
}

#[test]
fn data_file_zero_max_bytes_means_unlimited_and_is_kept() {
    let mut c = AppConfig::default();
    c.editor.data_file.max_format_bytes = 0;
    c.sanitize();
    assert_eq!(c.editor.data_file.max_format_bytes, 0);
}
