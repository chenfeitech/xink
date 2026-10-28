use super::*;
use crate::core::datafile::{DEFAULT_INDENT, DEFAULT_MAX_FORMAT_BYTES};
use crate::core::theme;

fn tmpdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("创建临时目录")
}

// ── 默认值 ────────────────────────────────────────────────────

#[test]
fn defaults_are_sane() {
    let c = AppConfig::default();
    assert_eq!(c.basic.theme, ThemeMode::System, "默认应跟随系统主题");
    assert_eq!(c.basic.icon_size, 16);
    assert_eq!(c.basic.tree_font.size, 14.0);
    assert_eq!(c.basic.tree_font.line_height, 1.5);

    assert!(c.editor.auto_save, "默认应开启自动保存");
    // 对齐 REQUIREMENTS.md R2.2 的"停止输入约 300ms 后写入"
    assert_eq!(c.editor.auto_save_delay_ms, 300);
    assert!(!c.editor.indent.use_tabs, "默认用空格缩进");
    assert_eq!(c.editor.indent.tab_size, 4);
    assert!(c.editor.keymap.is_empty());

    assert_eq!(c.highlight.theme, theme::DEFAULT_THEME);
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
    c.highlight.theme = "xink".into();
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
fn sanitize_restores_empty_font_family() {
    let mut c = AppConfig::default();
    c.basic.tree_font.family = "   ".into();

    c.sanitize();
    assert_eq!(c.basic.tree_font.family, ".SystemUIFont");
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
fn toggling_theme_always_flips_the_appearance() {
    // 跟随系统时按下切换：切到与当前外观相反的模式，用户一定能看到变化
    assert_eq!(ThemeMode::System.toggled(true), ThemeMode::Light);
    assert_eq!(ThemeMode::System.toggled(false), ThemeMode::Dark);
    // 已明确指定时在两者之间来回
    assert_eq!(ThemeMode::Light.toggled(false), ThemeMode::Dark);
    assert_eq!(ThemeMode::Dark.toggled(true), ThemeMode::Light);
    // 明确模式下 `is_dark` 不参与决定，避免系统外观变化后切换方向反直觉
    assert_eq!(ThemeMode::Light.toggled(true), ThemeMode::Dark);
    assert_eq!(ThemeMode::Dark.toggled(false), ThemeMode::Light);
}

#[test]
fn toggled_theme_never_falls_back_to_system() {
    for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
        for is_dark in [true, false] {
            assert_ne!(
                mode.toggled(is_dark),
                ThemeMode::System,
                "切换后应停留在明确模式，否则用户脱离不了「跟随系统」"
            );
        }
    }
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
    c.highlight.theme = "xink".into();
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
    assert_eq!(back.highlight.theme, "xink");
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

// ── 统一高亮配置 ──────────────────────────────────────────────

#[test]
fn highlight_theme_defaults_to_preset() {
    let c = AppConfig::default();
    assert_eq!(c.highlight.theme, theme::DEFAULT_THEME);
    assert!(
        theme::PRESET_THEMES.contains(&c.highlight.theme.as_str()),
        "默认主题必须是预置主题之一"
    );
}

#[test]
fn legacy_code_theme_is_migrated_once() {
    // 旧配置：用户选过 syntect 主题。升级后应落到预置主题上，且旧字段被丢弃，
    // 避免下次启动时再迁移一次、把用户后选的 highlight.theme 顶掉。
    let mut c = AppConfig::default();
    c.preview.legacy_code_theme = Some("InspiredGitHub".into());

    c.sanitize();
    assert!(
        c.preview.legacy_code_theme.is_none(),
        "迁移后旧字段必须被丢弃"
    );
    assert_eq!(
        c.highlight.theme,
        theme::migrate_code_theme("InspiredGitHub")
    );

    // 写回配置时不应再带上旧字段
    let text = serde_json::to_string(&c).unwrap();
    assert!(
        !text.contains("code_theme"),
        "序列化结果不应再含 code_theme: {text}"
    );

    // 再跑一次：旧字段已不在，不应再动 highlight.theme
    c.highlight.theme = "xink".into();
    c.sanitize();
    assert_eq!(c.highlight.theme, "xink");
}

#[test]
fn unknown_highlight_theme_falls_back_to_default() {
    // 手动改 config.json 写了不存在的主题名：规范化成默认，而不是让编辑区不着色
    let mut c = AppConfig::default();
    c.highlight.theme = "no-such-theme".into();

    c.sanitize();
    assert_eq!(c.highlight.theme, theme::DEFAULT_THEME);
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

// ── AI 配置 ──────────────────────────────────────────────────
//
// 这一组是"设置里填了越界值会怎样"的唯一护栏：AI 配置是 M2/M3 才新增的，
// 手改 config.json 的人很容易写出 0 / 空 / 巨大值，夹取错了不会报错，
// 只会表现为"发不出去"或"必然超时"。

fn model(id: &str) -> ModelEntry {
    ModelEntry {
        id: id.to_string(),
        ..ModelEntry::default()
    }
}

#[test]
fn ai_out_of_range_values_are_clamped() {
    let mut ai = AiConfig {
        timeout_secs: 0,
        max_context_chars: 1,
        max_steps: 0,
        max_tool_chars: 1,
        ..AiConfig::default()
    };
    ai.sanitize();
    // 下限 5s 是特意抬高的：手滑设成 0 会导致每次请求必然超时
    assert_eq!(ai.timeout_secs, 5);
    assert_eq!(ai.max_context_chars, 1_000);
    assert_eq!(ai.max_steps, 1);
    assert_eq!(ai.max_tool_chars, 500);

    ai.timeout_secs = u64::MAX;
    ai.max_context_chars = usize::MAX;
    ai.max_steps = 999;
    ai.max_tool_chars = usize::MAX;
    ai.sanitize();
    assert_eq!(ai.timeout_secs, 600);
    assert_eq!(ai.max_context_chars, 200_000);
    assert_eq!(ai.max_steps, 64);
    assert_eq!(ai.max_tool_chars, 20_000);
}

#[test]
fn ai_auto_apply_is_always_forced_off() {
    // 需求 A7.1：AI 的改写必须经人确认，配置里写了 true 也不认
    let mut ai = AiConfig {
        auto_apply: true,
        ..AiConfig::default()
    };
    ai.sanitize();
    assert!(!ai.auto_apply);
}

#[test]
fn ai_models_without_an_id_are_dropped() {
    let mut ai = AiConfig {
        models: vec![model("keep"), model(""), model("   ")],
        ..AiConfig::default()
    };
    ai.sanitize();
    let ids: Vec<&str> = ai.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["keep"], "空 id 的条目多半是手改配置留下的");
}

#[test]
fn ai_temperature_nan_and_out_of_range_are_clamped() {
    let mut ai = AiConfig {
        models: vec![ModelEntry {
            params: AiModelParams {
                temperature: f32::NAN,
                ..AiModelParams::default()
            },
            ..model("m")
        }],
        ..AiConfig::default()
    };
    ai.sanitize();
    // NaN 不能靠 `clamp`：它跟谁比都是 false，会原样穿过去
    assert_eq!(ai.models[0].params.temperature, 0.0);

    ai.models[0].params.temperature = -1.0;
    ai.sanitize();
    assert_eq!(ai.models[0].params.temperature, 0.0);
    ai.models[0].params.temperature = 9.0;
    ai.sanitize();
    assert_eq!(ai.models[0].params.temperature, 2.0);
}

#[test]
fn ai_num_ctx_is_clamped_by_the_models_own_context_limit() {
    let mut ai = AiConfig {
        models: vec![
            ModelEntry {
                context_limit: Some(4096),
                params: AiModelParams {
                    num_ctx: 1_000_000,
                    ..AiModelParams::default()
                },
                ..model("small")
            },
            ModelEntry {
                params: AiModelParams {
                    num_ctx: u32::MAX,
                    ..AiModelParams::default()
                },
                ..model("unknown")
            },
        ],
        ..AiConfig::default()
    };
    ai.sanitize();
    // 模型自己声明的上下文上限更小，按它来；没声明的按全局上限
    assert_eq!(ai.models[0].params.num_ctx, 4096);
    assert_eq!(ai.models[1].params.num_ctx, 1_048_576);

    ai.models[0].params.num_ctx = 0;
    ai.sanitize();
    assert_eq!(ai.models[0].params.num_ctx, 512, "下限不能被压穿");
}

#[test]
fn ai_num_predict_is_clamped() {
    let mut ai = AiConfig {
        models: vec![ModelEntry {
            params: AiModelParams {
                num_predict: 0,
                ..AiModelParams::default()
            },
            ..model("m")
        }],
        ..AiConfig::default()
    };
    ai.sanitize();
    assert_eq!(ai.models[0].params.num_predict, 16);

    ai.models[0].params.num_predict = u32::MAX;
    ai.sanitize();
    assert_eq!(ai.models[0].params.num_predict, 32_768);
}

#[test]
fn ai_default_model_is_cleared_when_it_cannot_be_used() {
    // "看着配好了却发不出去"是最难查的一类：选中的模型不存在或被停用
    let mut ai = AiConfig {
        default_model: "gone".into(),
        ..AiConfig::default()
    };
    ai.sanitize();
    assert!(ai.default_model.is_empty(), "不存在的模型要清掉选择");

    let mut disabled = model("m");
    disabled.enabled = false;
    ai.default_model = "m".into();
    ai.models = vec![disabled];
    ai.sanitize();
    assert!(ai.default_model.is_empty(), "被停用的模型也要清掉选择");

    ai.models = vec![model("m")];
    ai.default_model = "m".into();
    ai.sanitize();
    assert_eq!(ai.default_model, "m", "能用的选择要保留");
}

#[test]
fn active_model_requires_enabled_and_a_known_id() {
    let mut ai = AiConfig::default();
    assert!(ai.active_model().is_none(), "没选模型时是 None");

    let mut off = model("m");
    off.enabled = false;
    ai.models = vec![off];
    ai.default_model = "m".into();
    assert!(ai.active_model().is_none(), "停用等于没选");

    ai.models = vec![model("m")];
    assert!(ai.active_model().is_some());
}

#[test]
fn is_ready_needs_both_the_switch_and_a_model() {
    let mut ai = AiConfig {
        models: vec![model("m")],
        default_model: "m".into(),
        ..AiConfig::default()
    };
    assert!(!ai.is_ready(), "总开关没开就不算就绪");

    ai.enabled = true;
    assert!(ai.is_ready());

    ai.default_model.clear();
    assert!(!ai.is_ready(), "没有生效的模型就不算就绪");
}

#[test]
fn base_url_or_falls_back_to_the_provider_default() {
    let ai = AiConfig::default();
    let mut ollama = model("m");
    ollama.kind = AiProviderKind::Ollama;
    assert_eq!(ollama.base_url_or(&ai), ai.ollama.base_url);

    let mut compat = model("c");
    compat.kind = AiProviderKind::OpenAiCompatible;
    assert!(compat.base_url_or(&ai).is_empty(), "兼容端点没有默认值");

    ollama.base_url = "http://box:11434".into();
    assert_eq!(ollama.base_url_or(&ai), "http://box:11434", "条目覆盖优先");

    // 纯空格等同空：否则会拼出一个"看起来填了"的无效地址
    let mut blank = model("b");
    blank.base_url = "   ".into();
    assert_eq!(blank.base_url_or(&ai), ai.ollama.base_url);
}

#[test]
fn endpoint_for_uses_each_providers_shape() {
    let ai = AiConfig::default();
    let mut ollama = model("m");
    ollama.kind = AiProviderKind::Ollama;
    assert!(ai.endpoint_for(&ollama).ends_with("/api/chat"));

    let mut compat = model("c");
    compat.kind = AiProviderKind::OpenAiCompatible;
    compat.base_url = "https://api.example.com/v1/".into();
    let endpoint = ai.endpoint_for(&compat);
    assert_eq!(endpoint, "https://api.example.com/v1/chat/completions");
    assert!(!endpoint.contains("//chat"), "尾斜杠不能拼出双斜杠");
}

#[test]
fn 是否需要_key_只看发起请求的那个模型() {
    // 以前是"全局默认模型是不是 Ollama"，默认模型是本地时云模型取不到 key
    let local = model("m"); // kind 默认 Ollama
    assert!(!AiConfig::needs_key(&local), "本地 Ollama 不要 key");

    let mut compat = model("c");
    compat.kind = AiProviderKind::OpenAiCompatible;
    assert!(AiConfig::needs_key(&compat), "远程端点要 key");

    // 关键：默认模型是本地时，云模型的判定不受影响
    let ai = AiConfig {
        models: vec![local, compat],
        default_model: "m".into(),
        ..Default::default()
    };
    let cloud = ai.models.iter().find(|m| m.id == "c").unwrap();
    assert!(
        AiConfig::needs_key(cloud),
        "默认模型是 Ollama 时云模型仍需 key"
    );
}

#[test]
fn model_display_name_falls_back_to_the_id() {
    let mut m = model("qwen2.5:7b");
    assert_eq!(m.display_name(), "qwen2.5:7b");
    m.label = "  ".into();
    assert_eq!(m.display_name(), "qwen2.5:7b", "纯空格的 label 等同没写");
    m.label = "我的模型".into();
    assert_eq!(m.display_name(), "我的模型");
}

#[test]
fn capabilities_are_matched_exactly() {
    let mut m = model("m");
    m.capabilities = vec!["tools".into()];
    assert!(m.supports_tools());
    assert!(!m.supports_thinking());

    // 大小写不一致不算声明过：宁可让用户手动打开，也别猜
    m.capabilities = vec!["Tools".into()];
    assert!(!m.supports_tools());

    m.capabilities = vec!["thinking".into()];
    assert!(m.supports_thinking());
}

#[test]
fn clamp_f32_survives_nan_and_infinity() {
    assert_eq!(clamp_f32(f32::NAN, (0.0, 2.0)), 0.0);
    assert_eq!(clamp_f32(f32::INFINITY, (0.0, 2.0)), 2.0);
    assert_eq!(clamp_f32(f32::NEG_INFINITY, (0.0, 2.0)), 0.0);
    assert_eq!(clamp_f32(1.5, (0.0, 2.0)), 1.5, "区间内的值原样返回");
}

// ── 快捷动作 ──────────────────────────────────────────────────

#[test]
fn 默认带六个内置动作() {
    let c = AppConfig::default();
    assert_eq!(c.ai.actions.len(), 6);
    assert!(c.ai.action("polish").is_some());
    assert!(c.ai.action("tag").is_some());
    // 精简掉的两个不再存在
    assert!(c.ai.action("expand").is_none());
    assert!(c.ai.action("continue").is_none());
}

#[test]
fn 旧配置没有_actions_字段时补默认值() {
    // `AiConfig` 是 `#[serde(default)]`：老 config.json 里没有这个字段，
    // 反序列化要自动补上内置 6 项，而不是变成空列表
    let json = r#"{"enabled":true}"#;
    let ai: crate::core::config::AiConfig =
        serde_json::from_str(json).expect("缺字段不该反序列化失败");
    assert_eq!(ai.actions.len(), 6);
}

#[test]
fn 停用的动作不参与生效列表() {
    let mut c = AppConfig::default();
    let id = c.ai.actions[0].id.clone();
    c.ai.actions[0].enabled = false;
    assert!(c.ai.action(&id).is_none());
    assert_eq!(c.ai.active_actions().count(), 5);
}

#[test]
fn 动作_重复_id_只留第一条() {
    let mut c = AppConfig::default();
    let mut dup = c.ai.actions[0].clone();
    dup.name = "重名".into();
    c.ai.actions.push(dup);
    c.sanitize();
    let first = c.ai.actions[0].id.clone();
    assert_eq!(
        c.ai.actions.iter().filter(|a| a.id == first).count(),
        1,
        "重复 id 会让按钮跑错动作"
    );
}

#[test]
fn 动作_空_id_与残缺条目被清掉() {
    let mut c = AppConfig::default();
    c.ai.actions.push(AiActionConfig::default()); // id 与名字都空
    c.sanitize();
    assert!(c.ai.actions.iter().all(|a| !a.id.is_empty()));
}

#[test]
fn 动作_名字空时用_id_顶上() {
    let mut c = AppConfig::default();
    c.ai.actions[0].name = "  ".into();
    c.sanitize();
    assert!(!c.ai.actions[0].name.trim().is_empty(), "按钮上不能没名字");
}
