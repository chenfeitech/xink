//! 应用配置：基本 / 编辑器 / 预览 三大类
//!
//! 设计要点：
//! - 每个字段都有默认值（`Default` + `#[serde(default)]`）：旧配置缺字段时自动补齐，
//!   新增配置项不会让老用户的配置文件失效。
//! - 载入后统一 `sanitize()`：把越界值夹回合法区间，避免异常数值破坏界面。
//! - 本模块只有数据 + 文件读写，不依赖 GPUI，便于单元测试。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::core::theme;
use crate::state::workspace::config_path;

// ── 枚举型配置项 ──────────────────────────────────────────────

/// 主题模式。System 跟随系统外观，Light/Dark 强制指定。
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// 切换明暗后应进入的模式。
    ///
    /// `is_dark` 是"跟随系统"解析后的当前结果：从 System 起手时，
    /// 第一下切到与当前外观相反的明确模式，之后在 Light / Dark 之间来回，
    /// 这样用户按一次就能脱离"跟随系统"，且不会出现"按了没变化"。
    pub fn toggled(self, is_dark: bool) -> ThemeMode {
        match self {
            ThemeMode::System if is_dark => ThemeMode::Light,
            ThemeMode::System => ThemeMode::Dark,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        }
    }
}

/// 界面语言
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Zh,
    En,
}

/// 字重
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// 编辑器配色方案
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    /// 跟随主题
    #[default]
    Auto,
    Light,
    Dark,
}

/// 预览区图片显示方式
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageDisplay {
    /// 限制在预览宽度内（框架渲染图片时的默认行为）
    #[default]
    FitWidth,
    /// 只显示链接，不渲染图片
    Hidden,
    /// 旧版本遗留值（如已移除的 `original`）。
    ///
    /// 保留它是为了兼容：旧 `config.json` 里若写了已移除的选项，
    /// 没有这个变体会导致**整个配置文件解析失败**并回落默认值（用户全部设置丢失）。
    /// `sanitize` 会把它夹回 `FitWidth`。
    #[serde(other)]
    Legacy,
}

/// 预览区链接打开行为
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkOpen {
    /// 在应用内打开（同库笔记跳转）
    #[default]
    InApp,
    /// 用系统默认浏览器打开
    External,
}

// ── 结构化配置项 ──────────────────────────────────────────────

/// 字体设置，目录树 / 编辑器 / 预览各自持有一份
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    /// 行高倍数（1.0 = 单倍行距）
    pub line_height: f32,
    pub weight: FontWeight,
}

/// 缩进规则
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IndentConfig {
    /// true 用 Tab 字符，false 用空格
    pub use_tabs: bool,
    /// 一个缩进层级占几个字符宽度
    pub tab_size: u32,
}

// ── 三大类配置 ────────────────────────────────────────────────

/// 基本配置：界面外观与目录树
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BasicConfig {
    pub theme: ThemeMode,
    pub language: Language,
    /// 图标边长（像素）
    pub icon_size: u32,
    /// 目录树字体
    pub tree_font: FontConfig,
    /// 目录树是否显示**所有**文件（含非笔记/图片的文件与点开头的隐藏项）
    pub show_all_files: bool,
}

/// 编辑器配置：行为与外观
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    pub auto_save: bool,
    /// 停止输入后多久自动保存（毫秒）
    pub auto_save_delay_ms: u64,
    pub indent: IndentConfig,
    pub show_line_numbers: bool,
    pub font: FontConfig,
    pub color_scheme: ColorScheme,
    /// 快捷键映射：动作名 → 按键串（如 "cmd-b"），留空则用内置默认
    pub keymap: BTreeMap<String, String>,
    /// JSON / YAML / TOML 等数据文件的编辑与预览
    pub data_file: DataFileConfig,
}

/// 数据文件（JSON / YAML / TOML）配置
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DataFileConfig {
    /// 预览格式化的缩进宽度（空格数）。
    ///
    /// 只对 JSON 生效：`noyalib` 与 `toml` 的序列化器不提供缩进配置。
    pub indent: usize,
    /// 超过该字节数就跳过格式化（预览直接显示原文），避免大文件卡顿；
    /// 0 表示不限制
    pub max_format_bytes: usize,
}

