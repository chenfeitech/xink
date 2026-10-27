//! 设置界面（独立窗口）
//!
//! 设计要点（详见 `docs/settings-ui-design.md`）：
//! - **元数据驱动**：所有设置项登记在 [`all_fields`] 描述表里，UI 只负责渲染这张表。
//!   新增配置项 = 数据模型加字段 + 表里加一行，无需改动界面骨架。
//! - 三级结构：分类 → 分组 → 设置项，支持搜索定位。
//! - 单项重置（值 ≠ 默认时出现 ⟲）、依赖禁用、未接入渲染项标注"暂未生效"。
//! - 改动经 [`crate::app::update_config`] 立即生效并落盘。

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::dialog::DialogEntrance;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, Size};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::ai::skills;
use crate::core::config::{
    AppConfig, FontWeight, ImageDisplay, LinkOpen, OutputLang, PreviewDensity, ScrollSyncMode,
    ThemeMode,
};
use crate::core::keymap;
use crate::core::theme::PRESET_THEMES;

// ── 分类 ──────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Basic,
    Editor,
    Preview,
    Ai,
}

impl SettingsTab {
    const ALL: [SettingsTab; 4] = [Self::Basic, Self::Editor, Self::Preview, Self::Ai];

    fn title(self) -> &'static str {
        match self {
            SettingsTab::Basic => "基本",
            SettingsTab::Editor => "编辑器",
            SettingsTab::Preview => "预览",
            SettingsTab::Ai => "AI",
        }
    }

    /// 侧栏图标
    ///
    /// 只取 `gpui_component::IconName` 的兼容子集（gpui-kit 内置图标清单），
    /// 不引项目外的图标资源。
    fn icon(self) -> IconName {
        match self {
            SettingsTab::Basic => IconName::Settings,
            SettingsTab::Editor => IconName::FileText,
            SettingsTab::Preview => IconName::Eye,
            SettingsTab::Ai => IconName::Replace,
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
    /// 自由文本（base_url / keep_alive 这类无法枚举的值）
    Text,
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
/// 文本型取值（非文本项恒为空串）
type TextValueFn = Rc<dyn Fn(&AppConfig) -> String>;
/// 文本型写入（非文本项为空操作）
type TextSetFn = Rc<dyn Fn(&mut AppConfig, String)>;

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
    /// 文本型取值（非文本项恒为空串）
    pub text_value: TextValueFn,
    /// 文本型写入（非文本项为空操作）
    pub text_set: TextSetFn,
    pub control: ControlFn,
    /// 输入框的占位提示：留空时显示什么（快捷键项用它显示默认键位）
    pub placeholder: &'static str,
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
        text_value: Rc::new(|_| String::new()),
        text_set: Rc::new(|_, _| {}),
        control,
        placeholder: "",
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
        text_value: Rc::new(|_| String::new()),
        text_set: Rc::new(|_, _| {}),
        control,
        placeholder: "",
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
        text_value: Rc::new(|_| String::new()),
        text_set: Rc::new(|_, _| {}),
        control,
        placeholder: "",
    }
}

/// 文本型设置项：一行 = 一项设置（同上述三个构造器）
#[allow(clippy::too_many_arguments)]
fn text_field(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    tab: SettingsTab,
    group: &'static str,
    // 用闭包而不是 `fn` 指针：快捷键那批项要按 action 名捕获不同的读写路径，
    // 而 fn 指针无法携带捕获。`impl Fn` 对既有的非捕获闭包与函数项都兼容。
    get: impl Fn(&AppConfig) -> String + 'static,
    set: impl Fn(&mut AppConfig, String) + 'static,
) -> SettingField {
    let get: TextValueFn = Rc::new(get);
    let set: TextSetFn = Rc::new(set);

    let get_for_default = get.clone();
    let is_default =
        Rc::new(move |c: &AppConfig| get_for_default(c) == get_for_default(&AppConfig::default()));
    let get_for_reset = get.clone();
    let set_for_reset = set.clone();
    let reset = Rc::new(move |c: &mut AppConfig| {
        set_for_reset(c, get_for_reset(&AppConfig::default()));
    });
    let get_for_control = get.clone();
    let control =
        Rc::new(
            move |view: &mut SettingsView, c: &AppConfig| match view.inputs.get(id) {
                Some(state) => Input::new(state).small().w(px(240.)).into_any_element(),
                None => div().text_xs().child(get_for_control(c)).into_any_element(),
            },
        );
    SettingField {
        id,
        title,
        desc,
        tab,
        group,
        kind: FieldKind::Text,
        unavailable: None,
        disabled: Rc::new(|_| None),
        is_default,
        reset,
        value: Rc::new(|_| 0.0),
        set_value: Rc::new(|_, _| {}),
        text_value: get,
        text_set: set,
        control,
        placeholder: "",
    }
}

