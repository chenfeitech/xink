//! 设置界面（独立窗口）
//!
//! 设计要点（详见 `docs/settings-ui-design.md`）：
//! - **元数据驱动**：所有设置项登记在 [`all_fields`] 描述表里，UI 只负责渲染这张表。
//!   新增配置项 = 数据模型加字段 + 表里加一行，无需改动界面骨架。
//! - 三级结构：分类 → 分组 → 设置项，支持搜索定位。
//! - 单项重置（值 ≠ 默认时出现 ⟲）、依赖禁用、未接入渲染项标注"暂未生效"。
//! - 改动经 [`crate::app::update_config`] 立即生效并落盘。

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use gpui_kit::component::Root;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::*;

use crate::core::config::{
    AppConfig, ColorScheme, FontWeight, ImageDisplay, Language, LinkOpen, PreviewDensity, ThemeMode,
};
use crate::core::theme::PRESET_THEMES;

// ── 分类 ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Basic,
    Editor,
    Preview,
}

impl SettingsTab {
    const ALL: [SettingsTab; 3] = [Self::Basic, Self::Editor, Self::Preview];

    fn title(self) -> &'static str {
        match self {
            SettingsTab::Basic => "基本",
            SettingsTab::Editor => "编辑器",
            SettingsTab::Preview => "预览",
        }
    }
}

// ── 设置项描述表 ──────────────────────────────────────────────

/// 控件类型
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    Choice,
    Number,
    Font,
}

/// 控件构造函数。
///
/// 刻意**不接收 `Context`**：`Context<'_, V>` 带生命周期，闭包无法满足其
/// 高阶生命周期约束（HRTB）。控件内部需要的 `cx` 由各自的事件回调自行取得。
type ControlFn = Rc<dyn Fn(&mut SettingsView, &AppConfig) -> AnyElement>;

// 下面这组回调统一抽成别名，避免 `Rc<dyn Fn(…)>` 在字段上层层嵌套。

/// 返回 Some(原因) 时该项禁用
type DisabledFn = Rc<dyn Fn(&AppConfig) -> Option<&'static str>>;
/// 当前值是否等于默认值（决定是否显示 ⟲）
type IsDefaultFn = Rc<dyn Fn(&AppConfig) -> bool>;
/// 恢复该项默认值
type ResetFn = Rc<dyn Fn(&mut AppConfig)>;
/// 数值型取值（非数值项恒为 0），供统一渲染与输入框同步使用
type ValueFn = Rc<dyn Fn(&AppConfig) -> f32>;
/// 数值型写入（非数值项为空操作）
type SetValueFn = Rc<dyn Fn(&mut AppConfig, f32)>;

/// 一项设置的完整描述
pub struct SettingField {
    pub id: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    pub tab: SettingsTab,
    pub group: &'static str,
    pub kind: FieldKind,
    /// Some(原因)：该项已可配置但尚未接入渲染
    pub unavailable: Option<&'static str>,
    pub disabled: DisabledFn,
    pub is_default: IsDefaultFn,
    pub reset: ResetFn,
    pub value: ValueFn,
    pub set_value: SetValueFn,
    pub control: ControlFn,
}

// ── 描述表的构造器（消除逐项重复代码） ──────────────────────────
//
// 这三个构造器的参数个数（8 / 10 / 11）超过了 clippy 默认的 7 个。
// 这是元数据表的固有形态：**一行 = 一项设置的全部元信息**
// （id / 标题 / 说明 / 分类 / 分组 / 类型 / 取值 / 写入 …），
// 刻意保持"一项一行"的平铺写法——`all_fields()` 里几十项要能上下比对差异，
// 改成 builder 链式调用会把一行拆成七八行，可读性反而变差。
// 因此这里显式放行，而不是为了消告警去重构表结构。
#[allow(clippy::too_many_arguments)]
fn bool_field(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    tab: SettingsTab,
    group: &'static str,
    unavailable: Option<&'static str>,
    get: fn(&AppConfig) -> bool,
    set: fn(&mut AppConfig, bool),
) -> SettingField {
    let is_default = Rc::new(move |c: &AppConfig| get(c) == get(&AppConfig::default()));
    let reset = Rc::new(move |c: &mut AppConfig| set(c, get(&AppConfig::default())));
    let control = Rc::new(move |_: &mut SettingsView, c: &AppConfig| {
        Switch::new(id)
            .checked(get(c))
            .on_click(move |v, _, cx| {
                let v = *v;
                crate::app::update_config(cx, move |cfg| set(cfg, v));
            })
            .into_any_element()
    });
    SettingField {
        id,
        title,
        desc,
        tab,
        group,
        kind: FieldKind::Bool,
        unavailable,
        disabled: Rc::new(|_| None),
        is_default,
        reset,
        value: Rc::new(|_| 0.0),
        set_value: Rc::new(|_, _| {}),
        control,
    }
}

