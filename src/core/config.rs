//! 应用配置：基本 / 编辑器 / 预览 三大类
//!
//! 设计要点：
//! - 每个字段都有默认值（`Default` + `#[serde(default)]`）：旧配置缺字段时自动补齐，
//!   新增配置项不会让老用户的配置文件失效。
//! - 载入后统一 `sanitize()`：把越界值夹回合法区间，避免异常数值破坏界面。
//! - 本模块只有数据 + 文件读写，不依赖 GPUI，便于单元测试。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

use crate::core::keymap;
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

/// 滚动同步的方式
///
/// 两种坐标都可以对齐两端，取舍不同：
///
/// - **比例**：两端各在自己的可滚动范围内取百分比。换算**连续**（滚动全程平滑），
///   且读写用同一个公式，天然对称，不会互推；代价是"同一百分比"在两端对应的
///   内容不完全一致（长代码块处会偏）——但不会跳，也不会来回抖。
/// - **块锚点**：把源码行映射到预览的顶层块，位置更准；但"块内行比例 → 块内
///   像素"是分段线性的，块边界处必然跳，且依赖虚拟列表已测量该块。
///
/// 默认取**比例**（与旧版观感一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollSyncMode {
    /// 比例跟随：滚动中连续同步（默认，平滑；长代码块处可能略有偏差）
    #[default]
    #[serde(alias = "realtime")]
    Ratio,
    /// 块锚点：按 Markdown 顶层块对齐（位置更准，块边界处会跳）
    Anchor,
    /// 停止后对齐：滚动中完全不动对端，停下约 0.25 秒后对齐一次（绝对不抖）
    OnStop,
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
    /// 软换行：长行折行显示，不出现横向滚动条
    ///
    /// 关掉后"源码行号 ↔ 滚动位置"是一次乘除就能算准的（每个逻辑行正好一个显示行），
    /// 编辑区与预览区的滚动同步也随之更精确；代价是长行要横向滚动。
    /// 见 `docs/preview/scroll-sync-design.md` §8 D1。
    pub soft_wrap: bool,
    pub font: FontConfig,
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
    /// 同步时机：实时跟随 / 停止后对齐
    pub sync_mode: ScrollSyncMode,
    /// 正文最大宽度（像素），0 表示不限制
    pub max_width: u32,
    /// 版面密度：留白与段间距的整体松紧
    pub density: PreviewDensity,
    /// 调试用：在预览面板标题显示两端当前的源码行号与偏差
    ///
    /// 把"滚动同步对不对得上"从肉眼感觉变成可读的数字，便于验收与回归。
    pub debug_scroll_sync: bool,
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
    /// AI 协助（默认关闭：未配置时不发起任何网络请求）
    pub ai: AiConfig,
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
            // 与框架默认一致（gpui-base 的 soft_wrap 默认 true）：改的是编辑习惯，
            // 默认不动；想要最精确的滚动同步可以在设置里关掉。
            soft_wrap: true,
            font: FontConfig::mono(),
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
            sync_mode: ScrollSyncMode::default(),
            max_width: 0,
            debug_scroll_sync: false,
        }
    }
}

// ── AI 配置 ─────────────────────────────────────────────────
//
// 设计见 `docs/ai/ai-assistant-design.md` §4.1 与
// `docs/ai/ollama-provider-design.md` §4.1。
// AI 配置从未发布过，历史 `config.json` 里没有 `ai` 段，
// 因此**没有迁移逻辑**——`#[serde(default)]` + `sanitize()` 已足够。

/// Provider 类型
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderKind {
    /// 本地 Ollama（走 native `/api/chat`）
    #[default]
    Ollama,
    /// OpenAI 兼容端点（云厂商 / Ollama 的 /v1）
    OpenAiCompatible,
}

/// API key 的来源
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// macOS Keychain（默认）
    #[default]
    Keychain,
    /// 环境变量 `XINK_AI_API_KEY`
    Env,
    /// 明文写在 config.json（用户显式选择才用）
    Plain,
}