/// 在任意设置项之上追加输入框的占位提示（空值时的灰字）
fn with_placeholder(mut field: SettingField, placeholder: &'static str) -> SettingField {
    field.placeholder = placeholder;
    field
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
    use SettingsTab::{Ai, Basic, Editor, Preview};

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
        bool_field(
            "editor.soft_wrap",
            "软换行",
            "长行折行显示（关闭后长行需横向滚动，但滚动同步更精确：一个源码行正好一个显示行）",
            Editor,
            "编辑行为",
            None,
            |c| c.editor.soft_wrap,
            |c, v| c.editor.soft_wrap = v,
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
            "编辑区与预览区滚动位置互相同步（停止滚动约 0.25 秒后对齐一次；滚动过程中不动对端，避免抖动）",
            Preview,
            "同步",
            None,
            |c| c.preview.sync_scroll,
            |c, v| c.preview.sync_scroll = v,
        ),
        choice_field(
            "preview.sync_mode",
            "同步方式",
            "比例跟随：两端按各自滚动百分比对齐，连续平滑（默认）；块锚点：按 Markdown 顶层块对齐，位置更准但块边界会跳；停止后对齐：滚动中不动对端，停下约 0.25 秒后对齐一次",
            Preview,
            "同步",
            None,
            FieldKind::Choice,
            || {
                vec![
                    ("ratio".to_string(), "比例跟随".to_string()),
                    ("anchor".to_string(), "块锚点".to_string()),
                    ("on_stop".to_string(), "停止后对齐".to_string()),
                ]
            },
            |c| match c.preview.sync_mode {
                ScrollSyncMode::Ratio => "ratio".to_string(),
                ScrollSyncMode::Anchor => "anchor".to_string(),
                ScrollSyncMode::OnStop => "on_stop".to_string(),
            },
            |c, v| {
                c.preview.sync_mode = match v.as_str() {
                    "anchor" => ScrollSyncMode::Anchor,
                    "on_stop" => ScrollSyncMode::OnStop,
                    _ => ScrollSyncMode::Ratio,
                };
            },
        ),
        bool_field(
            "preview.debug_scroll_sync",
            "显示同步调试信息",
            "在预览面板标题显示两端当前的源码行号与偏差（排查滚动同步用）",
            Preview,
            "同步",
            None,
            |c| c.preview.debug_scroll_sync,
            |c, v| c.preview.debug_scroll_sync = v,
        ),
        // ── AI · 连接 ──
        bool_field(
            "ai.enabled",
            "启用 AI",
            "关闭时所有 AI 入口不可用，也不会发起任何网络请求",
            Ai,
            "连接",
            None,
            |c| c.ai.enabled,
            |c, v| c.ai.enabled = v,
        ),
        text_field(
            "ai.ollama.base_url",
            "Ollama 地址",
            "本地 Ollama 的服务地址（默认 http://localhost:11434）",
            Ai,
            "连接",
            |c| c.ai.ollama.base_url.clone(),
            |c, v| c.ai.ollama.base_url = v,
        ),
        text_field(
            "ai.ollama.keep_alive",
            "模型常驻时长",
            "Ollama keep_alive：5m 表示 5 分钟，-1 常驻，0 用完即卸",
            Ai,
            "连接",
            |c| c.ai.ollama.keep_alive.clone(),
            |c, v| c.ai.ollama.keep_alive = v,
        ),
        number_field(
            "ai.ollama.num_ctx",
            "上下文窗口",
            "Ollama num_ctx：超过模型上限时会被自动夹回",
            Ai,
            "连接",
            512.,
            131072.,
            512.,
            "",
            |c| c.ai.ollama.num_ctx as f32,
            |c, v| c.ai.ollama.num_ctx = v as u32,
        ),
        // ── AI · 生成 ──
        number_field(
            "ai.timeout_secs",
            "静默超时",
            "两次响应之间允许的最长间隔（秒）",
            Ai,
            "生成",
            5.,
            600.,
            5.,
            "秒",
            |c| c.ai.timeout_secs as f32,
            |c, v| c.ai.timeout_secs = v as u64,
        ),
        number_field(
            "ai.max_context_chars",
            "上下文上限",
            "发给模型的正文字符数上限，超出的部分会被截断并提示",
            Ai,
            "生成",
            1000.,
            200000.,
            1000.,
            "字",
            |c| c.ai.max_context_chars as f32,
            |c, v| c.ai.max_context_chars = v as usize,
        ),
        number_field(
            "ai.max_steps",
            "最大步数",
            "一次任务里模型最多可以调用多少轮工具",
            Ai,
            "生成",
            1.,
            64.,
            1.,
            "步",
            |c| c.ai.max_steps as f32,
            |c, v| c.ai.max_steps = v as usize,
        ),
        choice_field(
            "ai.output_language",
            "输出语言",
            "自动：跟随输入内容的语言",
            Ai,
            "生成",
            None,
            FieldKind::Choice,
            || {
                vec![
                    ("auto".into(), "跟随输入".into()),
                    ("zh".into(), "简体中文".into()),
                    ("en".into(), "English".into()),
                ]
            },
            |c| match c.ai.output_language {
                OutputLang::Auto => "auto".into(),
                OutputLang::Zh => "zh".into(),
                OutputLang::En => "en".into(),
            },
            |c, v| {
                c.ai.output_language = match v.as_str() {
                    "zh" => OutputLang::Zh,
                    "en" => OutputLang::En,
                    _ => OutputLang::Auto,
                }
            },
        ),
        // ── AI · 扩展 ──
        bool_field(
            "ai.skills_enabled",
            "启用 Skills",
            "库内 .xink/skills 与用户级 skills 是否参与命令面板",
            Ai,
            "扩展",
            None,
            |c| c.ai.skills_enabled,
            |c, v| c.ai.skills_enabled = v,
        ),
    ]
    .into_iter()
    // 快捷键：每个可绑定动作一项，表里在 `core/keymap.rs`
    .chain(keymap_fields())
    .collect()
}