// 同上：一行 = 一项设置，参数多是元数据表的固有形态
#[allow(clippy::too_many_arguments)]
fn choice_field(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    tab: SettingsTab,
    group: &'static str,
    unavailable: Option<&'static str>,
    kind: FieldKind,
    options: fn() -> Vec<(String, String)>,
    get: fn(&AppConfig) -> String,
    set: fn(&mut AppConfig, String),
) -> SettingField {
    let is_default = Rc::new(move |c: &AppConfig| get(c) == get(&AppConfig::default()));
    let reset = Rc::new(move |c: &mut AppConfig| set(c, get(&AppConfig::default())));
    let control = Rc::new(move |_: &mut SettingsView, c: &AppConfig| {
        let current = get(c);
        let items = options();
        if items.len() <= 4 {
            segmented(id, &items, &current, set)
        } else {
            dropdown(id, &items, &current, set)
        }
    });
    SettingField {
        id,
        title,
        desc,
        tab,
        group,
        kind,
        unavailable,
        disabled: Rc::new(|_| None),
        is_default,
        reset,
        value: Rc::new(|_| 0.0),
        set_value: Rc::new(|_, _| {}),
        control,
    }
}

// 同上：一行 = 一项设置，参数多是元数据表的固有形态
#[allow(clippy::too_many_arguments)]
fn number_field(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    tab: SettingsTab,
    group: &'static str,
    min: f32,
    max: f32,
    step: f32,
    unit: &'static str,
    get: fn(&AppConfig) -> f32,
    set: fn(&mut AppConfig, f32),
) -> SettingField {
    let is_default = Rc::new(move |c: &AppConfig| get(c) == get(&AppConfig::default()));
    let reset = Rc::new(move |c: &mut AppConfig| set(c, get(&AppConfig::default())));
    let control = Rc::new(move |view: &mut SettingsView, c: &AppConfig| {
        number_control(view, id, get(c), min, max, step, unit, set)
    });
    SettingField {
        id,
        title,
        desc,
        tab,
        group,
        kind: FieldKind::Number,
        unavailable: None,
        disabled: Rc::new(|_| None),
        is_default,
        reset,
        value: Rc::new(get),
        set_value: Rc::new(set),
        control,
    }
}

/// 在 [`bool_field`] / [`choice_field`] 之上追加"依赖禁用"与"未生效"标记
fn with_meta(
    mut field: SettingField,
    unavailable: Option<&'static str>,
    disabled: Option<fn(&AppConfig) -> Option<&'static str>>,
) -> SettingField {
    field.unavailable = unavailable;
    if let Some(f) = disabled {
        field.disabled = Rc::new(f);
    }
    field
}

// ── 设置项清单 ────────────────────────────────────────────────

