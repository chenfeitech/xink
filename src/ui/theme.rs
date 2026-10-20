//! 把预置主题装成 gpui 主题（编辑区与预览区共用同一份）
//!
//! `core::theme` 只提供 JSON 文本与名称逻辑，不碰 gpui 类型；
//! 反序列化成 `HighlightTheme` 是这里的事。

use std::sync::{Arc, OnceLock};

use gpui_kit::component::highlighter::HighlightTheme;

use crate::core::theme::{self, PRESET_THEMES};

/// 一份已解析的预置主题
struct Preset {
    name: &'static str,
    dark: bool,
    theme: Arc<HighlightTheme>,
}

/// 解析结果缓存：预览区每次渲染都要取色，不能每次都反序列化一遍 JSON
static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();

fn presets() -> &'static Vec<Preset> {
    PRESETS.get_or_init(|| {
        let mut loaded = Vec::new();
        for name in PRESET_THEMES {
            for dark in [false, true] {
                let Some(json) = theme::preset_json(name, dark) else {
                    continue;
                };
                match serde_json::from_str::<HighlightTheme>(json) {
                    Ok(parsed) => loaded.push(Preset {
                        name,
                        dark,
                        theme: Arc::new(parsed),
                    }),
                    // 预置主题是我们自己提交的，解析失败只可能是打包错误。
                    // 这里在渲染路径上，不能 panic，跳过即可（调用方退回框架默认）。
                    Err(err) => eprintln!("预置主题 {name} 解析失败，已跳过: {err}"),
                }
            }
        }
        loaded
    })
}

/// 按主题名与明暗取主题
///
/// 未命中（主题名不存在 / JSON 损坏）返回 `None`，由调用方退回框架默认主题——
/// 手改配置写错主题名时应用不崩，只是回到默认配色。
pub fn highlight_theme(name: &str, dark: bool) -> Option<Arc<HighlightTheme>> {
    presets()
        .iter()
        .find(|preset| preset.name == name && preset.dark == dark)
        .map(|preset| preset.theme.clone())
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/theme.rs"]
mod tests;