impl DataFileConfig {
    /// 把越界值夹回可用区间，避免手改配置文件后行为异常
    pub fn sanitize(&mut self) {
        self.indent = self
            .indent
            .clamp(limit::DATA_INDENT.0, limit::DATA_INDENT.1);
        // 0 表示"不限制"，是合法值，因此只设上限
        self.max_format_bytes = self.max_format_bytes.min(limit::MAX_FORMAT_BYTES.1);
    }
}

impl Default for DataFileConfig {
    fn default() -> Self {
        Self {
            indent: crate::core::datafile::DEFAULT_INDENT,
            max_format_bytes: crate::core::datafile::DEFAULT_MAX_FORMAT_BYTES,
        }
    }
}

/// 预览配置：渲染样式与排版
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    pub font: FontConfig,
    /// 旧版本的 `code_theme`（syntect 主题名）
    ///
    /// 仅用于反序列化时捕获旧配置并迁移一次（见 `sanitize_highlight`）；
    /// 用 `skip_serializing` 保证写回配置文件时不再出现。
    #[serde(default, skip_serializing)]
    pub legacy_code_theme: Option<String>,
    pub image_display: ImageDisplay,
    /// 图片显示宽度占预览区宽度的百分比（20–100）
    pub image_width_percent: u32,
    pub link_open: LinkOpen,
    /// 编辑器与预览双向同步滚动
    pub sync_scroll: bool,
    /// 正文最大宽度（像素），0 表示不限制
    pub max_width: u32,
    /// 版面密度：留白与段间距的整体松紧
    pub density: PreviewDensity,
}

/// 预览版面密度
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewDensity {
    /// 紧凑：留白小、段间距小，一屏能看更多
    Compact,
    /// 适中（默认）
    #[default]
    Normal,
    /// 宽松：留白大、段间距大，长时间阅读更舒服
    Relaxed,
}

/// 由密度决定的版面数值（纯数据，便于单测）
///
/// 只管**版面松紧**；字体与行高仍由 `PreviewConfig.font` 控制，
/// 避免两套设置互相覆盖。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewLayout {
    /// 左右内边距（px）
    pub pad_x: f32,
    /// 上下内边距（px）
    pub pad_y: f32,
    /// 段间距（rem）
    pub paragraph_gap: f32,
    /// 标题基准字号相对正文字号的倍数
    pub heading_scale: f32,
    /// 正文最大宽度（px）；用户在 `max_width` 里显式设置时优先用用户的
    pub max_width: f32,
}

impl PreviewDensity {
    /// 该档位的版面参数
    pub fn layout(self) -> PreviewLayout {
        match self {
            PreviewDensity::Compact => PreviewLayout {
                pad_x: 16.,
                pad_y: 8.,
                paragraph_gap: 0.55,
                heading_scale: 1.45,
                max_width: 960.,
            },
            PreviewDensity::Normal => PreviewLayout {
                pad_x: 24.,
                pad_y: 16.,
                paragraph_gap: 0.90,
                heading_scale: 1.70,
                max_width: 780.,
            },
            PreviewDensity::Relaxed => PreviewLayout {
                pad_x: 32.,
                pad_y: 24.,
                paragraph_gap: 1.30,
                heading_scale: 1.90,
                max_width: 680.,
            },
        }
    }

    /// 设置界面 / 菜单里的中文名
    pub fn label(self) -> &'static str {
        match self {
            PreviewDensity::Compact => "紧凑",
            PreviewDensity::Normal => "适中",
            PreviewDensity::Relaxed => "宽松",
        }
    }

    /// 从设置里的取值还原（未知值回落到适中）
    pub fn from_value(v: &str) -> Self {
        match v {
            "compact" => PreviewDensity::Compact,
            "relaxed" => PreviewDensity::Relaxed,
            _ => PreviewDensity::Normal,
        }
    }

    /// 设置里的取值
    pub fn value(self) -> String {
        match self {
            PreviewDensity::Compact => "compact".into(),
            PreviewDensity::Normal => "normal".into(),
            PreviewDensity::Relaxed => "relaxed".into(),
        }
    }
}