/// 快捷键设置项：一个动作一行
///
/// 值就是 `editor.keymap` 里的覆盖值；留空（或从没配过）表示用默认键位，
/// 默认键位写在说明里，同时作为输入框的占位提示。
/// 绑定只在启动时下发一次，所以改动**重启后生效**（说明里已标注）。
fn keymap_fields() -> Vec<SettingField> {
    keymap::BINDINGS
        .iter()
        .map(|binding| {
            let action = binding.action;
            let field = text_field(
                binding.id,
                binding.label,
                binding.desc,
                SettingsTab::Editor,
                "快捷键",
                move |c: &AppConfig| c.editor.keymap.get(action).cloned().unwrap_or_default(),
                move |c: &mut AppConfig, v: String| {
                    let value = keymap::normalize(&v);
                    if value.is_empty() {
                        c.editor.keymap.remove(action);
                    } else {
                        c.editor.keymap.insert(action.to_string(), value);
                    }
                },
            );
            // 空串默认表示"默认不绑定"，占位提示换成更好懂的说法
            let hint = if binding.default.is_empty() {
                "默认不绑定"
            } else {
                binding.default
            };
            with_placeholder(field, hint)
        })
        .collect()
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
    /// 数值 / 文本项的输入框状态，按字段 id 索引
    inputs: HashMap<&'static str, Entity<InputState>>,
    /// 顶部搜索框的输入状态
    search_input: Entity<InputState>,
    /// 「新增模型」的模型名输入
    new_model_id: Entity<InputState>,
    /// 「新增模型」的 base_url 输入（留空用全局地址）
    new_model_base: Entity<InputState>,
    /// 拉取模型列表的后台任务
    model_task: Option<Task<()>>,
    /// 模型管理区的提示 / 错误
    model_notice: Option<String>,
    /// 「清空 AI 快照」已按过一次，等待确认
    ///
    /// 清快照是删文件、不可撤销，两步确认比弹一个确认框轻。
    confirm_clear_snapshots: bool,
    /// 快照区的提示 / 错误
    snapshot_notice: Option<String>,
    /// 已发现的 skill 与被跳过的项：发现要读盘，缓存住，
    /// 只在首次渲染或点了"重新扫描"时读一次
    skills_cache: Option<(Vec<skills::Skill>, Vec<skills::SkillIssue>)>,
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
    /// 搜索无结果时的空态高度
    pub const EMPTY_H: f32 = 64.;
    /// 浮层高度：够放下大多数分类，超出部分由内容列滚动
    pub const INITIAL_H: f32 = 620.;

    /// 浮层宽度：侧栏 + 内容列。固定宽度可避免控件列在切换分类时左右跳动。
    pub fn window_width() -> f32 {
        SIDEBAR_W + CONTENT_W
    }
}

