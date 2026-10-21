//! 全文搜索面板（Cmd+Shift+F）。
//!
//! 与命令面板同一套承载方式（`Root::open_dialog` + 自绘内容）：
//! 输入框 + 结果列表，输入防抖 200ms，检索在后台线程做（读盘是同步 IO）。
//! 点击结果走 `tabs::open_note_at` 跳到命中行。

use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::WindowExt;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::search::{Hit, search_files};
use crate::ui::tabs;

/// 输入防抖：连续输入时只搜最后一次
const DEBOUNCE: Duration = Duration::from_millis(200);
/// 结果上限
const MAX_RESULTS: usize = 100;

pub struct SearchView {
    input: Entity<InputState>,
    hits: Vec<Hit>,
    query: String,
    searching: bool,
    /// 防抖任务；每次输入都会覆盖（旧任务被 drop 即取消）
    task: Option<Task<()>>,
}

impl SearchView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索笔记内容…"));
        // 订阅只能挂一次：写在 `render` 里会每帧多挂一个，
        // 于是同一次输入被处理 N 次（N 随重绘次数增长）。
        cx.subscribe_in(
            &input,
            window,
            move |this: &mut Self, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    let query = this.input.read(cx).value().to_string();
                    this.run_search(query, window, cx);
                }
            },
        )
        .detach();

        Self {
            input,
            hits: Vec::new(),
            query: String::new(),
            searching: false,
            task: None,
        }
    }

    fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
    }

    /// 输入变化 → 防抖 → 后台检索
    fn run_search(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        self.query = query.clone();
        if query.trim().is_empty() {
            self.hits.clear();
            self.searching = false;
            self.task = None;
            cx.notify();
            return;
        }

        let files: Vec<PathBuf> = match cx.try_global::<AppState>() {
            Some(state) => {
                let index = state.notes.read(cx);
                index.notes().map(|note| note.path.clone()).collect()
            }
            None => Vec::new(),
        };
        self.searching = true;
        let task = cx.background_spawn(async move { search_files(&files, &query, MAX_RESULTS) });
        // 防抖：连续输入只跑最后一次（旧任务被覆盖即取消）
        let delay = cx.background_executor().timer(DEBOUNCE);
        self.task = Some(cx.spawn_in(window, async move |this, window| {
            delay.await;
            let hits = task.await;
            _ = this.update_in(window, |this, _window, cx| {
                this.hits = hits;
                this.searching = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for SearchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let hits: Vec<(PathBuf, usize, String)> = self
            .hits
            .iter()
            .map(|hit| (hit.path.clone(), hit.line, hit.text.clone()))
            .collect();

        let mut view = div()
            .id("search-view")
            .size_full()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.input).w_full());

        if self.searching {
            view = view.child(div().text_sm().text_color(muted).child("正在搜索…"));
        } else if !self.query.trim().is_empty() && hits.is_empty() {
            view = view.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("没有找到匹配的内容"),
            );
        }

        view = view.child(
            div()
                .id("search-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .children(
                    hits.into_iter()
                        .enumerate()
                        .map(|(ix, (path, line, text))| {
                            let name = path
                                .file_stem()
                                .map(|stem| stem.to_string_lossy().to_string())
                                .unwrap_or_default();
                            div()
                                .id(("search-hit", ix))
                                .px_2()
                                .py_1()
                                .text_sm()
                                .cursor_pointer()
                                .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                                .child(div().font_weight(gpui_kit::FontWeight::BOLD).child(name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("第 {} 行 · {text}", line + 1)),
                                )
                                .on_click(move |_, window, cx| {
                                    crate::ui::search::close(window, cx);
                                    tabs::open_note_at(path.clone(), Some(line), window, cx);
                                })
                        }),
                ),
        );
        view
    }
}

/// 打开全文搜索面板（Cmd+Shift+F）
pub fn open_search(window: &mut Window, cx: &mut App) {
    // 同命令面板：视图只能创建一次（build 闭包在浮层重建时会重放），
    // 否则输入框会被不断重建，打进去的字留不下来。
    let view = cx.new(|cx| SearchView::new(window, cx));
    // 只在本次打开的首次 build 里给焦点（gpui-kit 0.8 的契约）
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            view.update(cx, |view, cx| view.focus(window, cx));
        }
        let view = view.clone();
        dialog
            .w(px(560.))
            .h(px(480.))
            .margin_top(px(60.))
            .close_button(false)
            .overlay(true)
            .overlay_closable(true)
            .content(move |content, _, _| content.child(view.clone()))
    });
}

/// 关闭搜索面板
fn close(window: &mut Window, cx: &mut App) {
    window.close_dialog(cx);
}