fn all_fields() -> Vec<SettingField> {
    use SettingsTab::{Basic, Editor, Preview};

    vec![
        // ── 基本 · 外观 ──
        choice_field(
            "basic.theme",
            "主题模式",
            "界面外观：跟随系统 / 浅色 / 深色",
            Basic,
            "外观",
            None,
            FieldKind::Choice,
            || {
                vec![
                    ("system".into(), "跟随系统".into()),
                    ("light".into(), "浅色".into()),
                    ("dark".into(), "深色".into()),
                ]
            },
            |c| match c.basic.theme {
                ThemeMode::System => "system".into(),
                ThemeMode::Light => "light".into(),
                ThemeMode::Dark => "dark".into(),
            },
            |c, v| {
                c.basic.theme = match v.as_str() {
                    "light" => ThemeMode::Light,
                    "dark" => ThemeMode::Dark,
                    _ => ThemeMode::System,
                }
            },
        ),
        with_meta(
            choice_field(
                "basic.language",
                "界面语言",
                "界面文案语言（界面文案尚未本地化）",
                Basic,
                "外观",
                None,
                FieldKind::Choice,
                || {
                    vec![
                        ("zh".into(), "中文".into()),
                        ("en".into(), "English".into()),
                    ]
                },
                |c| match c.basic.language {
                    Language::Zh => "zh".into(),
                    Language::En => "en".into(),
                },
                |c, v| {
                    c.basic.language = if v == "en" {
                        Language::En
                    } else {
                        Language::Zh
                    }
                },
            ),
            Some("界面文案尚未本地化"),
            None,
        ),
        choice_field(
            "highlight.theme",
            "高亮主题",
            "编辑区与预览区共用的语法配色，切换后两端同时生效",
            Basic,
            "外观",
            None,
            FieldKind::Choice,
            || {
                PRESET_THEMES
                    .iter()
                    .map(|name| ((*name).to_string(), (*name).to_string()))
                    .collect()
            },
            |c| c.highlight.theme.clone(),
            |c, v| c.highlight.theme = v,
        ),
        // ── 基本 · 目录树 ──
        number_field(
            "basic.icon_size",
            "图标大小",
            "目录树与工具栏图标边长，范围 12–48 px",
            Basic,
            "目录树",
            12.0,
            48.0,
            2.0,
            "px",
            |c| c.basic.icon_size as f32,
            |c, v| c.basic.icon_size = v as u32,
        ),
        choice_field(
            "basic.tree_font.family",
            "目录树字体",
            "左侧文件列表的字体",
            Basic,
            "目录树",
            None,
            FieldKind::Font,
            font_options,
            |c| c.basic.tree_font.family.clone(),
            |c, v| c.basic.tree_font.family = v,
        ),
        number_field(
            "basic.tree_font.size",
            "目录树字号",
            "文件列表字号，范围 8–72 px",
            Basic,
            "目录树",
            8.0,
            72.0,
            1.0,
            "px",
            |c| c.basic.tree_font.size,
            |c, v| c.basic.tree_font.size = v,
        ),
        number_field(
            "basic.tree_font.line_height",
            "目录树行高",
            "行高倍数，1.5 表示 1.5 倍行距",
            Basic,
            "目录树",
            0.8,
            3.0,
            0.1,
            "倍",
            |c| c.basic.tree_font.line_height,
            |c, v| c.basic.tree_font.line_height = v,
        ),
        choice_field(
            "basic.tree_font.weight",
            "目录树字重",
            "文件名的粗细",
            Basic,
            "目录树",
            None,
            FieldKind::Choice,
            || {
                vec![
                    ("regular".into(), "常规".into()),
                    ("medium".into(), "中等".into()),
                    ("semibold".into(), "半粗".into()),
                    ("bold".into(), "粗体".into()),
                ]
            },
            |c| match c.basic.tree_font.weight {
                FontWeight::Regular => "regular".into(),
                FontWeight::Medium => "medium".into(),
                FontWeight::Semibold => "semibold".into(),
                FontWeight::Bold => "bold".into(),
            },
            |c, v| {
                c.basic.tree_font.weight = match v.as_str() {
                    "medium" => FontWeight::Medium,
                    "semibold" => FontWeight::Semibold,
                    "bold" => FontWeight::Bold,
                    _ => FontWeight::Regular,
                }
            },
        ),
        bool_field(
            "basic.show_all_files",
            "显示所有文件",
            "目录树显示非笔记/图片的文件，以及点开头的隐藏项（`.git`、`node_modules`、`target` 等始终不显示）",
            Basic,
            "目录树",
            None,
            |c| c.basic.show_all_files,
            |c, v| c.basic.show_all_files = v,
        ),
        // ── 编辑器 · 编辑行为 ──
        bool_field(
            "editor.auto_save",
            "自动保存",
            "停止输入后自动写入磁盘；关闭后仍可用 Cmd-S 手动保存",
            Editor,
            "编辑行为",
            None,
            |c| c.editor.auto_save,
            |c, v| c.editor.auto_save = v,
        ),
        with_meta(
            number_field(
                "editor.auto_save_delay_ms",
                "自动保存延迟",
                "停止输入后多久保存，范围 200–60000 ms",
                Editor,
                "编辑行为",
                200.0,
                60_000.0,
                100.0,
                "ms",
                |c| c.editor.auto_save_delay_ms as f32,
                |c, v| c.editor.auto_save_delay_ms = v as u64,
            ),
            None,
            Some(|c| {
                if c.editor.auto_save {
                    None
                } else {
                    Some("需先开启自动保存")
                }
            }),
        ),
        with_meta(
            bool_field(
                "editor.indent.use_tabs",
                "使用 Tab 缩进",
                "关闭则用空格缩进",
                Editor,
                "编辑行为",
                None,
                |c| c.editor.indent.use_tabs,
                |c, v| c.editor.indent.use_tabs = v,
            ),
            // 已接线：set_tab_size 属 impl<M: MultiLineMode>，能力可用
            None,
            None,
        ),
        with_meta(
            number_field(
                "editor.indent.tab_size",
                "缩进宽度",
                "一个缩进层级占几个字符，范围 1–16",
                Editor,
                "编辑行为",
                1.0,
                16.0,
                1.0,
                "字符",
                |c| c.editor.indent.tab_size as f32,
                |c, v| c.editor.indent.tab_size = v as u32,
            ),
            // 已接线：set_tab_size 属 impl<M: MultiLineMode>，能力可用
            None,
            None,
        ),
        with_meta(
            bool_field(
                "editor.show_line_numbers",
                "显示行号",
                "在编辑区左侧显示行号（编辑区基于代码编辑器控件，行号始终可用）",
                Editor,
                "编辑行为",
                None,
                |c| c.editor.show_line_numbers,
                |c, v| c.editor.show_line_numbers = v,
            ),
            // 行号已随编辑内核升级接线生效，能力始终可用，因此不显示"暂未生效"：
            // - 开关打开 → 编辑区显示行号；
            // - 开关关闭 → 用户主动关闭，是选择而非能力缺失，同样不提示。
            // 仅当能力确实不可用时才应给出该标记（例如编辑内核退回非代码编辑器控件）。
            None,
            None,
        ),
        // ── 编辑器 · 字体与排版 ──
        choice_field(
            "editor.font.family",
            "编辑器字体",
            "正文编辑区字体，建议等宽",
            Editor,
            "字体与排版",
            None,
            FieldKind::Font,
            font_options,
            |c| c.editor.font.family.clone(),
            |c, v| c.editor.font.family = v,
        ),
        number_field(
            "editor.font.size",
            "编辑器字号",
            "正文编辑区字号，范围 8–72 px",
            Editor,
            "字体与排版",
            8.0,
            72.0,
            1.0,
            "px",
            |c| c.editor.font.size,
            |c, v| c.editor.font.size = v,
        ),
        number_field(
            "editor.font.line_height",
            "编辑器行高",
            "行高倍数",
            Editor,
            "字体与排版",
            0.8,
            3.0,
            0.1,
            "倍",
            |c| c.editor.font.line_height,
            |c, v| c.editor.font.line_height = v,
        ),
        choice_field(
            "editor.font.weight",
            "编辑器字重",
            "正文编辑区字体的粗细",
            Editor,
            "字体与排版",
            None,
            FieldKind::Choice,
            font_weight_options,
            |c| font_weight_value(c.editor.font.weight),
            |c, v| c.editor.font.weight = font_weight_parse(&v),
        ),
        with_meta(
            choice_field(
                "editor.color_scheme",
                "编辑器配色",
                "编辑区明暗配色",
                Editor,
                "字体与排版",
                None,
                FieldKind::Choice,
                || {
                    vec![
                        ("auto".into(), "跟随主题".into()),
                        ("light".into(), "浅色".into()),
                        ("dark".into(), "深色".into()),
                    ]
                },
                |c| match c.editor.color_scheme {
                    ColorScheme::Auto => "auto".into(),
                    ColorScheme::Light => "light".into(),
                    ColorScheme::Dark => "dark".into(),
                },
                |c, v| {
                    c.editor.color_scheme = match v.as_str() {
                        "light" => ColorScheme::Light,
                        "dark" => ColorScheme::Dark,
                        _ => ColorScheme::Auto,
                    }
                },
            ),
            Some("编辑区配色跟随主题，暂未独立适配"),
            None,
        ),
        // ── 编辑器 · 数据文件 ──
        number_field(
            "editor.data_file.indent",
            "JSON 缩进宽度",
            "预览里 JSON 的缩进空格数，范围 1–8（YAML / TOML 序列化器不提供该配置）",
            Editor,
            "数据文件",
            1.0,
            8.0,
            1.0,
            "空格",
            |c| c.editor.data_file.indent as f32,
            |c, v| c.editor.data_file.indent = v as usize,
        ),
        number_field(
            "editor.data_file.max_format_bytes",
            "格式化体积上限",
            "超过该字节数就不再排版、直接显示原文；0 表示不限制（按 1MB 步进）",
            Editor,
            "数据文件",
            0.0,
            20_000_000.0,
            1_000_000.0,
            "字节",
            |c| c.editor.data_file.max_format_bytes as f32,
            |c, v| c.editor.data_file.max_format_bytes = v as usize,
        ),
        // ── 预览 · 渲染 ──
        // 旧设置项 `preview.code_theme` 已从界面移除：语法配色统一由
        // `highlight.theme`（基本 · 外观）控制。配置里的旧值在启动时被
        // `sanitize_highlight` 迁移一次（见 `core::theme::migrate_code_theme`）。
        with_meta(
            choice_field(
                "preview.image_display",
                "图片显示",
                "预览区图片的呈现方式（原始尺寸无法实现：渲染层会强制限制在预览宽度内）",
                Preview,
                "渲染",
                None,
                FieldKind::Choice,
                || {
                    vec![
                        ("fit_width".into(), "适应宽度".into()),
                        ("hidden".into(), "不显示".into()),
                    ]
                },
                |c| match c.preview.image_display {
                    ImageDisplay::Hidden => "hidden".into(),
                    // 适应宽度与旧版本遗留值都按适应宽度呈现
                    _ => "fit_width".into(),
                },
                |c, v| {
                    c.preview.image_display = match v.as_str() {
                        "hidden" => ImageDisplay::Hidden,
                        _ => ImageDisplay::FitWidth,
                    }
                },
            ),
            // 已接线：预处理阶段按此设置决定内联渲染还是退化为链接
            None,
            None,
        ),
        with_meta(
            number_field(
                "preview.image_width_percent",
                "图片宽度",
                "图片占预览区宽度的百分比，范围 20–100",
                Preview,
                "渲染",
                20.0,
                100.0,
                5.0,
                "%",
                |c| c.preview.image_width_percent as f32,
                |c, v| c.preview.image_width_percent = v as u32,
            ),
            None,
            // 不显示图片时宽度没有意义
            Some(|c| {
                if c.preview.image_display == ImageDisplay::Hidden {
                    Some("当前设置为不显示图片")
                } else {
                    None
                }
            }),
        ),
        with_meta(
            choice_field(
                "preview.link_open",
                "链接打开方式",
                "点击预览区链接时的行为",
                Preview,
                "渲染",
                None,
                FieldKind::Choice,
                || {
                    vec![
                        ("in_app".into(), "应用内打开".into()),
                        ("external".into(), "系统浏览器".into()),
                    ]
                },
                |c| match c.preview.link_open {
                    LinkOpen::InApp => "in_app".into(),
                    LinkOpen::External => "external".into(),
                },
                |c, v| {
                    c.preview.link_open = if v == "external" {
                        LinkOpen::External
                    } else {
                        LinkOpen::InApp
                    }
                },
            ),
            // 已接线（preview.rs 的 on_link_click）：
            // InApp 会在应用内打开库内 .md，否则用系统浏览器；点击图片则打开预览窗口
            None,
            None,
        ),
        // ── 预览 · 排版 ──
        choice_field(
            "preview.font.family",
            "预览字体",
            "预览区正文字体",
            Preview,
            "排版",
            None,
            FieldKind::Font,
            font_options,
            |c| c.preview.font.family.clone(),
            |c, v| c.preview.font.family = v,
        ),
        number_field(
            "preview.font.size",
            "预览字号",
            "预览区正文字号，范围 8–72 px",
            Preview,
            "排版",
            8.0,
            72.0,
            1.0,
            "px",
            |c| c.preview.font.size,
            |c, v| c.preview.font.size = v,
        ),
        number_field(
            "preview.font.line_height",
            "预览行高",
            "预览区行高倍数",
            Preview,
            "排版",
            0.8,
            3.0,
            0.1,
            "倍",
            |c| c.preview.font.line_height,
            |c, v| c.preview.font.line_height = v,
        ),
        choice_field(
            "preview.font.weight",
            "预览字重",
            "预览区正文字体的粗细",
            Preview,
            "排版",
            None,
            FieldKind::Choice,
            font_weight_options,
            |c| font_weight_value(c.preview.font.weight),
            |c, v| c.preview.font.weight = font_weight_parse(&v),
        ),
        choice_field(
            "preview.density",
            "版面密度",
            "预览留白与段间距：紧凑 / 适中 / 宽松",
            Preview,
            "排版",
            None,
            FieldKind::Choice,
            || {
                vec![
                    ("compact".into(), "紧凑".into()),
                    ("normal".into(), "适中".into()),
                    ("relaxed".into(), "宽松".into()),
                ]
            },
            |c| c.preview.density.value(),
            |c, v| c.preview.density = PreviewDensity::from_value(&v),
        ),
        number_field(
            "preview.max_width",
            "正文最大宽度",
            "0 表示不限制；用于收窄阅读区",
            Preview,
            "排版",
            0.0,
            4_096.0,
            64.0,
            "px",
            |c| c.preview.max_width as f32,
            |c, v| c.preview.max_width = v as u32,
        ),
        // ── 预览 · 同步 ──
        bool_field(
            "preview.sync_scroll",
            "同步滚动",
            "编辑区与预览区滚动位置互相同步",
            Preview,
            "同步",
            None,
            |c| c.preview.sync_scroll,
            |c, v| c.preview.sync_scroll = v,
        ),
    ]
}