/// 输出语言
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputLang {
    /// 跟随输入内容
    #[default]
    Auto,
    Zh,
    En,
}

/// 单模型的生成参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiModelParams {
    /// 上下文窗口（Ollama `options.num_ctx`），必须 ≤ 模型的 context_length
    pub num_ctx: u32,
    /// 最大生成 token（Ollama `num_predict` / OpenAI `max_tokens`）
    pub num_predict: u32,
    pub temperature: f32,
    /// 是否允许思考（Ollama native 专有；兼容端点忽略此参数）
    pub think: bool,
    /// 模型常驻时长（Ollama `keep_alive`），如 "5m"、"-1"、"0"
    pub keep_alive: String,
}

/// 一个可选模型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelEntry {
    /// Ollama 里是模型名（如 `qwen3.5:0.8b`）
    pub id: String,
    pub kind: AiProviderKind,
    /// 展示名，空则用 id
    pub label: String,
    /// 覆盖全局 base_url（多实例 / 远端 Ollama）
    pub base_url: String,
    pub api_key_source: KeySource,
    pub params: AiModelParams,
    /// 来自 `/api/tags` 的 context_length；None 时按默认 num_ctx 用
    pub context_limit: Option<u32>,
    /// 来自 `/api/tags`：`completion` / `tools` / `thinking` / `vision`
    pub capabilities: Vec<String>,
    /// 是否启用工具阶段（连接测试通过才置 true）
    pub tools_enabled: bool,
    /// 列表里停用
    pub enabled: bool,
    /// 上次连接测试时间（毫秒时间戳），0 表示没测过
    pub last_checked_ms: u64,
}

/// Ollama 连接参数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OllamaConfig {
    pub base_url: String,
    /// `keep_alive`：默认 5m；设 -1 常驻、0 用完即卸
    pub keep_alive: String,
    pub num_ctx: u32,
}

/// 一个快捷动作的范围：有选中时用选中，还是始终整篇
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiActionScope {
    /// 有选中就用选中，没有选中才用整篇
    #[default]
    Selection,
    /// 忽略选中，始终作用于整篇
    Whole,
}

/// 结果的落法：替换原文对应部分，还是插到光标处
///
/// 「生成摘要 / 生成标签」这类是**往里加内容**，判成替换会把原文其余部分
/// 删掉，所以它们默认走 `Insert`。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiActionApply {
    /// 替换（有选中替换选中，否则替换整篇）
    #[default]
    Replace,
    /// 插入到光标处，不动原文
    Insert,
}

/// 一个快捷动作
///
/// 内置动作不再是代码里的枚举，而是配置数据：可新增、可改提示词、可删除。
/// 提示词里的 `{{known_tags}}` 会被替换成库内已有的标签（没有就不注入），
/// 这样"生成标签"复用既有标签的能力不再依赖硬编码的动作 id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiActionConfig {
    /// 唯一标识。内置项沿用 `polish` / `condense` / …，自定义项用户自取
    pub id: String,
    /// 按钮与命令面板上显示的名字
    pub name: String,
    /// 发给模型的指令
    pub prompt: String,
    pub scope: AiActionScope,
    pub apply: AiActionApply,
    /// 停用后不出现在对话条与命令面板，但配置仍保留
    pub enabled: bool,
}

impl Default for AiActionConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            prompt: String::new(),
            scope: AiActionScope::default(),
            apply: AiActionApply::default(),
            enabled: true,
        }
    }
}

