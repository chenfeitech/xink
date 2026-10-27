//! 标签面板：聚合库内 `#tag`，点击展开带该标签的笔记。
//!
//! 设计见 `docs/index-panels-design.md`。与反链面板同构：数据读索引、
//! 刷新靠订阅 `IndexChanged`、点击走 `tabs::open_note_at`。

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::*;

use crate::app::AppState;
use crate::core::ai::tasks::{self, TaskKind};
use crate::core::index::IndexChanged;
use crate::ui::ai::plan;
use crate::ui::tabs;

pub struct TagsPanel {
    focus_handle: FocusHandle,
    /// 当前展开的标签；`None` 表示只显示标签云
    selected: Option<String>,
    /// 批量任务入口的提示（如"没有可继续的任务"）
    notice: Option<String>,
}

impl TagsPanel {
    /// 打开库级计划面板（M3-P1）
    ///
    /// 候选集合由面板自己按 `kind` 决定：改 / 删标签是"带这个标签的"，
    /// 加标签是"全库挑"（逐条取消勾选）。
    fn open_plan(&mut self, kind: TaskKind, window: &mut Window, cx: &mut Context<Self>) {
        let tag = match kind {
            // 加哪个标签要用户现场填，面板会先问
            TaskKind::AddTag => String::new(),
            _ => match &self.selected {
                Some(tag) => tag.clone(),
                None => return,
            },
        };
        let Some(root) = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root())
        else {
            return;
        };
        plan::open(root, kind, &tag, window, cx);
    }
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 与反链同理：订阅只做一次，放 render 里会每帧新增订阅
        if let Some(state) = cx.try_global::<AppState>() {
            let index = state.notes.clone();
            cx.subscribe(&index, |_, _, _: &IndexChanged, cx| cx.notify())
                .detach();
        }
        Self {
            focus_handle: cx.focus_handle(),
            selected: None,
            notice: None,
        }
    }

    /// 继续上次没跑完的批量任务（M3-P1）
    ///
    /// 点击时才读盘：任务列表会变，缓存住就会显示成"没有"，不如每次现看。
    fn resume_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = cx
            .try_global::<AppState>()
            .and_then(|state| state.vaults.active_root())
        else {
            return;
        };
        let pending = tasks::list(&root)
            .into_iter()
            .find(|task| !task.is_finished());
        match pending {
            Some(task) => {
                self.notice = None;
                plan::open_existing(root, task, window, cx);
            }
            None => {
                self.notice = Some("没有没跑完的批量任务".into());
                cx.notify();
            }
        }
    }
}

impl BasePanel for TagsPanel {
    fn panel_name(&self) -> &'static str {
        "TagsPanel"
    }
}

impl Panel for TagsPanel {
    fn title(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "标签"
    }
}

impl EventEmitter<PanelEvent> for TagsPanel {}

impl Focusable for TagsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TagsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = cx.try_global::<AppState>() else {
            return div().id("tags-panel").size_full();
        };
        let index = state.notes.clone();
        let index = index.read(cx);
        let muted = cx.theme().muted_foreground;
        let selected = self.selected.clone();

        if index.is_empty() {
            return div()
                .id("tags-panel")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child("索引还在构建…");
        }

        // 展开某个标签：列出带它的笔记
        if let Some(tag) = selected {
            let paths = index.notes_with_tag(&tag);
            return div()
                .id("tags-panel")
                .size_full()
                .overflow_y_scroll()
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("tags-back")
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(cx.theme().primary)
                        .cursor_pointer()
                        .child(format!("← #{tag}"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.selected = None;
                            cx.notify();
                        })),
                )
                // 库级批量动作的入口（M3-P1）：候选集合就是带这个标签的笔记，
                // 不需要用户再挑一遍
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .child(
                            Button::new("tags-retag")
                                .small()
                                .ghost()
                                .label("改名")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_plan(TaskKind::Retag, window, cx);
                                })),
                        )
                        .child(
                            Button::new("tags-remove")
                                .small()
                                .ghost()
                                .label("删除")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_plan(TaskKind::RemoveTag, window, cx);
                                })),
                        )
                        .child(div().text_xs().text_color(muted).child("全库批量")),
                )
                .children(paths.into_iter().enumerate().map(|(ix, path)| {
                    let title = index
                        .note(&path)
                        .map(|note| note.title.clone())
                        .unwrap_or_else(|| {
                            path.file_stem()
                                .map(|stem| stem.to_string_lossy().to_string())
                                .unwrap_or_default()
                        });
                    let target = path.clone();
                    div()
                        .id(("tag-note", ix))
                        .px_2()
                        .py_1()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                        .child(title)
                        .on_click(move |_, window, cx| {
                            tabs::open_note_at(target.clone(), None, window, cx);
                        })
                }));
        }

        let tags = index.tags();
        if tags.is_empty() {
            return div()
                .id("tags-panel")
                .size_full()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child("库里还没有标签：在笔记里写 #标签");
        }

        div()
            .id("tags-panel")
            .size_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            // 加标签的候选是"全库挑"，因此入口在标签云这一层而不是展开态
            .child(
                div()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("tags-add")
                            .small()
                            .ghost()
                            .label("批量加标签")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_plan(TaskKind::AddTag, window, cx);
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("全库挑，计划里可逐条取消"),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("tags-resume")
                            .small()
                            .ghost()
                            .label("继续上次")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.resume_task(window, cx);
                            })),
                    )
                    .children(
                        self.notice
                            .as_ref()
                            .map(|notice| div().text_xs().text_color(muted).child(notice.clone())),
                    ),
            )
            .children(tags.into_iter().enumerate().map(|(ix, (name, count))| {
                let tag = name.clone();
                div()
                    .id(("tag-item", ix))
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(cx.theme().accent.opacity(0.12)))
                    .child(format!("#{name}"))
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(tag.clone());
                        cx.notify();
                    }))
            }))
    }
}
