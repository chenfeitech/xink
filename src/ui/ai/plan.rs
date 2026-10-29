//! 库级任务的执行计划面板（M3-P1）
//!
//! 设计见 `docs/ai/ai-m3-library-tasks-design.md`。三条硬约束：
//! 1. **计划只读**：面板只展示"将要发生什么"，写盘只在用户点「执行」之后；
//! 2. **逐条可取消**：勾选框决定这一篇改不改，跳过的显示原因且勾不动；
//! 3. **可中断**：后台批量跑，`Arc<AtomicBool>` 取消，每篇结果立刻落 `task.json`，
//!    已改的保持已改，下次从 `cursor` 继续（不重跑）。

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Sizable;
use gpui_kit::component::WindowExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::*;

use crate::app::{AppState, index_note_saved};
use crate::core::ai::snapshot::{SNAPSHOT_DIR, write_snapshot};
use crate::core::ai::tasks::Task as BatchTask;
use crate::core::ai::tasks::{self, BATCH_SIZE, ItemState, TagOutcome, TaskKind, TaskStatus};
use crate::core::note::atomic_write;
use crate::state::workspace::app_data_dir;
use crate::ui::tokens;

/// 浮层尺寸：计划可能上百行，宁可大一点
const WIDTH: f32 = 660.;
const HEIGHT: f32 = 560.;
/// 列表区的最大高度：超出由它自己滚，底部操作区始终可见
const LIST_MAX_H: f32 = 360.;

/// 面板处在哪个阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// 还缺一个"改成什么"（只有改标签会走到这里）
    Ask,
    /// 计划已生成，可以勾选与执行
    Ready,
    /// 正在批量执行
    Running,
}

pub struct PlanView {
    /// 库根：任务落在这里的 `.xink/agent/tasks`
    root: PathBuf,
    task: BatchTask,
    /// 每条是否勾选（与 `task.items` 同序）；跳过的恒为 false
    checked: Vec<bool>,
    phase: Phase,
    /// 「改成什么」的输入框（只有改标签用得上，但始终建着，省一套分支）
    input: Entity<InputState>,
    notice: Option<String>,
    /// 后台批量的取消标志
    cancel: Arc<AtomicBool>,
    running: bool,
}

/// 打开计划面板
///
/// `target` 是入口处已经确定的那个标签；改标签要改成什么、加标签要加什么，
/// 都还没定，因此传空串由面板先问一句（[`Phase::Ask`]）。
///
/// 与 `open_command_palette` / `open_settings` 同构：视图在 build 闭包**之外**
/// 创建一次，否则浮层重建时会换实例、输入内容被清空。
pub fn open(root: PathBuf, kind: TaskKind, target: &str, window: &mut Window, cx: &mut App) {
    let task = BatchTask::new(tasks::new_id(kind, target), kind, target, "");
    open_with(root, task, window, cx)
}

/// 打开一个**已存在**的任务（中断恢复）
///
/// 条目与 `cursor` 都从 `task.json` 读回来了，因此不再重新生成计划：
/// 已完成的保持已完成，"继续"从下一个待处理开始。
pub fn open_existing(root: PathBuf, task: BatchTask, window: &mut Window, cx: &mut App) {
    open_with(root, task, window, cx)
}

fn open_with(root: PathBuf, task: BatchTask, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| PlanView::new(root, task, window, cx));
    // 只在首次 build 里给焦点（gpui-kit 0.8 的契约）
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            view.update(cx, |view, cx| view.focus(window, cx));
        }
        let view = view.clone();
        let height = px(HEIGHT).min((window.viewport_size().height - px(48.)).max(px(220.)));
        dialog
            .w(px(WIDTH).min(window.viewport_size().width - px(32.)))
            .h(height)
            .margin_top(((window.viewport_size().height - height) / 2.).max(px(0.)))
            .close_button(false)
            .overlay(true)
            .overlay_closable(true)
            .content(move |content, _, _| content.child(view.clone()))
    });
}