/// 内置动作的默认配置（6 项）
///
/// 提示词原文来自旧的 `prompt::Action::instruction()`；「扩写」「续写」
/// 按需求精简掉，**不再保留**——要用就自己新建一条并写提示词。
pub fn builtin_actions() -> Vec<AiActionConfig> {
    vec![
        AiActionConfig {
            id: "polish".into(),
            name: "润色".into(),
            prompt: "润色下面这段笔记：修正语病与不通顺的表达，保持原意、\
                     保持原有的 Markdown 结构，不要新增事实。"
                .into(),
            scope: AiActionScope::Selection,
            apply: AiActionApply::Replace,
            enabled: true,
        },
        AiActionConfig {
            id: "condense".into(),
            name: "精简".into(),
            prompt: "精简下面这段笔记：保留全部关键信息，压缩到原来的一半以内，\
                     保持 Markdown 结构。"
                .into(),
            scope: AiActionScope::Selection,
            apply: AiActionApply::Replace,
            enabled: true,
        },
        AiActionConfig {
            id: "translate".into(),
            name: "翻译".into(),
            prompt: "翻译下面这段笔记：保持 Markdown 结构与专有名词原样，\
                     只翻译自然语言部分。"
                .into(),
            scope: AiActionScope::Selection,
            apply: AiActionApply::Replace,
            enabled: true,
        },
        AiActionConfig {
            id: "retitle".into(),
            name: "改写标题".into(),
            prompt: "为下面这篇笔记拟一个标题：只输出一行标题文本，\
                     不要 Markdown 标记，不要解释。"
                .into(),
            scope: AiActionScope::Whole,
            apply: AiActionApply::Replace,
            enabled: true,
        },
        AiActionConfig {
            id: "summarize".into(),
            name: "生成摘要".into(),
            prompt: "为下面这篇笔记写一段摘要：3 句话以内，只输出摘要正文。".into(),
            scope: AiActionScope::Whole,
            apply: AiActionApply::Insert,
            enabled: true,
        },
        AiActionConfig {
            id: "tag".into(),
            name: "生成标签".into(),
            prompt: "为下面这篇笔记生成标签：只输出一行以逗号分隔的标签，3–5 个，\
                     不要 `#` 前缀，不要解释。\
                     优先从库里已有的标签中选择：{{known_tags}}；\
                     确实都不合适时才造新标签。"
                .into(),
            scope: AiActionScope::Whole,
            apply: AiActionApply::Insert,
            enabled: true,
        },
    ]
}

/// AI 配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// 总开关，关掉则所有 AI 入口不可用
    pub enabled: bool,
    /// 当前模型（对应 [`ModelEntry::id`]）
    pub default_model: String,
    pub models: Vec<ModelEntry>,
    pub ollama: OllamaConfig,
    /// 静默超时（秒）
    pub timeout_secs: u64,
    /// 发给模型的正文字符预算
    pub max_context_chars: usize,
    /// agent loop 的最大步数
    pub max_steps: usize,
    /// agent loop 的总开关：关掉就永远走单轮（与模型支不支持工具无关）
    ///
    /// 小模型上多步反而更慢更差，留一个能退回去的开关。
    pub agentic: bool,
    /// 单条工具结果回传的字符上限（防止一篇长笔记把上下文吃光）
    pub max_tool_chars: usize,
    pub output_language: OutputLang,
    /// 是否跳过 diff 直接落盘：默认 false，UI 不暴露
    pub auto_apply: bool,
    pub skills_enabled: bool,
    pub disabled_skills: Vec<String>,
    pub memory_enabled: bool,
    /// 快捷动作列表（可新增 / 改提示词 / 删除）
    pub actions: Vec<AiActionConfig>,
}

impl Default for AiModelParams {
    fn default() -> Self {
        Self {
            num_ctx: 8192,
            // 给足预算：thinking 模型会把 num_predict 用在思维链上
            num_predict: 1024,
            temperature: 0.7,
            think: false,
            keep_alive: "5m".into(),
        }
    }
}

impl Default for ModelEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: AiProviderKind::default(),
            label: String::new(),
            base_url: String::new(),
            api_key_source: KeySource::default(),
            params: AiModelParams::default(),
            context_limit: None,
            capabilities: Vec::new(),
            tools_enabled: false,
            enabled: true,
            last_checked_ms: 0,
        }
    }
}