// ── 控件 ──────────────────────────────────────────────────────

/// 选项少于等于 4 个时用分段按钮，直接可见、无需展开
fn segmented(
    id: &'static str,
    items: &[(String, String)],
    current: &str,
    set: fn(&mut AppConfig, String),
) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap_1().justify_end();
    for (ix, (value, label)) in items.iter().enumerate() {
        let selected = value == current;
        let value = value.clone();
        let mut button = Button::new(format!("{id}#{ix}"))
            .small()
            .label(label.clone());
        button = if selected {
            button.primary()
        } else {
            button.ghost()
        };
        row = row.child(button.on_click(move |_, _, cx| {
            let value = value.clone();
            crate::app::update_config(cx, move |cfg| set(cfg, value));
        }));
    }
    row.into_any_element()
}

/// 选项多于 4 个时用下拉，避免占满整行
fn dropdown(
    id: &'static str,
    items: &[(String, String)],
    current: &str,
    set: fn(&mut AppConfig, String),
) -> AnyElement {
    let label = items
        .iter()
        .find(|(v, _)| v == current)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| current.to_string());
    let items = items.to_vec();

    DropdownButton::new(id)
        .button(Button::new(id).small().outline().label(label))
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            for (value, text) in items.iter() {
                let value = value.clone();
                menu = menu.item(PopupMenuItem::new(text.clone()).on_click(move |_, _, cx| {
                    let value = value.clone();
                    crate::app::update_config(cx, move |cfg| set(cfg, value));
                }));
            }
            menu
        })
        .into_any_element()
}