impl SettingsView {
    /// 把焦点交给搜索框（浮层打开 / 重复触发时调用）
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 系统字体列表只取一次，之后全局缓存
        SYSTEM_FONTS.get_or_init(|| cx.text_system().all_font_names());

        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设置…"));

        let new_model_id =
            cx.new(|cx| InputState::new(window, cx).placeholder("模型名，如 qwen3.5:0.8b"));
        let new_model_base =
            cx.new(|cx| InputState::new(window, cx).placeholder("地址（留空用全局地址）"));

        let mut this = Self {
            tab: SettingsTab::Basic,
            query: String::new(),
            highlight: None,
            inputs: HashMap::new(),
            search_input,
            new_model_id,
            new_model_base,
            model_task: None,
            model_notice: None,
            confirm_clear_snapshots: false,
            snapshot_notice: None,
            skills_cache: None,
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
            let is_number = field.kind == FieldKind::Number;
            let is_text = field.kind == FieldKind::Text;
            if !is_number && !is_text {
                continue;
            }
            let placeholder = field.placeholder;
            let state = cx.new(|cx| {
                let state = InputState::new(window, cx);
                if placeholder.is_empty() {
                    state
                } else {
                    state.placeholder(placeholder)
                }
            });
            let text = if is_number {
                format_number(&field, &cfg)
            } else {
                (field.text_value)(&cfg)
            };
            state.update(cx, |s, cx| s.set_value(text, window, cx));

            // 取出写入回调，避免在 FnMut 闭包里移动捕获的变量
            let set_value = field.set_value.clone();
            let text_set = field.text_set.clone();
            cx.subscribe_in(
                &state,
                window,
                move |_this, state, ev: &InputEvent, _window, cx| {
                    if !matches!(ev, InputEvent::Change) {
                        return;
                    }
                    let raw = state.read(cx).value().to_string();
                    if is_number {
                        let Some(value) = raw.trim().parse::<f32>().ok() else {
                            return;
                        };
                        let set_value = set_value.clone();
                        crate::app::update_config(cx, move |c| set_value(c, value));
                    } else {
                        let text_set = text_set.clone();
                        crate::app::update_config(cx, move |c| text_set(c, raw));
                    }
                },
            )
            .detach();

            this.inputs.insert(field.id, state);
        }