impl ModelEntry {
    /// 由 `/api/tags` 的发现结果建一条（未测过工具能力）
    pub fn discovered(found: &crate::core::ai::models::DiscoveredModel) -> Self {
        Self {
            id: found.name.clone(),
            kind: AiProviderKind::Ollama,
            capabilities: found.capabilities.clone(),
            context_limit: found.context_length,
            tools_enabled: false,
            ..Self::default()
        }
    }

    /// 展示名：优先 label
    pub fn display_name(&self) -> &str {
        if self.label.trim().is_empty() {
            &self.id
        } else {
            &self.label
        }
    }

    /// 实际使用的 base_url：条目覆盖优先，否则用 provider 默认值
    pub fn base_url_or(&self, cfg: &AiConfig) -> String {
        if self.base_url.trim().is_empty() {
            match self.kind {
                AiProviderKind::Ollama => cfg.ollama.base_url.clone(),
                AiProviderKind::OpenAiCompatible => String::new(),
            }
        } else {
            self.base_url.clone()
        }
    }

    /// 是否声明支持工具调用（M2 工具阶段用它决定是否显示"启用工具"开关）
    #[allow(dead_code)]
    pub fn supports_tools(&self) -> bool {
        self.capabilities.iter().any(|c| c == "tools")
    }

    /// 是否声明支持思考（M2 起用来决定是否显示"思考"开关）
    #[allow(dead_code)]
    pub fn supports_thinking(&self) -> bool {
        self.capabilities.iter().any(|c| c == "thinking")
    }
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".into(),
            keep_alive: "5m".into(),
            num_ctx: 8192,
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_model: String::new(),
            models: Vec::new(),
            ollama: OllamaConfig::default(),
            timeout_secs: 60,
            max_context_chars: 12_000,
            max_steps: 8,
            agentic: true,
            max_tool_chars: 4000,
            output_language: OutputLang::default(),
            auto_apply: false,
            skills_enabled: true,
            disabled_skills: Vec::new(),
            memory_enabled: false,
            actions: builtin_actions(),
        }
    }
}

impl AiConfig {
    /// 当前生效的模型
    pub fn active_model(&self) -> Option<&ModelEntry> {
        if self.default_model.is_empty() {
            return None;
        }
        self.models
            .iter()
            .find(|m| m.id == self.default_model && m.enabled)
    }

    /// 可发起请求的前提：启用 + 有模型 + 模型有 endpoint
    pub fn is_ready(&self) -> bool {
        self.enabled && self.active_model().is_some()
    }

    /// 该模型的 endpoint（Ollama 走 native，兼容端点拼 `/chat/completions`）
    pub fn endpoint_for(&self, model: &ModelEntry) -> String {
        let base = model.base_url_or(self);
        match model.kind {
            AiProviderKind::Ollama => crate::core::ai::ollama::chat_endpoint(&base),
            AiProviderKind::OpenAiCompatible => crate::core::ai::openai::chat_endpoint(&base),
        }
    }

    /// 是否所有模型都是本地 Ollama（本地不需要 key）
    pub fn ollama_only_ignores_keys(&self) -> bool {
        matches!(
            self.active_model().map(|m| m.kind),
            Some(AiProviderKind::Ollama) | None
        )
    }

    pub fn default_key_source(&self) -> KeySource {
        self.active_model()
            .map(|m| m.api_key_source)
            .unwrap_or_default()
    }

    pub fn plain_key(&self) -> Option<&str> {
        // 明文 key 目前只有一处（当前模型），存放在模型条目外：见 `plain_key_raw`
        None
    }

