//! 大纲（TOC）面板：按层级展示当前笔记的标题，点击跳到对应行。
//!
//! 设计见 `docs/outline-design.md`。两条约定：
//! 1. **数据源是编辑器实时解析的结果**，不读索引——大纲必须跟手，
//!    而索引是"保存之后"的语义；
//! 2. **跳转只走 `tabs::open_note_at`**，不自己开标签，
//!    因此点击已在编辑的笔记不会产生第二个标签。

use std::path::{Path, PathBuf};

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::markdown::{DocFacts, parse_facts};
use crate::ui::tabs;

/// 大纲里条目缩进的最大层级：`######` 之后不再继续缩进，否则文本区太窄
const MAX_INDENT_LEVEL: u8 = 6;

/// 一条大纲条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// 标题层级 1–6
    pub level: u8,
    pub text: String,
    /// 0-based 行号，供跳转
    pub line: usize,
}

/// 当前笔记的大纲（编辑器的派生状态，不落盘）
#[derive(Debug, Default, Clone)]
pub struct OutlineState {
    /// 当前笔记；`None` 表示没有活动笔记
    pub path: Option<PathBuf>,
    pub items: Vec<OutlineItem>,
}

impl OutlineState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否为某篇笔记重新填充条目
    fn replace(&mut self, path: PathBuf, items: Vec<OutlineItem>) {
        self.path = Some(path);
        self.items = items;
    }

    /// 清空（关闭标签 / 变成空白页时）
    pub fn clear(&mut self) {
        self.path = None;
        self.items.clear();
    }
}

/// 由解析结果生成大纲条目。
///
/// 抽成纯函数是为了可测：它决定了"哪些标题会出现、行号对不对"。
pub fn outline_items(facts: &DocFacts) -> Vec<OutlineItem> {
    facts
        .headings
        .iter()
        .map(|heading| OutlineItem {
            level: heading.level,
            text: heading.text.clone(),
            line: heading.line,
        })
        .collect()
}

/// 编辑器推送当前笔记正文，更新大纲（在编辑与切标签时调用）。
pub fn update_outline(path: &Path, text: &str, cx: &mut App) {
    let Some(outline) = cx
        .try_global::<AppState>()
        .map(|state| state.outline.clone())
    else {
        return;
    };
    let items = outline_items(&parse_facts(text));
    outline.update(cx, |state, cx| {
        state.replace(path.to_path_buf(), items);
        cx.notify();
    });
}

/// 清空大纲（笔记被关闭 / 编辑区变成空白页时）
pub fn clear_outline(cx: &mut App) {
    let Some(outline) = cx
        .try_global::<AppState>()
        .map(|state| state.outline.clone())
    else {
        return;
    };
    outline.update(cx, |state, cx| {
        state.clear();
        cx.notify();
    });
}

/// 大纲面板
pub struct OutlinePanel {
    focus_handle: FocusHandle,
}

impl OutlinePanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl BasePanel for OutlinePanel {
    fn panel_name(&self) -> &'static str {
        "OutlinePanel"
    }
}

impl Panel for OutlinePanel {
    fn title(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "大纲"
    }
}

impl EventEmitter<PanelEvent> for OutlinePanel {}

impl Focusable for OutlinePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        // 面板本身不接受输入，但焦点句柄要存在：面板系统会在切换 tab 时用到
        self.focus_handle.clone()
    }
}

impl Render for OutlinePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = cx.try_global::<AppState>() else {
            return div().id("outline-panel").size_full();
        };
        let state = state.outline.read(cx);
        let muted = cx.theme().muted_foreground;

        if state.items.is_empty() {
            return div()
                .id("outline-panel")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child(if state.path.is_some() {
                    "这篇笔记还没有标题：以 # 开头写一个"
                } else {
                    "没有打开的笔记"
                });
        }

        let path = state.path.clone().unwrap_or_default();
        div()
            .id("outline-panel")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .children(state.items.iter().map(|item| {
                let line = item.line;
                let level = item.level;
                let indent = px(12. * (level.saturating_sub(1).min(MAX_INDENT_LEVEL)) as f32);
                let text = item.text.clone();
                let target = path.clone();
                div()
                    .id(("outline-item", item.line))
                    .pl(indent)
                    .py_0p5()
                    .text_sm()
                    // 层级越高越淡，形成自然的层次感
                    .text_color(if level <= 2 {
                        cx.theme().foreground
                    } else {
                        muted
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                    .child(text)
                    .on_click(move |_, window, cx| {
                        if !target.as_os_str().is_empty() {
                            tabs::open_note_at(target.clone(), Some(line), window, cx);
                        }
                    })
            }))
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/outline.rs"]
mod tests;