/// 高亮配置：编辑区与预览区**共用**
///
/// 以前两端各有一套配色：编辑区吃框架内嵌主题、预览区吃 `preview.code_theme`
/// 指名的 syntect 主题，改一边另一边不动。现在统一从这里取，见
/// `docs/editor/unified-highlight-config-design.md`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightConfig {
    /// 预置主题名（见 [`crate::core::theme::PRESET_THEMES`]）
    ///
    /// 明暗不由这里决定：每个预置主题自带 dark / light 两个变体，
    /// 切换界面明暗时自动取对应变体。
    pub theme: String,
}

impl Default for HighlightConfig {
    fn default() -> Self {
        Self {
            theme: theme::DEFAULT_THEME.into(),
        }
    }
}

/// 完整应用配置
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub basic: BasicConfig,
    pub editor: EditorConfig,
    pub preview: PreviewConfig,
    pub highlight: HighlightConfig,
}

// ── 默认值 ────────────────────────────────────────────────────

impl FontWeight {
    /// 转成 gpui 使用的字重数值
    pub fn value(self) -> f32 {
        match self {
            FontWeight::Regular => 400.0,
            FontWeight::Medium => 500.0,
            FontWeight::Semibold => 600.0,
            FontWeight::Bold => 700.0,
        }
    }
}

impl FontConfig {
    /// 默认正文字体
    pub fn body() -> Self {
        Self {
            family: ".SystemUIFont".into(),
            size: 14.0,
            line_height: 1.5,
            weight: FontWeight::Regular,
        }
    }

    /// 默认等宽字体，用于编辑器
    pub fn mono() -> Self {
        Self {
            family: ".SystemMonospacedFont".into(),
            size: 14.0,
            line_height: 1.6,
            weight: FontWeight::Regular,
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self::body()
    }
}

impl Default for IndentConfig {
    fn default() -> Self {
        Self {
            use_tabs: false,
            tab_size: 4,
        }
    }
}

impl Default for BasicConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::default(),
            language: Language::default(),
            icon_size: 16,
            tree_font: FontConfig::body(),
            // 默认显示全部：其他类型的文件也常需要在树里查看或引用（噪音目录仍始终跳过）
            show_all_files: true,
        }
    }
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            auto_save: true,
            // 与 REQUIREMENTS.md R2.2 一致：停止输入约 300ms 后写入
            auto_save_delay_ms: 300,
            indent: IndentConfig::default(),
            show_line_numbers: false,
            font: FontConfig::mono(),
            color_scheme: ColorScheme::default(),
            keymap: BTreeMap::new(),
            data_file: DataFileConfig::default(),
        }
    }
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            font: FontConfig::body(),
            legacy_code_theme: None,
            density: PreviewDensity::default(),
            image_display: ImageDisplay::default(),
            // 100% = 占满预览区宽度（与框架默认行为一致）
            image_width_percent: 100,
            link_open: LinkOpen::default(),
            sync_scroll: true,
            max_width: 0,
        }
    }
}

// ── 合法区间校正 ──────────────────────────────────────────────

/// 各项配置的合法区间，越界值会被 `sanitize` 夹回区间内
mod limit {
    pub const FONT_SIZE: (f32, f32) = (8.0, 72.0);
    pub const LINE_HEIGHT: (f32, f32) = (0.8, 3.0);
    pub const ICON_SIZE: (u32, u32) = (12, 48);
    pub const SAVE_DELAY_MS: (u64, u64) = (200, 60_000);
    pub const TAB_SIZE: (u32, u32) = (1, 16);
    pub const MAX_WIDTH: (u32, u32) = (0, 4_096);
    pub const IMAGE_WIDTH_PERCENT: (u32, u32) = (20, 100);
    /// 数据文件预览格式化：缩进 1–8 空格；体积上限 0（不限制）–20MB
    pub const DATA_INDENT: (usize, usize) = (1, 8);
    pub const MAX_FORMAT_BYTES: (usize, usize) = (0, 20_000_000);
}