/// 数值：步进器 + 可直接输入的输入框
///
/// 参数 8 个（id / 当前值 / 最小 / 最大 / 步长 / 单位 / 写入回调 / view），
/// 就是"画一个步进器需要知道的全部"，拆成结构体反而要在调用点来回传。
#[allow(clippy::too_many_arguments)]
fn number_control(
    view: &mut SettingsView,
    id: &'static str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    unit: &'static str,
    set: fn(&mut AppConfig, f32),
) -> AnyElement {
    let dec = Button::new(format!("{id}-dec"))
        .small()
        .ghost()
        .label("−")
        .on_click(move |_, _, cx| {
            let next = (value - step).max(min);
            crate::app::update_config(cx, move |cfg| set(cfg, next));
        });
    let inc = Button::new(format!("{id}-inc"))
        .small()
        .ghost()
        .label("+")
        .on_click(move |_, _, cx| {
            let next = (value + step).min(max);
            crate::app::update_config(cx, move |cfg| set(cfg, next));
        });

    // 输入框支持直接键入精确值；实体在 new() 中创建，这里只负责渲染
    let input = match view.inputs.get(id) {
        Some(state) => Input::new(state).small().w(px(64.)).into_any_element(),
        None => div().child(format!("{value}{unit}")).into_any_element(),
    };

    div()
        .flex()
        .items_center()
        .gap_1()
        .child(dec)
        .child(input)
        .child(div().text_xs().child(unit))
        .child(inc)
        .into_any_element()
}

