//! 高亮主题的元数据与预置资源（纯数据层）
//!
//! 编辑区（tree-sitter）与预览区（syntect）共用同一批预置主题，改一次两端都变。
//! 统一设计见 `docs/editor/unified-highlight-config-design.md`。
//!
//! 本模块**只提供 JSON 文本与名称逻辑**，不碰 gpui 的类型——`core` 层的约定是
//! 不引用样式 API（见 `core/mod.rs`）。把 JSON 装成 `HighlightTheme` 是
//! `ui::theme` 的事。
//!
//! 主题为什么是 JSON 而不是 Rust 常量：gpui-component 的 `ThemeStyle` 字段是
//! 私有的（`color` / `font_style` / `font_weight` 都没有 `pub`），从代码构造不出来，
//! 只能 serde 反序列化。

/// 默认预置主题名
pub const DEFAULT_THEME: &str = "xink";

/// 可用的预置主题名（设置界面下拉的选项）
///
/// - `xink`：取自框架 `default-theme.json` 的 syntax 段，观感与统一前的编辑区一致
/// - `ocean` / `solarized`：由 syntect 内置主题经同一张 `SCOPE_MAP` 转换而来，
///   因此"预览区看到的颜色"与"主题里写的颜色"天然一致
pub const PRESET_THEMES: [&str; 3] = ["xink", "ocean", "solarized"];

/// 预置主题的原始 JSON；主题不存在返回 `None`
pub fn preset_json(name: &str, dark: bool) -> Option<&'static str> {
    match (name, dark) {
        ("xink", true) => Some(include_str!("themes/xink-dark.json")),
        ("xink", false) => Some(include_str!("themes/xink-light.json")),
        ("ocean", true) => Some(include_str!("themes/ocean-dark.json")),
        ("ocean", false) => Some(include_str!("themes/ocean-light.json")),
        ("solarized", true) => Some(include_str!("themes/solarized-dark.json")),
        ("solarized", false) => Some(include_str!("themes/solarized-light.json")),
        _ => None,
    }
}

/// 规范化主题名：未知名退回默认
///
/// 配置是手改得动的，写错时要有确定行为，而不是"编辑区突然不着色"。
pub fn normalize_theme_name(name: &str) -> &'static str {
    PRESET_THEMES
        .iter()
        .copied()
        .find(|preset| *preset == name)
        .unwrap_or(DEFAULT_THEME)
}

/// 旧的 `preview.code_theme`（syntect 主题名）→ 预置主题名
///
/// 两套主题体系无法一一对应：syntect 是 TextMate scope 体系，这里是语义化语法名。
/// 只把有对应预置主题的旧值映射过去（`base16-ocean.*` 与 `Solarized` 都是
/// 从同名 syntect 主题转换来的），其余退回默认。
pub fn migrate_code_theme(code_theme: &str) -> &'static str {
    match code_theme.trim() {
        other if other.starts_with("base16-ocean") => "ocean",
        other if other.starts_with("Solarized") => "solarized",
        _ => DEFAULT_THEME,
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/theme.rs"]
mod tests;