fn clamp_f32(v: f32, (min, max): (f32, f32)) -> f32 {
    if v.is_nan() { min } else { v.clamp(min, max) }
}

impl FontConfig {
    fn sanitize(&mut self) {
        self.size = clamp_f32(self.size, limit::FONT_SIZE);
        self.line_height = clamp_f32(self.line_height, limit::LINE_HEIGHT);
        if self.family.trim().is_empty() {
            self.family = FontConfig::body().family;
        }
    }
}

impl AppConfig {
    /// 把越界 / 空值夹回合法区间。载入与导入后都会调用。
    pub fn sanitize(&mut self) {
        self.basic.icon_size = self
            .basic
            .icon_size
            .clamp(limit::ICON_SIZE.0, limit::ICON_SIZE.1);
        self.basic.tree_font.sanitize();

        self.editor.auto_save_delay_ms = self
            .editor
            .auto_save_delay_ms
            .clamp(limit::SAVE_DELAY_MS.0, limit::SAVE_DELAY_MS.1);
        self.editor.indent.tab_size = self
            .editor
            .indent
            .tab_size
            .clamp(limit::TAB_SIZE.0, limit::TAB_SIZE.1);
        self.editor.font.sanitize();
        self.editor.data_file.sanitize();

        self.preview.font.sanitize();
        self.preview.max_width = self
            .preview
            .max_width
            .clamp(limit::MAX_WIDTH.0, limit::MAX_WIDTH.1);
        self.preview.image_width_percent = self
            .preview
            .image_width_percent
            .clamp(limit::IMAGE_WIDTH_PERCENT.0, limit::IMAGE_WIDTH_PERCENT.1);
        // 旧版本遗留的显示方式（如已移除的"原始尺寸"）夹回适应宽度
        if matches!(self.preview.image_display, ImageDisplay::Legacy) {
            self.preview.image_display = ImageDisplay::FitWidth;
        }
        self.sanitize_highlight();
    }

    /// 高亮主题的规范化与旧配置迁移
    ///
    /// 迁移只跑一次：把旧的 `preview.code_theme`（syntect 主题名）换成预置主题后，
    /// 就把旧字段复位成默认值。否则用户改了新的 `highlight.theme`，
    /// 下次启动又会被旧值覆盖回去。
    fn sanitize_highlight(&mut self) {
        // 迁移只跑一次：旧字段被 `take()` 掉后写回配置时不会再出现，
        // 否则用户改了新的 `highlight.theme`，下次启动又被旧值覆盖回去。
        if let Some(legacy) = self.preview.legacy_code_theme.take() {
            self.highlight.theme = theme::migrate_code_theme(&legacy).into();
        }
        // 手改配置写了不存在的主题名时退回默认，避免两端整片不着色
        self.highlight.theme = theme::normalize_theme_name(&self.highlight.theme).into();
    }

    // ── 持久化 ────────────────────────────────────────────────

    /// 读取用户配置；文件不存在或损坏时回落到默认值（不会让应用起不来）
    pub fn load() -> Self {
        let mut config: Self = fs::read_to_string(config_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        config.sanitize();
        config
    }

    /// 写入用户配置
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json).with_context(|| format!("写入配置失败: {}", path.display()))
    }

    // ── 导入 / 导出 ──────────────────────────────────────────

    /// 导出到指定路径（备份 / 迁移用）
    pub fn export_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).with_context(|| format!("导出配置失败: {}", path.display()))
    }

    /// 从指定路径导入；解析失败会返回错误，不改动当前配置
    pub fn import_from(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        let mut config: Self = serde_json::from_str(&text)
            .with_context(|| format!("解析配置失败: {}", path.display()))?;
        config.sanitize();
        Ok(config)
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/config.rs"]
mod tests;