/// 字体候选：系统字体（首次使用时缓存）+ 常用兜底项
/// 字重选项，与 [`FontWeight`] 一一对应
fn font_weight_options() -> Vec<(String, String)> {
    vec![
        ("regular".into(), "常规".into()),
        ("medium".into(), "中等".into()),
        ("semibold".into(), "半粗".into()),
        ("bold".into(), "粗体".into()),
    ]
}

fn font_weight_value(weight: FontWeight) -> String {
    match weight {
        FontWeight::Regular => "regular".into(),
        FontWeight::Medium => "medium".into(),
        FontWeight::Semibold => "semibold".into(),
        FontWeight::Bold => "bold".into(),
    }
}

fn font_weight_parse(v: &str) -> FontWeight {
    match v {
        "medium" => FontWeight::Medium,
        "semibold" => FontWeight::Semibold,
        "bold" => FontWeight::Bold,
        _ => FontWeight::Regular,
    }
}

fn font_options() -> Vec<(String, String)> {
    let mut names = SYSTEM_FONTS
        .get()
        .cloned()
        .unwrap_or_else(|| FALLBACK_FONTS.iter().map(|s| s.to_string()).collect());

    // 系统默认字体以 . 开头，排在最前便于选择
    names.retain(|n| !n.is_empty());
    for special in [".SystemUIFont", ".SystemMonospacedFont"] {
        if !names.iter().any(|n| n == special) {
            names.insert(0, special.to_string());
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let label = font_label(&n);
            (n, label)
        })
        .collect()
}

fn font_label(name: &str) -> String {
    match name {
        ".SystemUIFont" => "系统默认".into(),
        ".SystemMonospacedFont" => "系统默认（等宽）".into(),
        other => other.to_string(),
    }
}

static SYSTEM_FONTS: OnceLock<Vec<String>> = OnceLock::new();
const FALLBACK_FONTS: &[&str] = &[
    "PingFang SC",
    "Microsoft YaHei",
    "Helvetica",
    "Arial",
    "Menlo",
    "Courier",
];

// ── 视图 ──────────────────────────────────────────────────────

pub struct SettingsView {
    tab: SettingsTab,
    query: String,
    /// 命中的设置项 id，用于搜索后高亮
    highlight: Option<&'static str>,
    /// 数值项的输入框状态，按字段 id 索引
    inputs: HashMap<&'static str, Entity<InputState>>,
    /// 顶部搜索框的输入状态
    search_input: Entity<InputState>,
    /// 上次自适应所用的内容指纹（分类 + 搜索词 + 可见项数 + 分组数）。
    /// 指纹不变就不动窗口尺寸，避免每帧 resize，也保留用户手动调整的结果。
    size_fingerprint: Option<(SettingsTab, String, usize, usize)>,
}

/// 设置窗口各部分的固定尺寸。
///
/// 框架没有通用的"测量元素"API，无法在渲染前拿到内容的自然高度；
/// 因此这里把每部分的行高固定下来，窗口高度即可按可见项数**精确计算**，
/// 从而做到打开时贴合内容：既没有多余空白，也不会出现滚动条。
mod layout {
    pub const SIDEBAR_W: f32 = 160.;
    pub const CONTENT_W: f32 = 560.;
    pub const SEARCH_H: f32 = 45.;
    pub const FOOTER_H: f32 = 48.;
    /// 单个设置项行高（含上下内边距）
    pub const ROW_H: f32 = 52.;
    /// 分组标题行高
    pub const GROUP_H: f32 = 32.;
    /// 内容区上下内边距之和（`py_2` × 2）
    pub const CONTENT_PAD_Y: f32 = 16.;
    /// 搜索无结果时的空态高度
    pub const EMPTY_H: f32 = 64.;
    pub const MIN_H: f32 = 220.;
    pub const MAX_H: f32 = 860.;

    /// 按可见项数与分组数计算窗口内容区高度。
    pub fn window_height(rows: usize, groups: usize) -> f32 {
        let content = if rows == 0 {
            EMPTY_H
        } else {
            CONTENT_PAD_Y + rows as f32 * ROW_H + groups as f32 * GROUP_H
        };
        (SEARCH_H + FOOTER_H + content).clamp(MIN_H, MAX_H)
    }

    /// 窗口宽度固定：侧栏 + 内容列。固定宽度可避免控件列在切换分类时左右跳动。
    pub fn window_width() -> f32 {
        SIDEBAR_W + CONTENT_W
    }
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 系统字体列表只取一次，之后全局缓存
        SYSTEM_FONTS.get_or_init(|| cx.text_system().all_font_names());

        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设置…"));

        let mut this = Self {
            tab: SettingsTab::Basic,
            query: String::new(),
            highlight: None,
            inputs: HashMap::new(),
            search_input,
            // None 表示"还没自适应过"，首次渲染会立即贴合内容
            size_fingerprint: None,
        };

        // 搜索框是唯一的 query 写入口：输入即过滤，无需回车
        cx.subscribe_in(
            &this.search_input,
            window,
            |this, state, ev: &InputEvent, _window, cx| {
                if !matches!(ev, InputEvent::Change) {
                    return;
                }
                this.query = state.read(cx).value().to_string();
                cx.notify();
            },
        )
        .detach();