impl PlanView {
    fn new(root: PathBuf, task: BatchTask, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = match task.kind {
            TaskKind::Retag => "改成什么标签（不带 #）",
            TaskKind::AddTag if task.target.is_empty() => "要加什么标签（不带 #）",
            _ => "",
        };
        let input = cx.new(|cx| {
            let state = InputState::new(window, cx);
            if placeholder.is_empty() {
                state
            } else {
                state.placeholder(placeholder)
            }
        });
        // 回车即生成计划：与命令面板、设置里的输入框一致
        cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(
                ev,
                InputEvent::PressEnter {
                    secondary: false,
                    ..
                }
            ) {
                this.generate(window, cx);
            }
        })
        .detach();

        // 恢复的任务自带条目：直接进 Ready，勾选只保留还没处理的
        let resumed = !task.items.is_empty();
        let checked = task
            .items
            .iter()
            .map(|item| !item.state.is_finished())
            .collect();

        let mut this = Self {
            root,
            task,
            checked,
            phase: Phase::Ask,
            input,
            notice: None,
            cancel: Arc::new(AtomicBool::new(false)),
            running: false,
        };
        if resumed {
            this.phase = Phase::Ready;
            this.notice = Some(format!(
                "继续上次：还剩 {} 篇待处理（已改的不重跑）",
                this.task.pending()
            ));
        } else {
            // 不缺信息（标签名已定）就进来直接生成计划；改标签与"加哪个还没填"都要先问
            let needs_input = this.task.kind == TaskKind::Retag || this.task.target.is_empty();
            if !needs_input {
                this.generate(window, cx);
            }
        }
        this
    }

    fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
    }

    /// 候选集合：加标签是"全库挑"，改 / 删标签是"带这个标签的那些"
    fn candidates(&self, cx: &App) -> Vec<PathBuf> {
        let Some(state) = cx.try_global::<AppState>() else {
            return Vec::new();
        };
        let index = state.notes.read(cx);
        match self.task.kind {
            TaskKind::AddTag => index.notes().map(|note| note.path.clone()).collect(),
            TaskKind::Retag | TaskKind::RemoveTag => index.notes_with_tag(&self.task.target),
        }
    }

    /// 生成计划（后台读盘，只读不写）
    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let typed = self
            .input
            .read(cx)
            .value()
            .trim()
            .trim_start_matches('#')
            .to_string();
        // 加标签：要加哪个标签还没定，先在面板里问
        if self.task.target.is_empty() {
            if !valid_tag(&typed) {
                self.notice = Some("先填一个标签名（不带 #、不能有空格）".into());
                cx.notify();
                return;
            }
            self.task.target = typed;
        } else if self.task.kind == TaskKind::Retag {
            // 改标签：新名字在这里补上
            if !valid_tag(&typed) {
                self.notice = Some("先填一个新标签名（不带 #、不能有空格）".into());
                cx.notify();
                return;
            }
            if typed == self.task.target {
                self.notice = Some("新标签和原来的一样".into());
                cx.notify();
                return;
            }
            self.task.new_tag = typed;
        }

        let candidates = self.candidates(cx);
        if candidates.is_empty() {
            self.notice = Some(match self.task.kind {
                TaskKind::AddTag => "库里还没有笔记".into(),
                _ => format!("库里没有带 #{} 的笔记", self.task.target),
            });
            self.phase = Phase::Ready;
            cx.notify();
            return;
        }

        self.notice = Some("正在生成计划…".into());
        self.phase = Phase::Ready;
        let kind = self.task.kind;
        let target = self.task.target.clone();
        let new_tag = self.task.new_tag.clone();
        let job =
            cx.background_spawn(async move { tasks::plan(kind, &target, &new_tag, &candidates) });

        cx.spawn_in(window, async move |this, window| {
            let items = job.await;
            _ = this.update_in(window, |this, _window, cx| {
                this.checked = items
                    .iter()
                    .map(|item| !matches!(item.state, ItemState::Skipped(_) | ItemState::Failed(_)))
                    .collect();
                this.task.items = items;
                this.task.cursor = 0;
                this.notice = Some(format!(
                    "计划已生成：{} 篇可改，其余已跳过。确认后点「执行」",
                    this.checked.iter().filter(|c| **c).count()
                ));
                cx.notify();
            });
        })
        .detach();
    }

    /// 执行一批：后台线程写盘，逐条回传结果
    fn start_batch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let batch: Vec<usize> = self
            .task
            .next_batch(BATCH_SIZE)
            .into_iter()
            .filter(|ix| self.checked.get(*ix).copied().unwrap_or(false))
            .collect();
        if batch.is_empty() {
            self.notice = Some("没有勾选任何一篇".into());
            cx.notify();
            return;
        }

        self.running = true;
        self.phase = Phase::Running;
        self.cancel.store(false, Ordering::Relaxed);
        self.task.status = TaskStatus::Running;
        self.notice = None;

        let kind = self.task.kind;
        let target = self.task.target.clone();
        let new_tag = self.task.new_tag.clone();
        let paths: Vec<PathBuf> = batch
            .iter()
            .map(|ix| self.task.items[*ix].path.clone())
            .collect();
        let snapshot_dir = app_data_dir().join(SNAPSHOT_DIR);
        let cancel = self.cancel.clone();
        let (tx, rx) = flume::unbounded::<(usize, ItemState)>();

        cx.background_spawn(async move {
            for (seq, ix) in batch.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let path = &paths[seq];
                let state = match std::fs::read_to_string(path) {
                    Err(err) => ItemState::Failed(format!("读不到：{err}")),
                    Ok(before) => {
                        let name = file_name_of(path);
                        if let Err(err) = write_snapshot(&snapshot_dir, &name, &before) {
                            // 快照写不出来就不写这篇：没有回退手段的改动不该发生
                            ItemState::Failed(format!("快照失败：{err}"))
                        } else {
                            match tasks::apply_text(kind, &before, &target, &new_tag) {
                                TagOutcome::Changed(after) => match atomic_write(path, &after) {
                                    Ok(()) => ItemState::Done,
                                    Err(err) => ItemState::Failed(format!("写入失败：{err}")),
                                },
                                // 计划之后内容又变了：不猜，跳过
                                _ => ItemState::Skipped("内容已变，跳过".into()),
                            }
                        }
                    }
                };
                if tx.send((*ix, state)).is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.spawn_in(window, async move |this, window| {
            while let Ok((ix, state)) = rx.recv_async().await {
                _ = this.update_in(window, |this, _window, cx| {
                    this.apply_item(ix, state, cx);
                });
            }
            _ = this.update_in(window, |this, _window, cx| this.finish_batch(cx));
        })
        .detach();
    }

    /// 单条结果落地：改状态 → 落盘 → 刷新索引
    fn apply_item(&mut self, ix: usize, state: ItemState, cx: &mut Context<Self>) {
        let Some(item) = self.task.items.get_mut(ix) else {
            return;
        };
        let done = matches!(state, ItemState::Done);
        item.state = state;
        self.task.cursor = self.task.cursor.max(ix + 1);
        // 处理完就取消勾选：再点「执行」不会重复改它
        if let Some(checked) = self.checked.get_mut(ix) {
            *checked = false;
        }
        if done {
            index_note_saved(&item.path, cx);
        }
        // 每篇都落一次：中途崩了也不会丢"改到哪了"
        if let Err(err) = tasks::save(&self.root, &self.task) {
            eprintln!("[xink] 任务落盘失败：{err}");
        }
        cx.notify();
    }

    fn finish_batch(&mut self, cx: &mut Context<Self>) {
        self.running = false;
        self.phase = Phase::Ready;
        let (done, skipped, failed) = self.task.stats();
        self.task.status = if self.task.is_finished() {
            TaskStatus::Done
        } else {
            TaskStatus::Paused
        };
        if let Err(err) = tasks::save(&self.root, &self.task) {
            eprintln!("[xink] 任务落盘失败：{err}");
        }
        self.notice = Some(if self.task.is_finished() {
            format!("完成：改了 {done} 篇，跳过 {skipped}，失败 {failed}")
        } else {
            format!(
                "已改 {done} 篇（跳过 {skipped}、失败 {failed}），还剩 {} 篇：继续点「执行」",
                self.task.pending()
            )
        });
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.notice = Some("正在停止…（已改的保持已改）".into());
        cx.notify();
    }

    fn toggle(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(item) = self.task.items.get(ix) else {
            return;
        };
        // 跳过与失败的勾不动：它们的结局已经定了
        if item.state.is_finished() {
            return;
        }
        self.checked[ix] = !self.checked[ix];
        cx.notify();
    }
}