    pub fn sanitize(&mut self) {
        self.timeout_secs = self
            .timeout_secs
            .clamp(limit::AI_TIMEOUT_SECS.0, limit::AI_TIMEOUT_SECS.1);
        self.max_context_chars = self
            .max_context_chars
            .clamp(limit::AI_CONTEXT_CHARS.0, limit::AI_CONTEXT_CHARS.1);
        self.max_steps = self
            .max_steps
            .clamp(limit::AI_MAX_STEPS.0, limit::AI_MAX_STEPS.1);
        self.max_tool_chars = self
            .max_tool_chars
            .clamp(limit::AI_TOOL_CHARS.0, limit::AI_TOOL_CHARS.1);
        self.auto_apply = false;
        // 空 id 的条目没有意义（多半是手改配置留下的）
        self.models.retain(|m| !m.id.trim().is_empty());
        for model in &mut self.models {
            model.params.num_predict = model
                .params
                .num_predict
                .clamp(limit::AI_NUM_PREDICT.0, limit::AI_NUM_PREDICT.1);
            model.params.temperature = clamp_f32(
                model.params.temperature,
                (limit::AI_TEMPERATURE.0, limit::AI_TEMPERATURE.1),
            );
            let max_ctx = model.context_limit.unwrap_or(limit::AI_NUM_CTX.1);
            model.params.num_ctx = model
                .params
                .num_ctx
                .clamp(limit::AI_NUM_CTX.0, max_ctx.max(limit::AI_NUM_CTX.0));
        }
        // 选中的模型被删掉或停用了：清空选择，避免"看着配好了却发不出去"
        if !self.default_model.is_empty() && self.active_model().is_none() {
            self.default_model.clear();
        }
        self.sanitize_actions();
    }

    /// 生效的快捷动作（停用 / 残缺的都不出现）
    pub fn active_actions(&self) -> impl Iterator<Item = &AiActionConfig> {
        self.actions
            .iter()
            .filter(|a| a.enabled && !a.id.is_empty())
    }

    /// 按 id 取一个启用的动作
    pub fn action(&self, id: &str) -> Option<&AiActionConfig> {
        self.active_actions().find(|a| a.id == id)
    }

    fn sanitize_actions(&mut self) {
        // id 空、或名字与提示词都空：多半是手改配置留下的残条
        self.actions.retain(|a| {
            !a.id.trim().is_empty() && !(a.name.trim().is_empty() && a.prompt.trim().is_empty())
        });
        // id 重复会让"点这个按钮跑成那个动作"，只留第一条
        let mut seen = HashSet::new();
        self.actions.retain(|a| seen.insert(a.id.clone()));
        for action in &mut self.actions {
            action.name = action
                .name
                .trim()
                .chars()
                .take(limit::AI_ACTION_NAME)
                .collect();
            action.prompt = action
                .prompt
                .trim()
                .chars()
                .take(limit::AI_ACTION_PROMPT)
                .collect();
            // 名字空就用 id 顶上，免得按钮上什么都不显示
            if action.name.is_empty() {
                action.name = action.id.clone();
            }
        }
        self.actions.truncate(limit::AI_ACTIONS);
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
    /// AI：静默超时（秒）；下限 5s 避免手滑设成 0 导致必然超时
    pub const AI_TIMEOUT_SECS: (u64, u64) = (5, 600);
    pub const AI_CONTEXT_CHARS: (usize, usize) = (1_000, 200_000);
    pub const AI_MAX_STEPS: (usize, usize) = (1, 64);
    /// 单条工具结果回传的字符数：太小模型看不到内容，太大吃掉上下文预算
    pub const AI_TOOL_CHARS: (usize, usize) = (500, 20_000);
    pub const AI_NUM_PREDICT: (u32, u32) = (16, 32_768);
    pub const AI_TEMPERATURE: (f32, f32) = (0.0, 2.0);
    pub const AI_NUM_CTX: (u32, u32) = (512, 1_048_576);
    /// 快捷动作：名字 / 提示词长度与条数上限
    pub const AI_ACTION_NAME: usize = 32;
    pub const AI_ACTION_PROMPT: usize = 2_000;
    pub const AI_ACTIONS: usize = 32;
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
        // 键位：空值 / 非法键串 / 未知动作名一律删除（等价于"用默认键位"），
        // 不留"存了但不生效"的键。见 `docs/config/keymap-wiring-design.md` §3.5
        keymap::sanitize(&mut self.editor.keymap);

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
        self.ai.sanitize();
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