        let cfg = crate::app::config(cx);
        for field in all_fields() {
            if field.kind != FieldKind::Number {
                continue;
            }
            let state = cx.new(|cx| InputState::new(window, cx));
            let text = format_number(&field, &cfg);
            state.update(cx, |s, cx| s.set_value(text, window, cx));

            // 取出写入回调，避免在 FnMut 闭包里移动捕获的变量
            let set_value = field.set_value.clone();
            cx.subscribe_in(
                &state,
                window,
                move |_this, state, ev: &InputEvent, _window, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    let raw = state.read(cx).value().to_string();
                    let Some(value) = raw.trim().parse::<f32>().ok() else {
                        return;
                    };
                    let set_value = set_value.clone();
                    crate::app::update_config(cx, move |c| set_value(c, value));
                },
            )
            .detach();

            this.inputs.insert(field.id, state);
        }

        this
    }

    /// 命中搜索的第一项，用于自动切分类并定位
    fn first_match(&self, cfg: &AppConfig) -> Option<&'static str> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return None;
        }
        let _ = cfg;
        all_fields()
            .iter()
            .find(|f| matches_query(f, &q))
            .map(|f| f.id)
    }
}

fn matches_query(field: &SettingField, query: &str) -> bool {
    field.title.to_lowercase().contains(query)
        || field.desc.to_lowercase().contains(query)
        || field.group.to_lowercase().contains(query)
        || field.tab.title().to_lowercase().contains(query)
}

fn format_number(field: &SettingField, cfg: &AppConfig) -> String {
    // 数值一律按"去掉多余小数"呈现，避免出现 14.000001
    let value = (field.value)(cfg);
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value:.1}")
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = crate::app::config(cx);
        let fields = all_fields();

        // 搜索命中时自动切到命中项所属分类
        if let Some(id) = self.first_match(&cfg) {
            if let Some(field) = fields.iter().find(|f| f.id == id)
                && self.tab != field.tab
            {
                self.tab = field.tab;
            }
            self.highlight = Some(id);
        } else {
            self.highlight = None;
        }

        // 数值输入框与配置保持同步（步进/导入/重置后回显新值）
        for field in &fields {
            if field.kind != FieldKind::Number {
                continue;
            }
            let text = format_number(field, &cfg);
            if let Some(state) = self.inputs.get(field.id)
                && state.read(cx).value() != text
            {
                state.update(cx, |s, cx| s.set_value(text.clone(), window, cx));
            }
        }

        let query = self.query.trim().to_lowercase();
        let visible: Vec<&SettingField> = fields
            .iter()
            .filter(|f| f.tab == self.tab)
            .filter(|f| query.is_empty() || matches_query(f, &query))
            .collect();

        // ── 左侧分类 ──
        let mut sidebar = div().w(px(160.)).flex_col().gap_1().p_2();
        for tab in SettingsTab::ALL {
            let selected = tab == self.tab;
            let mut button = Button::new(tab.title()).small().label(tab.title());
            button = if selected {
                button.primary()
            } else {
                button.ghost()
            };
            sidebar = sidebar.child(button.on_click(cx.listener(move |this, _, _, cx| {
                this.tab = tab;
                cx.notify();
            })));
        }

        // ── 右侧设置项（按分组）──
        let mut content = div().flex_1().min_w_0().flex_col().px_4().py_2();
        let mut current_group = String::new();
        // 分组数参与窗口高度计算
        let mut group_count = 0usize;
        for field in &visible {
            if field.group != current_group {
                current_group = field.group.to_string();
                group_count += 1;
                content = content.child(
                    div()
                        // 固定行高，使窗口高度可精确计算
                        .h(px(layout::GROUP_H))
                        .flex()
                        .items_center()
                        .pt_2()
                        .pb_1()
                        .text_sm()
                        .font_weight(FontWeight(600.))
                        .child(field.group),
                );
            }
            content = content.child(self.render_field(field, &cfg, cx));
        }

        if visible.is_empty() {
            content = content.child(
                div()
                    .h(px(layout::EMPTY_H))
                    .flex()
                    .items_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("没有匹配的设置项"),
            );
        }

        // ── 窗口尺寸自适应内容 ──
        //
        // 只在"内容指纹"变化时调整：打开窗口、切换分类、搜索过滤会改变可见项数；
        // 单纯改某个配置值不会，因此用户手动调大的窗口不会被立刻缩回。
        let fingerprint = (self.tab, query.clone(), visible.len(), group_count);
        if self.size_fingerprint.as_ref() != Some(&fingerprint) {
            self.size_fingerprint = Some(fingerprint);
            let desired = size(
                px(layout::window_width()),
                px(layout::window_height(visible.len(), group_count)),
            );
            if window.viewport_size() != desired {
                window.resize(desired);
            }
        }

        div()
            .id("settings-view")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex_none()
                    .h(px(layout::SEARCH_H))
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(self.render_search(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(sidebar.border_r_1().border_color(cx.theme().border))
                    .child(content.overflow_y_scrollbar()),
            )
            .child(self.render_footer(cx))
    }
}