/// 标签名合法：非空、不含空白（`#` 允许用户在前面写，已在调用处剥掉）
fn valid_tag(tag: &str) -> bool {
    !tag.is_empty() && !tag.contains(char::is_whitespace)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "note.md".into())
}

impl Render for PlanView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let primary = cx.theme().primary;
        let warning = cx.theme().warning;
        let (done, skipped, failed) = self.task.stats();

        let mut view = div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_b_1()
                    .border_color(border)
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(600.))
                            .child(self.task.title()),
                    )
                    .child(div().text_xs().text_color(muted).child(format!(
                        "{} · 共 {} 篇 · 待处理 {} · 已完成 {done} · 跳过 {skipped} · 失败 {failed}",
                        self.task.status.label(),
                        self.task.items.len(),
                        self.task.pending(),
                    ))),
            );

        // 改标签：先问"改成什么"
        if self.phase == Phase::Ask {
            view = view.child(
                div()
                    .flex_none()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.input).small().w_full()),
                    )
                    .child(
                        Button::new("plan-generate")
                            .small()
                            .label("生成计划")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.generate(window, cx);
                            })),
                    ),
            );
        }

        // 计划列表
        let mut list = div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .overflow_y_scrollbar();
        if self.task.items.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .text_sm()
                    .text_color(muted)
                    .child(self.notice.clone().unwrap_or_else(|| "还没有计划".into())),
            );
        }
        for (ix, item) in self.task.items.iter().enumerate() {
            let checked = self.checked.get(ix).copied().unwrap_or(false);
            let name = item
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| item.path.to_string_lossy().to_string());
            let finished = item.state.is_finished();

            let mut row = div()
                .id(("plan-item", ix))
                .px(px(tokens::SPACE_SM))
                .py(px(tokens::SPACE_XS))
                .flex()
                .flex_col()
                .gap(px(tokens::SPACE_XS))
                // 以前有圆角没底色，看着不像卡片
                .rounded(tokens::card_radius(cx))
                .bg(tokens::card_bg(cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(if finished { muted } else { primary })
                                .child(if checked { "☑" } else { "☐" }),
                        )
                        .child(div().flex_1().min_w_0().text_sm().child(name))
                        .child(div().text_xs().text_color(muted).child(item.state.label())),
                );

            match &item.state {
                ItemState::Skipped(_) | ItemState::Failed(_) => {
                    row = row.child(
                        div()
                            .text_xs()
                            .text_color(if matches!(item.state, ItemState::Failed(_)) {
                                warning
                            } else {
                                muted
                            })
                            .child(item.state.label()),
                    );
                }
                _ => {
                    for (before, after) in &item.preview {
                        row = row.child(div().text_xs().text_color(muted).child(
                            if before.is_empty() {
                                format!("+ {after}")
                            } else if after.is_empty() {
                                format!("- {before}")
                            } else {
                                format!("- {before} → + {after}")
                            },
                        ));
                    }
                }
            }

            if !finished {
                row = row
                    .cursor_pointer()
                    // 可勾选却一直没有 hover 反馈
                    .hover(|s| s.bg(tokens::hover_bg(cx)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle(ix, cx);
                    }));
            }
            list = list.child(row);
        }
        view = view.child(list.max_h(px(LIST_MAX_H)));

        // 底部：执行 / 停止 / 关闭 + 提示
        let mut actions = div().flex_none().p_3().flex().items_center().gap_2();
        if self.running {
            actions = actions.child(
                Button::new("plan-stop")
                    .small()
                    .outline()
                    .label("停止")
                    .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
            );
        } else {
            let pending = self.checked.iter().filter(|c| **c).count();
            actions = actions.child(
                Button::new("plan-run")
                    .small()
                    .label(if self.task.status == TaskStatus::Paused {
                        format!("继续（{pending}）")
                    } else {
                        format!("执行（{pending}）")
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.start_batch(window, cx);
                    })),
            );
        }
        actions = actions.child(
            Button::new("plan-close")
                .small()
                .ghost()
                .label("关闭")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        );
        if let Some(notice) = &self.notice {
            actions = actions.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(muted)
                    .child(notice.clone()),
            );
        }
        view = view.child(
            div()
                .flex_none()
                .border_t_1()
                .border_color(border)
                .child(actions)
                .child(
                    div()
                        .px_3()
                        .pb_3()
                        .text_xs()
                        .text_color(muted)
                        .child(format!(
                            "每篇改前快照到 …/xink/{SNAPSHOT_DIR}（保留最近 {} 个，批量较大时会挤掉旧的）",
                            crate::core::ai::snapshot::MAX_SNAPSHOTS
                        )),
                ),
        );
        view
    }
}
