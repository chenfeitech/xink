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

use crate::core::highlight::default_theme;
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
    /// 限制在预览宽度内
    #[default]
    FitWidth,
    /// 按原始尺寸显示（可横向滚动）
    Original,
    /// 只显示链接，不渲染图片
    Hidden,
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
    pub spell_check: bool,
    pub font: FontConfig,
    pub color_scheme: ColorScheme,
    /// 快捷键映射：动作名 → 按键串（如 "cmd-b"），留空则用内置默认
    pub keymap: BTreeMap<String, String>,
}

/// 预览配置：渲染样式与排版
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    pub font: FontConfig,
    /// 代码块高亮主题（syntect 主题名）
    pub code_theme: String,
    pub image_display: ImageDisplay,
    pub link_open: LinkOpen,
    /// 编辑器与预览双向同步滚动
    pub sync_scroll: bool,
    /// 正文最大宽度（像素），0 表示不限制
    pub max_width: u32,
}

/// 完整应用配置
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub basic: BasicConfig,
    pub editor: EditorConfig,
    pub preview: PreviewConfig,
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
            spell_check: false,
            font: FontConfig::mono(),
            color_scheme: ColorScheme::default(),
            keymap: BTreeMap::new(),
        }
    }
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            font: FontConfig::body(),
            code_theme: default_theme(true).into(),
            image_display: ImageDisplay::default(),
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

        self.preview.font.sanitize();
        self.preview.max_width = self
            .preview
            .max_width
            .clamp(limit::MAX_WIDTH.0, limit::MAX_WIDTH.1);
        if self.preview.code_theme.trim().is_empty() {
            self.preview.code_theme = PreviewConfig::default().code_theme;
        }
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

#[cfg(test)]
mod tests;