impl SettingsView {
    /// 搜索框：输入即过滤，回车跳转到首个命中项
    fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.search_input).small().w_full()),
            )
            .children(if self.query.is_empty() {
                None
            } else {
                Some(
                    Button::new("clear-search")
                        .small()
                        .ghost()
                        .label("清除")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.query.clear();
                            this.search_input.update(cx, |state, cx| {
                                state.set_value(String::new(), window, cx);
                            });
                            cx.notify();
                        })),
                )
            })
    }

    /// 单个设置项：标题 + 说明 + 控件 + 单项重置 + 状态标记
    fn render_field(
        &mut self,
        field: &SettingField,
        cfg: &AppConfig,
        cx: &mut Context<SettingsView>,
    ) -> Div {
        let disabled_reason = (field.disabled)(cfg);
        let highlighted = self.highlight == Some(field.id);
        let control = (field.control)(self, cfg);

        let mut row = div()
            // 固定行高：窗口高度按"可见项数 × 行高"精确计算，避免估算误差
            .h(px(layout::ROW_H))
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .px_2()
            .py_1()
            .rounded_md();
        if highlighted {
            row = row.bg(cx.theme().accent.opacity(0.15));
        }

        // 左：标题 + 说明 + 状态标记
        let mut left = div().flex_1().min_w_0().flex_col();
        let mut title = div().text_sm().child(field.title);
        if let Some(reason) = field.unavailable {
            title = title.child(
                div()
                    .ml_2()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .child(format!("⚠ 暂未生效 · {reason}")),
            );
        }
        left = left.child(title);
        left = left.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(match disabled_reason {
                    Some(reason) => reason.to_string(),
                    None => field.desc.to_string(),
                }),
        );
        row = row.child(left);

        // 右：控件 + 单项重置
        let mut right = div().flex_none().flex().items_center().gap_2();
        match disabled_reason {
            Some(_) => {
                // 依赖未满足：只展示当前值，不提供交互
                right = right.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("已禁用"),
                );
            }
            None => right = right.child(control),
        }
        if !(field.is_default)(cfg) {
            let reset = field.reset.clone();
            right = right.child(
                Button::new(format!("{}-reset", field.id))
                    .small()
                    .ghost()
                    .tooltip("恢复该项默认值")
                    .label("⟲")
                    .on_click(move |_, _, cx| {
                        let reset = reset.clone();
                        crate::app::update_config(cx, move |c| reset(c));
                    }),
            );
        }
        row = row.child(right);
        row
    }

    /// 底部：备份迁移 + 整体恢复默认
    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .h(px(layout::FOOTER_H))
            .flex()
            .items_center()
            .gap_2()
            .p_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("import-config")
                    .small()
                    .ghost()
                    .label("导入配置…")
                    .on_click(|_, _, cx| import_config(cx)),
            )
            .child(
                Button::new("export-config")
                    .small()
                    .ghost()
                    .label("导出配置…")
                    .on_click(|_, _, cx| export_config(cx)),
            )
            .child(
                Button::new("reset-config")
                    .small()
                    .ghost()
                    .label("恢复全部默认")
                    .on_click(|_, _, cx| {
                        crate::app::update_config(cx, |c| *c = AppConfig::default());
                    }),
            )
    }
}

// ── 导入 / 导出 ───────────────────────────────────────────────

fn import_config(cx: &mut App) {
    let prompt = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("选择要导入的配置文件".into()),
    });

    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = prompt.await else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        let result = AppConfig::import_from(&path);
        cx.update(move |cx| match result {
            Ok(config) => {
                crate::app::update_config(cx, |c| *c = config);
                println!("[xink] 已导入配置: {}", path.display());
            }
            Err(err) => eprintln!("[xink] 导入配置失败: {err:?}"),
        });
    })
    .detach();
}

fn export_config(cx: &mut App) {
    let dir = dirs::home_dir().unwrap_or_default();
    let prompt = cx.prompt_for_new_path(&dir, Some("xink-config.json"));

    cx.spawn(async move |cx| {
        let Ok(Ok(Some(path))) = prompt.await else {
            return;
        };
        cx.update(move |cx| {
            let config = crate::app::config(cx);
            match config.export_to(&path) {
                Ok(()) => println!("[xink] 已导出配置: {}", path.display()),
                Err(err) => eprintln!("[xink] 导出配置失败: {err:?}"),
            }
        });
    })
    .detach();
}

// ── 独立设置窗口（单例） ──────────────────────────────────────

/// 记录已打开的设置窗口，重复触发时聚焦而不是再开一个
struct SettingsWindow {
    handle: Option<WindowHandle<Root>>,
}

impl Global for SettingsWindow {}

/// 打开设置窗口；已存在则聚焦已有窗口。
pub fn open_settings(cx: &mut App) {
    let existing = cx.try_global::<SettingsWindow>().and_then(|g| g.handle);

    if let Some(handle) = existing {
        let mut alive = false;
        _ = handle.update(cx, |_, window, _cx| {
            window.activate_window();
            alive = true;
        });
        if alive {
            return;
        }
    }

    cx.spawn(async move |cx| {
        match cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|cx| SettingsView::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        }) {
            Ok(handle) => {
                cx.update(move |cx| {
                    cx.set_global(SettingsWindow {
                        handle: Some(handle),
                    });
                });
            }
            Err(err) => eprintln!("[xink] 打开设置窗口失败: {err:?}"),
        }
    })
    .detach();
}