        this
    }

    /// 模型管理区：拉取 / 新增 / 设为默认 / 停用 / 删除
    ///
    /// 写入一律走 `crate::app::update_config`（校正 + 落盘 + 重绘），
    /// 不直接改全局，避免与"改完立即生效"的既有约定脱节。
    fn render_models(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let cfg = crate::app::config(cx);
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let primary = cx.theme().primary;

        let mut block = div()
            .flex_col()
            .gap_1()
            .pt_3()
            .mt_2()
            .border_t_1()
            .border_color(border)
            .child(div().text_sm().font_weight(FontWeight(600.)).child("模型"));

        // ── 拉取 ──
        let base_url = cfg.ai.ollama.base_url.clone();
        block = block.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Button::new("ai-fetch-models")
                        .small()
                        .outline()
                        .label("从 Ollama 拉取")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let base = base_url.clone();
                            this.model_notice = Some("正在拉取…".into());
                            let base_for_task = base.clone();
                            let task = cx.background_spawn(async move {
                                crate::core::ai::ollama::list_models(
                                    &base_for_task,
                                    std::time::Duration::from_secs(5),
                                )
                            });
                            this.model_task =
                                Some(cx.spawn_in(window, async move |this, window| {
                                    let result = task.await;
                                    _ = this.update_in(window, |this, _window, cx| {
                                        match result {
                                            Ok(models) => {
                                                let count = models.len();
                                                let cache = crate::core::ai::models::ModelCache {
                                                    fetched_at_ms: crate::core::ai::models::now_ms(
                                                    ),
                                                    base_url: base.clone(),
                                                    models,
                                                };
                                                let mut added = 0;
                                                crate::app::update_config(cx, |cfg| {
                                                    added = cache.merge_into(&mut cfg.ai.models);
                                                });
                                                this.model_notice = Some(format!(
                                                    "拉取到 {count} 个模型，新增 {added} 个"
                                                ));
                                            }
                                            Err(err) => {
                                                this.model_notice = Some(err.message());
                                            }
                                        }
                                        cx.notify();
                                    });
                                }));
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("只更新上下文上限与能力，不会覆盖你改过的参数"),
                ),
        );

        // ── 列表 ──
        let models: Vec<(String, String, bool, bool)> = cfg
            .ai
            .models
            .iter()
            .map(|m| {
                (
                    m.id.clone(),
                    m.capabilities.join("/"),
                    m.enabled,
                    m.id == cfg.ai.default_model,
                )
            })
            .collect();

        if models.is_empty() {
            block = block.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("还没有模型：点上面拉取，或在下面手动添加"),
            );
        }

        for (id, caps, enabled, is_default) in models {
            let id_for_default = id.clone();
            let id_for_toggle = id.clone();
            let id_for_delete = id.clone();
            let id_for_probe = id.clone();
            let mut row = div()
                .flex()
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(if is_default {
                            primary
                        } else {
                            cx.theme().foreground
                        })
                        .child(id.clone()),
                )
                .child(div().text_xs().text_color(muted).child(caps))
                .child(
                    Button::new(SharedString::from(format!("ai-default-{id}")))
                        .small()
                        .ghost()
                        .label(if is_default {
                            "当前默认"
                        } else {
                            "设为默认"
                        })
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let id = id_for_default.clone();
                            crate::app::update_config(cx, move |cfg| cfg.ai.default_model = id);
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("ai-toggle-{id}")))
                        .small()
                        .ghost()
                        .label(if enabled { "停用" } else { "启用" })
                        .on_click(cx.listener(move |_, _, _, cx| {
                            let id = id_for_toggle.clone();
                            crate::app::update_config(cx, move |cfg| {
                                if let Some(m) = cfg.ai.models.iter_mut().find(|m| m.id == id) {
                                    m.enabled = !m.enabled;
                                }
                            });
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("ai-delete-{id}")))
                        .small()
                        .ghost()
                        .label("删除")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id_for_delete.clone();
                            crate::app::update_config(cx, move |cfg| {
                                cfg.ai.models.retain(|m| m.id != id);
                            });
                            this.model_notice = Some("已删除".into());
                        })),
                )
                // 连接测试：能力声明不可信（设计 D5），是否支持工具
                // 只能靠一次真实探针来判断
                .child(
                    Button::new(SharedString::from(format!("ai-probe-{id}")))
                        .small()
                        .ghost()
                        .label("测试")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let id = id_for_probe.clone();
                            let cfg = crate::app::config(cx);
                            let Some(model) = cfg.ai.models.iter().find(|m| m.id == id).cloned()
                            else {
                                this.model_notice = Some("模型已不在列表中".into());
                                return;
                            };
                            let ai = cfg.ai.clone();
                            this.model_notice = Some(format!("正在测试 {id}…"));
                            let task = cx.background_spawn(async move {
                                crate::core::ai::probe::probe_model(&ai, &model)
                            });
                            this.model_task =
                                Some(cx.spawn_in(window, async move |this, window| {
                                    let result = task.await;
                                    // 面板可能已经随设置窗口关掉了，拿不到就算了
                                    _ = this.update_in(window, |this, _window, cx| {
                                        let id_for_write = id.clone();
                                        let tools = result.supports_tools;
                                        crate::app::update_config(cx, move |cfg| {
                                            if let Some(m) = cfg
                                                .ai
                                                .models
                                                .iter_mut()
                                                .find(|m| m.id == id_for_write)
                                            {
                                                m.tools_enabled = tools;
                                                // 能力列表跟着探针结论走：
                                                // 声明有但实际不支持的，要摘掉
                                                m.capabilities.retain(|c| c != "tools");
                                                if tools {
                                                    m.capabilities.push("tools".into());
                                                }
                                                m.last_checked_ms =
                                                    crate::core::ai::models::now_ms();
                                            }
                                        });
                                        this.model_notice = Some(match result.error {
                                            Some(err) => format!("{id}：{}", err.message()),
                                            None => format!(
                                                "{id}：{} {} ms{}",
                                                if tools {
                                                    "支持工具"
                                                } else {
                                                    "不支持工具"
                                                },
                                                result.elapsed_ms,
                                                if tools {
                                                    ""
                                                } else {
                                                    "（将按单轮使用）"
                                                }
                                            ),
                                        });
                                    });
                                }));
                        })),
                );
            let _ = &mut row;
            block = block.child(row);
        }

        // ── 手动新增 ──
        block = block.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .pt_2()
                .child(Input::new(&self.new_model_id).small().w(px(200.)))
                .child(Input::new(&self.new_model_base).small().w(px(180.)))
                .child(
                    Button::new("ai-add-model")
                        .small()
                        .outline()
                        .label("添加为 Ollama 模型")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let id = this.new_model_id.read(cx).value().trim().to_string();
                            let base = this.new_model_base.read(cx).value().trim().to_string();
                            if id.is_empty() {
                                this.model_notice = Some("请先填写模型名".into());
                                return;
                            }
                            let entry = crate::core::config::ModelEntry {
                                id: id.clone(),
                                base_url: base,
                                ..Default::default()
                            };
                            let mut added = false;
                            crate::app::update_config(cx, |cfg| {
                                if cfg.ai.models.iter().any(|m| m.id == id) {
                                    return;
                                }
                                cfg.ai.models.push(entry);
                                if cfg.ai.default_model.is_empty() {
                                    cfg.ai.default_model = id.clone();
                                }
                                added = true;
                            });
                            this.model_notice = Some(if added {
                                format!("已添加 {id}")
                            } else {
                                "该模型已在列表中".into()
                            });
                        })),
                ),
        );

        if let Some(notice) = &self.model_notice {
            block = block.child(div().text_xs().text_color(muted).child(notice.clone()));
        }
        block
    }

    /// AI 改前快照：自动轮转之外，给一个手动清空的入口（需求 A8.4）
    ///
    /// 快照是"AI 改坏了能回退"的唯一手段（项目没有撤销栈），所以数量要看得见、
    /// 能清干净；清之前要求再点一次确认。
    fn render_snapshots(&mut self, cx: &mut Context<Self>) -> Div {
        use crate::core::ai::snapshot::{MAX_SNAPSHOTS, SNAPSHOT_DIR, clear, count};

        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let dir = crate::state::workspace::app_data_dir().join(SNAPSHOT_DIR);
        let total = count(&dir);

        let mut block = div()
            .flex_col()
            .gap_1()
            .pt_3()
            .mt_2()
            .border_t_1()
            .border_color(border)
            .child(div().text_sm().font_weight(FontWeight(600.)).child("快照"));

        block = block.child(div().text_xs().text_color(muted).child(format!(
            "改前快照 {total} 个（自动保留最近 {MAX_SNAPSHOTS} 个）"
        )));

        // 两步确认：第一次点击只是把按钮换成"确认清空？"
        let confirming = self.confirm_clear_snapshots;
        let dir_for_click = dir.clone();
        let mut row = div().flex().items_center().gap_2().child(
            Button::new("ai-clear-snapshots")
                .small()
                .outline()
                .label(if confirming {
                    "确认清空？"
                } else {
                    "清空快照"
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.confirm_clear_snapshots {
                        this.confirm_clear_snapshots = true;
                        cx.notify();
                        return;
                    }
                    this.confirm_clear_snapshots = false;
                    this.snapshot_notice = match clear(&dir_for_click) {
                        Ok(0) => Some("没有可清空的快照".into()),
                        Ok(n) => Some(format!("已清空 {n} 个快照")),
                        Err(err) => Some(format!("清空失败: {err}")),
                    };
                    cx.notify();
                })),
        );
        if confirming {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("再点一次删除，切走分类即取消"),
            );
        }
        block = block.child(row);

        if let Some(notice) = &self.snapshot_notice {
            block = block.child(div().text_xs().text_color(muted).child(notice.clone()));
        }
        block
    }

    /// Skills：列出发现到的技能、逐个启停，并给出被跳过的原因
    /// （需求 A5.6 / A5.7，设计 `docs/ai/ai-skills-design.md` §3.5）
    ///
    /// 与「模型管理」「快照」同一个理由单独渲染：列表是**运行时数据**，
    /// 元数据表（一行一项、取值 / 写入都是固定字段）表达不了。
    fn render_skills(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let cfg = crate::app::config(cx);
        let root = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root());

        if self.skills_cache.is_none() {
            let user_dir = skills::user_skills_dir(&crate::state::workspace::app_data_dir());
            self.skills_cache = Some(skills::discover(root.as_deref(), &user_dir));
        }
        let (found, issues) = self.skills_cache.clone().unwrap_or_default();

        let mut block = div()
            .flex_col()
            .gap_1()
            .pt_3()
            .mt_2()
            .border_t_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(FontWeight(600.))
                            .child("Skills"),
                    )
                    .child(
                        Button::new("ai-rescan-skills")
                            .small()
                            .outline()
                            .label("重新扫描")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.skills_cache = None;
                                cx.notify();
                            })),
                    ),
            );

        block = block.child(
            div()
                .text_xs()
                .text_color(muted)
                .child(format!("发现 {} 个技能", found.len())),
        );
        if !cfg.ai.skills_enabled {
            block = block.child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("总开关已关闭：下面的启停不影响命令面板"),
            );
        }

        for skill in &found {
            let name = skill.name.clone();
            let enabled = !cfg.ai.disabled_skills.iter().any(|n| n == &skill.name);
            let toggle_name = name.clone();
            block = block.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex_col()
                            .child(div().text_sm().child(format!(
                                "{} · {}",
                                skill.name,
                                skill.source.label()
                            )))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(skill.description.clone()),
                            ),
                    )
                    .child(
                        Switch::new(format!("skill-{name}"))
                            .checked(enabled)
                            .on_click(move |v, _, cx| {
                                let v = *v;
                                let name = toggle_name.clone();
                                crate::app::update_config(cx, move |cfg| {
                                    if v {
                                        cfg.ai.disabled_skills.retain(|n| n != &name);
                                    } else if !cfg.ai.disabled_skills.contains(&name) {
                                        cfg.ai.disabled_skills.push(name.clone());
                                    }
                                });
                            }),
                    ),
            );
        }

        // 被跳过的要看得见原因：静默消失会让用户以为"放了没生效"
        for issue in &issues {
            block = block.child(div().text_xs().text_color(muted).child(format!(
                "跳过 {}: {}",
                issue.path.display(),
                issue.reason
            )));
        }

        if found.is_empty() && issues.is_empty() {
            block =
                block.child(div().text_xs().text_color(muted).child(
                    "还没有技能：放到库内 .xink/skills/<名称>/SKILL.md，或用户级 skills 目录",
                ));
        }
        block
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
            let is_number = field.kind == FieldKind::Number;
            let is_text = field.kind == FieldKind::Text;
            if !is_number && !is_text {
                continue;
            }
            let text = if is_number {
                format_number(field, &cfg)
            } else {
                (field.text_value)(&cfg)
            };
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
        //
        // 不用 `Button`：它内部强制 `justify_center`，图标 + 文字会被居中，
        // 改不了（Styled 的调用顺序在 render 里被覆盖）。这里自己排一行，
        // 左靠齐 + 图标在前，选中态用主色底。
        let mut sidebar = div()
            .w(px(layout::SIDEBAR_W))
            // 内容列是 `flex_1`（且滚动容器自带 `w_full`），不锁住的话
            // 这一列会被挤窄
            .flex_none()
            .flex_col()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(cx.theme().border);
        for tab in SettingsTab::ALL {
            let selected = tab == self.tab;
            // `.id()` 是必须的：只有 `Stateful` 才带点击与 hover 交互
            let mut item = div()
                .id(format!("settings-tab-{}", tab.title()))
                .h_8()
                .px_2()
                .w_full()
                .flex()
                .items_center()
                // 左靠齐：图标 + 文字从左侧排，不做居中
                .justify_start()
                .gap_2()
                .rounded(cx.theme().radius)
                .text_sm()
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().primary.opacity(0.12)));
            // 框架没给 Stateful 提供 `when`，选中态用普通分支（同 `rail.rs`）
            if selected {
                item = item
                    .bg(cx.theme().primary)
                    .text_color(cx.theme().primary_foreground);
            } else {
                item = item.text_color(cx.theme().foreground);
            }
            sidebar = sidebar.child(
                item.child(
                    Icon::new(tab.icon())
                        // 图标颜色跟随文字色，选中态自动反白
                        .with_size(Size::Size(px(16.))),
                )
                .child(tab.title())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tab = tab;
                    cx.notify();
                })),
            );
        }

        // ── 右侧设置项（按分组）──
        //
        // 同样要 `min_h_0`：否则这一列会被自己的内容撑到"内容高度"，
        // 滚动区就有等于内容的高度，滚不动（同上面对话框内容层的坑）。
        let mut content = div().flex_1().min_w_0().min_h_0().flex_col().px_4().py_2();
        let mut current_group = String::new();
        for field in &visible {
            if field.group != current_group {
                current_group = field.group.to_string();
                content = content.child(
                    div()
                        // 固定行高，分组标题不随内容长短跳动
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

        // AI 分类额外挂一块「模型管理」：模型是列表、要能拉取与增删，
        // 元数据表（一行一项、取值/写入都是固定字段）表达不了，因此单独渲染。
        if self.tab == SettingsTab::Ai && query.is_empty() {
            content = content.child(self.render_models(window, cx));
            // 同一分类下再挂一块「快照」：列表类信息，元数据表表达不了
            content = content.child(self.render_snapshots(cx));
            // Skills 同理：条目来自运行时发现，且要逐个启停
            content = content.child(self.render_skills(cx));
        } else {
            // 切走就是放弃"确认清空"
            self.confirm_clear_snapshots = false;
        }

        // 尺寸由外层对话框给定，这里不再动窗口：设置界面现在是主窗口内的
        // 居中浮层（见 [`open_settings`]），调 `window.resize` 会把主窗口改小。
        // 内容超出时由内容列自己的滚动条承担。

        div()
            .id("settings-view")
            // `min_h_0` 与对话框内容层同理：不许被内容撑开，
            // 高度必须等于对话框给的可视高度，滚动才成立。
            .size_full()
            .min_h_0()
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
                    .child(sidebar)
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

// ── 设置浮层（主窗口内居中，单例） ────────────────────────────

/// 记录已打开的设置浮层，重复触发时不再叠第二层
struct SettingsDialog {
    view: Option<Entity<SettingsView>>,
}

impl Global for SettingsDialog {}

/// 在**主窗口内**居中打开设置界面。
///
/// 这里刻意不再单独开窗口：新窗口的位置由系统决定（gpui 的 `default_bounds`
/// 在拿不到活动窗口时会落到 (0,0)，表现就是"贴着屏幕左上角"），而且跨屏时
/// 更不可控。改成主窗口里的居中浮层后，"相对软件主界面居中"是天然的——
/// 对话框的位置本来就是按当前窗口视口算的。
///
/// 尺寸：宽度取 [`layout::window_width`]（与之前一致），高度取
/// [`layout::INITIAL_H`] 并按视口高度收窄；内容超出时靠内容列滚动。
pub fn open_settings(window: &mut Window, cx: &mut App) {
    if let Some(state) = cx.try_global::<SettingsDialog>()
        && state.view.is_some()
    {
        // 已经开着：只把焦点给搜索框，不再叠一层
        if let Some(view) = state.view.clone() {
            view.update(cx, |view, cx| view.focus(window, cx));
        }
        return;
    }

    // 与搜索面板同理：视图只创建一次（build 闭包在浮层重建时会重放），
    // 否则输入框会被不断重建，打进去的字留不下来。
    let view = cx.new(|cx| SettingsView::new(window, cx));
    cx.set_global(SettingsDialog {
        view: Some(view.clone()),
    });
    // 只在本次打开的首次 build 里给焦点（gpui-kit 0.8 的契约）
    let initial_focus = Cell::new(true);

    let width = px(layout::window_width());
    let height = px(layout::INITIAL_H);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            view.update(cx, |view, cx| view.focus(window, cx));
        }
        let view = view.clone();
        // 对话框默认水平居中、顶部留 1/10 视口；这里按视口高度算出
        // 上边距，让它在垂直方向也居中。
        let margin_top = ((window.viewport_size().height - height) / 2.).max(px(0.));
        let h = height.min((window.viewport_size().height - px(48.)).max(px(220.)));
        dialog
            .w(width.min(window.viewport_size().width - px(32.)))
            .h(h)
            .margin_top(margin_top)
            // 设置面板是"原地出现"的，从窗口顶部滑下来（默认 SlideDown）会让人
            // 误以为它跟窗口有关系；淡入更贴合"浮在主界面上方"的定位。
            // 关闭仍是即时的，且尊重系统的"减弱动态效果"设置。
            .entrance(DialogEntrance::Fade)
            .close_button(true)
            .overlay(true)
            .overlay_closable(true)
            .on_close(|_, _, cx| {
                cx.set_global(SettingsDialog { view: None });
            })
            // `min_h_0` 是能不能滚动的关键：flex 子项的 `min-height` 默认是
            // `auto`，会被内容撑开。这一层撑开后，视图里的滚动区拿到的
            // "可视高度"就等于内容高度——永远不需要滚，超出部分被对话框的
            // `overflow_hidden` 直接裁掉（表现：下面的设置项根本看不到）。
            .content(move |content, _, _| content.min_h_0().child(view.clone()))
    });
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/settings.rs"]
mod tests;
