//! 反链面板：显示"哪些笔记引用了当前笔记"。
//!
//! 设计见 `docs/index-panels-design.md`。数据一律读索引，不自己扫盘；
//! 刷新靠订阅 `IndexChanged` 与 `ActiveDocChanged`，不做轮询；
//! 点击一律走 `tabs::open_note_at`。

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::*;

use crate::app::{ActiveDocChanged, AppState};
use crate::core::index::IndexChanged;
use crate::ui::tabs;
use crate::ui::tokens;

pub struct BacklinkPanel {
    focus_handle: FocusHandle,
}

impl BacklinkPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 订阅只在构造时做一次：放在 render 里会每帧新增一个订阅
        if let Some(state) = cx.try_global::<AppState>() {
            let index = state.notes.clone();
            let preview = state.preview.clone();
            cx.subscribe(&index, |_, _, _: &IndexChanged, cx| cx.notify())
                .detach();
            cx.subscribe(&preview, |_, _, _: &ActiveDocChanged, cx| cx.notify())
                .detach();
        }
        Self {
            focus_handle: cx.focus_handle(),
        }
    }
}

impl BasePanel for BacklinkPanel {
    fn panel_name(&self) -> &'static str {
        "BacklinkPanel"
    }
}

impl Panel for BacklinkPanel {
    fn title(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "反链"
    }
}

impl EventEmitter<PanelEvent> for BacklinkPanel {}

impl Focusable for BacklinkPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for BacklinkPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 订阅只做"该重绘了"的通知：数据始终在 render 时现读，
        // 这样索引变动与切换笔记都能自动跟上，回调里也不需要复制数据
        let Some(state) = cx.try_global::<AppState>() else {
            return div().id("backlink-panel").size_full();
        };
        let index = state.notes.clone();
        let current = state.tabs.active().cloned();

        let muted = cx.theme().muted_foreground;
        let Some(current) = current else {
            return div()
                .id("backlink-panel")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child("没有打开的笔记");
        };

        let index = index.read(cx);
        if index.is_empty() {
            return div()
                .id("backlink-panel")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child("索引还在构建…");
        }

        let refs = index.backlinks(&current);
        if refs.is_empty() {
            return div()
                .id("backlink-panel")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child("还没有笔记引用它");
        }

        div()
            .id("backlink-panel")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .children(refs.into_iter().map(|link| {
                // 显示来源笔记的标题：比文件名友好，且不额外读文件
                let title = index
                    .note(&link.from)
                    .map(|note| note.title.clone())
                    .unwrap_or_else(|| {
                        link.from
                            .file_stem()
                            .map(|stem| stem.to_string_lossy().to_string())
                            .unwrap_or_default()
                    });
                let from = link.from.clone();
                let line = link.line;
                div()
                    .id(("backlink-item", link.line))
                    .py_1()
                    .px_2()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(tokens::hover_bg(cx)))
                    .child(title)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("第 {} 行", line + 1)),
                    )
                    .on_click(move |_, window, cx| {
                        tabs::open_note_at(from.clone(), Some(line), window, cx);
                    })
            }))
    }
}
