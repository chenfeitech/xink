// 只测不依赖 App 上下文的部分：预置主题 JSON 能被 gpui 反序列化成
// `HighlightTheme`。真实着色效果（编辑区是否跟着换）走人工验收。
use gpui_kit::component::highlighter::HighlightTheme;

use super::*;

#[test]
fn preset_themes_deserialize_into_highlight_theme() {
    // 反序列化失败时 `ui::theme` 只打印一行日志并跳过，界面会静默退回框架默认，
    // 所以必须在测试里挡住这种情况。
    for name in PRESET_THEMES {
        for dark in [false, true] {
            let json = theme::preset_json(name, dark)
                .unwrap_or_else(|| panic!("预置主题 {name}（dark={dark}）缺少 JSON 资源"));
            let parsed: HighlightTheme = serde_json::from_str(json)
                .unwrap_or_else(|err| panic!("预置主题 {name}（dark={dark}）反序列化失败: {err}"));
            assert!(
                parsed.style.syntax.style("keyword").is_some(),
                "预置主题 {name}（dark={dark}）没有语法配色"
            );
        }
    }
}

#[test]
fn highlight_theme_hits_every_preset() {
    for name in PRESET_THEMES {
        for dark in [false, true] {
            assert!(
                highlight_theme(name, dark).is_some(),
                "预置主题 {name}（dark={dark}）取不到 HighlightTheme"
            );
        }
    }
}

#[test]
fn unknown_theme_name_returns_none() {
    // 手改配置写错主题名：返回 None，由 `apply_theme` 保持框架默认，不覆盖、不崩
    assert!(highlight_theme("no-such-theme", true).is_none());
    assert!(highlight_theme("no-such-theme", false).is_none());
}
