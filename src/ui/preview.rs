//! 右侧预览面板：渲染当前活动笔记的 Markdown

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::base::Theme as BaseTheme;
use gpui_kit::base::{ScrollbarHandle, TextView, TextViewState, TextViewStyle};
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::*;
use gpui_kit::{ListOffset, ListState};

use crate::app::{
    ActiveDocChanged, AppState, EditorScrolled, PreviewImageChanged, PreviewScrolled,
};
use crate::core::config::{LinkOpen, PreviewDensity, ScrollSyncMode};
use crate::core::highlight::{SpanStyle, highlight};
use crate::core::markdown::path_from_file_url;
use crate::core::media::is_image_path;
use crate::core::sync::{AnchorTable, GestureGuard, ScrollAnchor, Side, SyncGuard};

use std::sync::Arc;
use std::time::Duration;

/// 源码行 → 滚动比例（锚点不可用时的降级换算）
fn ratio_of_line(line: f32, table: &AnchorTable) -> f32 {
    let last = table.total_lines().saturating_sub(1).max(1) as f32;
    (line / last).clamp(0.0, 1.0)
}

pub struct PreviewPanel {
    focus_handle: FocusHandle,
    /// 当前显示的文档。切换笔记时整体替换引用，已解析过的文档不再重解析。
    doc: Option<Entity<TextViewState>>,
    /// 图片预览模式：非空时面板显示这张图片，而不是 Markdown
    image: Option<PathBuf>,
    /// 滚动同步方向锁：谁先滚谁在一段时间内独占推动同步，对端回显一律吸收
    guard: SyncGuard,
    /// 上次观测到的块数量：内容重解析 / 图片加载完成后会变，用它触发重新对齐
    last_item_count: usize,
    /// 最近一次应用的锚点：重新对齐时把它再应用一次
    last_applied: Option<ScrollAnchor>,
    /// 用户手势静默窗口：期间不写预览滚动位置，避免与滚轮 / 惯性抢。
    /// 与编辑区对称（`EditorPanel::gesture`）
    gesture: GestureGuard,
    /// 上一帧的滚动位置，用来识别"位置变了但不是我们写的" = 用户在滚
    last_scroll_y: Pixels,
    /// 有滚动发生过、但还没同步出去（"停止后对齐"模式下用）
    pending_push: bool,
    /// 帧计数：实时模式下推送降频用
    sync_tick: u8,
    /// 实时模式下最近一次推送出去的锚点（死区判定）
    last_pushed: Option<ScrollAnchor>,
    /// 上次观测到的内容高度：图片加载完成后会变，用它触发重新对齐
    last_content_h: Pixels,
    /// 上次重新对齐的时刻（冷却用）
    last_realign: Option<Instant>,
    /// 最近一次**由同步写入**滚动位置的时刻（与编辑区的 `last_write_at` 对称）
    last_write_at: Option<Instant>,
    /// 文档内容最近一次发生变化的时刻（重解析 / 图片加载 / 测到新高度）
    ///
    /// 这段时间里预览的滚动位置不可信（重解析会把它打回顶部），
    /// 一律**不推**给编辑区，否则编辑区会被拽到文章开头。
    content_changed_at: Option<Instant>,
}

impl PreviewPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            doc: None,
            image: None,
            guard: SyncGuard::default(),
            last_item_count: 0,
            last_applied: None,
            gesture: GestureGuard::default(),
            last_scroll_y: px(0.),
            pending_push: true,
            sync_tick: 0,
            last_pushed: None,
            last_content_h: px(0.),
            last_realign: None,
            last_write_at: None,
            content_changed_at: None,
        };

        let Some(state) = cx.try_global::<AppState>().map(|s| s.preview.clone()) else {
            return this;
        };

        // 笔记可能先于预览面板创建，先把已存在的活动文档接上
        this.doc = state.read(cx).active_doc.clone();
        this.image = state.read(cx).image.clone();

        cx.subscribe_in(
            &state,
            window,
            move |this, _, ev: &ActiveDocChanged, _, cx| {
                this.doc = Some(ev.0.clone());
                // 切换/编辑笔记时退出图片模式，回到 Markdown
                this.image = None;
                // 换文档了：不同步状态必须归零，否则带着上一篇的位置跳一下（R6）
                this.guard.reset();
                cx.notify();
            },
        )
        .detach();

        // 图片预览：与 Markdown 共用同一个预览面板，不再新开窗口
        cx.subscribe_in(
            &state,
            window,
            move |this, _, ev: &PreviewImageChanged, _, cx| {
                this.image = ev.0.clone();
                cx.notify();
            },
        )
        .detach();

        // 编辑器滚动 → 同步预览滚动
        cx.subscribe_in(
            &state,
            window,
            move |this, _, ev: &EditorScrolled, _, cx| {
                let EditorScrolled(anchor) = ev;
                this.apply_editor_anchor(*anchor, cx);
            },
        )
        .detach();

        // 启动预览滚动同步轮询（框架在实体移除后不再回调，循环自动停止）
        this.schedule_scroll_tick(window, cx);

        this
    }

    /// 实时模式的推送降频倍率（每 N 帧推一次）
    const SYNC_EVERY_N_FRAMES: u8 = 2;
    /// 实时模式的死区：锚点变化小于这个值不推送（单位：源码行）
    const DEAD_ZONE: f32 = 0.2;
    /// 重新对齐的冷却时间：滚动过程中内容高度一直在变，不能每帧对齐
    const REALIGN_COOLDOWN: Duration = Duration::from_millis(300);
    /// 刚被对端写过之后，这段时间内不反向推（斩断互推）
    const PUSH_SUPPRESS: Duration = Duration::from_millis(400);
    /// 文档内容变化后的静默期：这段时间预览的位置不可信，不推给编辑区
    ///
    /// 要覆盖"编辑防抖（默认 300ms）+ 重解析 + 重新测量"的完整往返，
    /// 短了会漏掉前半段，长了会吃掉用户紧接着滚预览的同步。
    const CONTENT_SETTLE: Duration = Duration::from_millis(800);

    /// 是否处于"用户正在滚预览"的静默窗口内
    fn is_user_scrolling(&self, now: Instant) -> bool {
        self.gesture.is_quiet(now)
    }

    /// 记下"我们刚写到的位置"，避免下一帧被误判成用户滚动
    fn note_written_offset(&mut self, cx: &App) {
        if let Some(doc) = self.doc.as_ref() {
            self.last_scroll_y = doc.read(cx).list_state().offset().y;
        }
        self.last_write_at = Some(Instant::now());
    }

    /// 当前文档的锚点表（由活动编辑器发布；数据文件为 `None`）
    fn anchors(&self, cx: &App) -> Option<Arc<AnchorTable>> {
        cx.try_global::<AppState>()
            .and_then(|state| state.preview.read(cx).anchors.clone())
    }

    /// 应用编辑区推送过来的锚点
    ///
    /// 有锚点表时走**块定位**（`scroll_to(ListOffset)`），完全绕开
    /// `content_size`——后者只统计已测量过的 item，虚拟列表里会低估，
    /// 是"越往下滚偏差越大"的根源（R3）。
    fn apply_editor_anchor(&mut self, anchor: ScrollAnchor, cx: &mut Context<Self>) {
        // 图片模式下没有 Markdown 可同步
        if self.image.is_some() {
            return;
        }
        // 关闭同步滚动后，编辑器滚动不再带动预览
        if !crate::app::config(cx).preview.sync_scroll {
            return;
        }
        // 用户正在滚预览（滚轮 / 触控板惯性）：让位，不写
        if self.is_user_scrolling(Instant::now()) {
            return;
        }

        let Some(doc) = self.doc.clone() else {
            return;
        };
        self.last_applied = Some(anchor);

        let ScrollAnchor::Line(line) = anchor else {
            self.apply_ratio(anchor.value(), cx);
            return;
        };

        // 只有块锚点模式才做"源码行 → 顶层块"的换算。
        // 收到行号却不在该模式（对端刚切过来、或两端模式还没同步）→ 忽略：
        // 拿行号当比例用会让预览跳到完全不相干的位置。
        if crate::app::config(cx).preview.sync_mode != ScrollSyncMode::Anchor {
            return;
        }

        let list = doc.read(cx).list_state().clone();
        if let Some(table) = self.anchors(cx)
            && !table.is_empty()
            && table.len() == list.item_count()
        {
            let (item_ix, frac) = table.locate(line);
            // 目标块还没测量时 `bounds_for_item` 返回 None（虚拟列表特性）。
            // 此时**不能写**：拿不到高度就只能把块内偏移当 0，等于把预览拽到块的
            // 开头——等它测量出来后再写，抖动就是这么来的。
            let Some(height) = list
                .bounds_for_item(item_ix)
                .map(|bounds| bounds.size.height)
                .filter(|height| *height > px(0.))
            else {
                return;
            };
            list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: frac * height,
            });
            self.note_written_offset(cx);
            self.guard.expect(anchor);
            return;
        }

        // 降级：块数对不上（解析行为有出入）→ 按比例换算，绝不硬跳。
        // 锚点表本身缺失（数据文件）时两端本来就走比例，行号在这里没有意义，忽略。
        if let Some(table) = self.anchors(cx) {
            self.apply_ratio(ratio_of_line(line, &table), cx);
        }
    }

    /// 按比例设置预览滚动位置（降级路径）
    fn apply_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else {
            return;
        };
        let target_y = {
            let st = doc.read(cx);
            let list = st.list_state();
            // 可滚动范围拿不到（没布局 / 内容不足一屏）就别写：
            // 此时任何比例都会被夹成同一个位置，看起来就是"预览自己乱跳"
            let Some(max) = self.scroll_range(list) else {
                return;
            };
            -(ratio.clamp(0.0, 1.0) * max)
        };
        doc.update(cx, |st, cx| {
            st.list_state().set_offset(point(px(0.), target_y));
            cx.notify();
        });
        self.note_written_offset(cx);
        self.guard.expect(ScrollAnchor::Ratio(ratio));
    }

    /// 这一帧的位置变化，是不是"我们自己刚写入的余波"
    ///
    /// 与编辑区 [`crate::ui::editor::EditorPanel`] 同理：虚拟列表会在布局阶段
    /// 修正刚写入的位置（未测量块被测量、`offset_in_item` 归一化到下一块），
    /// 落点与写入目标差几像素是常态。按像素判手势就会启动一轮两端互推。
    fn is_own_write(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(anchor) = self.current_anchor(cx) else {
            return false;
        };
        let tol = match anchor {
            ScrollAnchor::Line(_) => 1.5,
            ScrollAnchor::Ratio(_) => 0.002,
        };
        self.guard.is_echo(anchor, tol)
    }

    /// 当前滚动位置对应的锚点
    fn current_anchor(&self, cx: &mut Context<Self>) -> Option<ScrollAnchor> {
        let doc = self.doc.as_ref()?;
        let list = doc.read(cx).list_state().clone();

        // 比例模式（默认）：两端各按自己的可滚动范围取百分比。
        // 不依赖块是否测量，也就不存在"未测量块"导致的跳变。
        if crate::app::config(cx).preview.sync_mode != ScrollSyncMode::Anchor {
            return Some(ScrollAnchor::Ratio(self.ratio_now(&list)?));
        }

        let Some(table) = self.anchors(cx) else {
            // 数据文件：没有块 ↔ 行映射，退回比例
            return Some(ScrollAnchor::Ratio(self.ratio_now(&list)?));
        };
        // 块数对不上：解析器行为有出入，本帧按比例算，绝不乱跳
        if table.is_empty() || table.len() != list.item_count() {
            return Some(ScrollAnchor::Ratio(self.ratio_now(&list)?));
        }

        let top = list.logical_scroll_top();
        // 顶部块还没测量 → 拿不到高度。此时**不要造锚点**：把块内偏移当 0
        // 会让锚点整块整块地跳，两端互相纠正就是肉眼可见的抖动。
        let height = list
            .bounds_for_item(top.item_ix)
            .filter(|bounds| bounds.size.height > px(0.))?
            .size
            .height;
        let frac = (top.offset_in_item / height).clamp(0.0, 1.0);
        Some(ScrollAnchor::Line(table.line_of(top.item_ix, frac)))
    }

    /// 可滚动范围（内容高度 − 视口高度）
    ///
    /// 拿不到（视口还没布局）或**内容不足一屏**时返回 `None`：此时"滚动比例"
    /// 没有意义——虚拟列表还没测量出高度时 `content_size ≈ viewport`，
    /// 硬算会得到 0（或 1），把对端一下拽到文章开头（或结尾）。
    /// 这个 `None` 必须一路传上去让本帧**不产生锚点**。
    fn scroll_range(&self, list: &ListState) -> Option<Pixels> {
        let viewport_h = list.viewport_bounds().size.height;
        if viewport_h <= px(0.) {
            return None;
        }
        let max = list.content_size().height - viewport_h;
        (max > px(1.)).then_some(max)
    }

    /// 当前滚动比例（降级路径 / 数据文件）
    fn ratio_now(&self, list: &ListState) -> Option<f32> {
        let max = self.scroll_range(list)?;
        Some((-list.offset().y / max).clamp(0.0, 1.0))
    }

    /// 每帧轮询预览滚动位置，换算成锚点推给编辑器。
    ///
    /// 防循环改用 [`SyncGuard`] 方向锁：不再用固定阈值比较单值，
    /// 既避免"取整误差被当成用户滚动"引起的 ping-pong（R4），
    /// 也不会把用户的连续滚动误吸收掉。
    fn tick_scroll_sync(&mut self, cx: &mut Context<Self>) {
        // 图片模式下没有 Markdown 可同步
        if self.image.is_some() {
            return;
        }

        // 关闭同步滚动后，不再把预览滚动推给编辑器
        if !crate::app::config(cx).preview.sync_scroll {
            return;
        }

        // 识别用户手势：位置变了而我们这一帧没写过 → 用户在滚（滚轮 / 触控板惯性）
        let now = Instant::now();
        let off_now = self
            .doc
            .as_ref()
            .map(|doc| doc.read(cx).list_state().offset().y);
        if let Some(off) = off_now {
            // `is_own_write` 过滤：虚拟列表会在布局阶段修正刚写入的位置
            // （未测量块、`offset_in_item` 归一化），按像素比较会把自己写的
            // 结果当成用户手势 → 静默窗口结束后反推给编辑区 → 两端互推 = 抖
            if (off - self.last_scroll_y).abs() > px(0.5) && !self.is_own_write(cx) {
                self.gesture.note_user_movement(now);
            }
            self.last_scroll_y = off;
        }

        let mode = crate::app::config(cx).preview.sync_mode;
        // "停止后对齐"：滚动中不读不写对端，只记"待对齐"
        if mode == ScrollSyncMode::OnStop && self.is_user_scrolling(now) {
            self.pending_push = true;
            return;
        }

        // 内容变化后重新对齐：块数变（重解析）或内容高度变（图片加载完成、
        // 虚拟列表测到新高度）都会让原来的位置失效。用最近一次锚点再对齐一次，
        // 位置才不会漂。
        if let Some(doc) = self.doc.clone() {
            let list = doc.read(cx).list_state().clone();
            let count = list.item_count();
            let content_h = list.content_size().height;
            // 冷却：滚动过程中 `content_size` 会随新块被测量而不断变化，
            // 没有冷却就会每帧重新对齐 = 每帧写滚动位置 = 抖得更厉害。
            let cooled = self
                .last_realign
                .is_none_or(|at| now.duration_since(at) >= Self::REALIGN_COOLDOWN);
            if count != self.last_item_count || (content_h - self.last_content_h).abs() > px(1.) {
                // 内容刚变过（编辑防抖后重解析 / 图片加载完成 / 虚拟列表测到
                // 新高度）。这一刻预览的滚动位置是**不可信**的：重解析会
                // 把它打回顶部，而它看起来和"用户滚到了顶部"一模一样。
                // 若照样推给编辑区，编辑区就会被拽到文章开头 ——
                // "一边打字一边跳回开头"就是这么来的。
                self.content_changed_at = Some(now);
                if cooled {
                    self.last_item_count = count;
                    self.last_content_h = content_h;
                    self.last_realign = Some(now);
                    if let Some(anchor) = self.last_applied {
                        self.apply_editor_anchor(anchor, cx);
                    }
                }
            }
        }

        if mode == ScrollSyncMode::OnStop {
            // 没滚过（或已经同步完）→ 不动
            if !self.pending_push {
                return;
            }
            self.pending_push = false;
        } else {
            // 实时模式：降频（每 2 帧一次），滚动中不必每帧都让编辑区重排
            self.sync_tick = self.sync_tick.wrapping_add(1);
            if !self.sync_tick.is_multiple_of(Self::SYNC_EVERY_N_FRAMES) {
                return;
            }
        }

        let Some(anchor) = self.current_anchor(cx) else {
            return;
        };

        // 内容刚变过：预览此刻的滚动位置不可信（重解析可能把它打回顶部），
        // 推出去会把编辑区拽到文章开头。静默一拍再推。
        if let Some(at) = self.content_changed_at
            && now.duration_since(at) < Self::CONTENT_SETTLE
        {
            return;
        }

        // 实时模式的死区：亚像素级变化不推（避免两端来回微调形成抖动）
        //
        // 单位随锚点种类变：行锚点是"行"（0.2 行），比例锚点是 0~1，
        // 用同一个 0.2 等于 20% 的滚动范围，会把同步掐死（表现为"不同步"）。
        let dead_zone = match anchor {
            ScrollAnchor::Line(_) => Self::DEAD_ZONE,
            ScrollAnchor::Ratio(_) => 0.002,
        };
        if mode != ScrollSyncMode::OnStop
            && let Some(prev) = self.last_pushed
            && prev.same_kind(anchor)
            && (prev.value() - anchor.value()).abs() < dead_zone
        {
            return;
        }
        self.last_pushed = Some(anchor);

        // 与编辑区同阈值：写入是 deferred 的，落点带几像素误差是常态
        let tol = match anchor {
            ScrollAnchor::Line(_) => 1.5,
            ScrollAnchor::Ratio(_) => 0.002,
        };
        // 回显：我刚按编辑器的目标滚过 → 吸收，不再反向推
        if self.guard.is_echo(anchor, tol) {
            return;
        }
        // 刚被对端写过：别把余波反推回去（用户在滚时不拦）
        if !self.is_user_scrolling(now)
            && let Some(at) = self.last_write_at
            && now.duration_since(at) < Self::PUSH_SUPPRESS
        {
            return;
        }
        // 对端正持有方向锁 → 这次变化是对端同步的余波
        if !self.guard.claim(Side::Preview, Instant::now()) {
            return;
        }

        let debug = crate::app::config(cx).preview.debug_scroll_sync;
        cx.update_global::<AppState, _>(|state, cx| {
            state.preview.update(cx, |preview, cx| {
                if debug {
                    let mut pair = preview.debug_anchors.unwrap_or((0.0, 0.0));
                    pair.1 = anchor.value();
                    preview.debug_anchors = Some(pair);
                }
                cx.emit(PreviewScrolled(anchor));
            });
        });
    }

    /// 调度下一帧滚动同步轮询。框架在实体被移除后不再回调，循环自动停止。
    fn schedule_scroll_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, move |this, window, cx| {
            this.tick_scroll_sync(cx);
            this.schedule_scroll_tick(window, cx);
        });
    }
}

impl EventEmitter<PanelEvent> for PreviewPanel {}

impl Focusable for PreviewPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for PreviewPanel {
    fn panel_name(&self) -> &'static str {
        "PreviewPanel"
    }
}

impl Panel for PreviewPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 密度菜单与"预览"同一行：水平对齐、间距由 gap 统一控制
        let mut row = div().flex().items_center().gap_2().child("预览");

        // 调试信息：把"两端对不对得上"变成可读数字（设置里可开关）
        if let (true, Some((editor, preview))) = (
            crate::app::config(cx).preview.debug_scroll_sync,
            cx.try_global::<AppState>()
                .and_then(|state| state.preview.read(cx).debug_anchors),
        ) {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "E {editor:6.1} / P {preview:6.1} · Δ{:.1}",
                        (editor - preview).abs()
                    )),
            );
        }

        row.child(density_menu(crate::app::config(cx).preview.density))
    }
}

impl Render for PreviewPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Instant::now();
        let cfg = crate::app::config(cx);

        // 图片预览模式：复用同一个预览面板显示图片，不再为每张图开新窗口。
        // 切换/编辑笔记时会自动清掉 image，回到 Markdown。
        if let Some(path) = self.image.clone() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            return div()
                .id("preview-panel")
                .size_full()
                .flex()
                .flex_col()
                .bg(cx.theme().background)
                .child(
                    div()
                        .flex_none()
                        .p_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(name),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_2()
                        .child(
                            img(path)
                                .object_fit(ObjectFit::Contain)
                                .max_w(relative(1.))
                                .max_h(relative(1.)),
                        ),
                );
        }

        let Some(doc) = self.doc.clone() else {
            return div()
                .id("preview-panel")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child("没有打开的笔记");
        };

        // 代码块配色与编辑区同源：统一预置主题 + 当前明暗（不再走旧的 syntect 主题名）
        let highlight_theme = cfg.highlight.theme.clone();
        let dark = crate::app::is_dark(cx);
        let font = cfg.preview.font.clone();

        let density = cfg.preview.density;
        let layout = density.layout();

        // 排版样式：段间距与标题层级随密度变化；颜色沿用主题，明暗自动一致
        // `from_theme` 要 gpui_base 的 Theme（与 gpui_component 的 Theme 是不同类型），
        // 它是全局的，直接取即可——这样颜色随主题，只覆盖布局相关的几项
        // 标题：gpui-kit 0.7.3 起 `with_heading_base_font_size` /
        // `with_heading_font_size` 合并成一个 `with_heading(level)` 钩子；
        // 框架内部只在 refinement 没给 `font_size` 时才用"继承字号 × 层级系数"，
        // 因此这里把字号显式算好写回去，保持与升级前一致的标题层级。
        let heading_base = font.size * layout.heading_scale;
        let text_style = TextViewStyle::from_theme(cx.global::<BaseTheme>())
            .with_paragraph_gap(rems(layout.paragraph_gap))
            .with_heading(move |level: u8| {
                let factor = match level {
                    1 => 1.0,
                    2 => 0.87,
                    3 => 0.77,
                    4 => 0.68,
                    5 => 0.62,
                    _ => 0.58,
                };
                StyleRefinement::default().text_size(px(heading_base * factor))
            });

        // 代码块**不设**最大高度、也不开块内滚动：套一层可滚动的框等于把内容
        // "收起来"。数据文件预览（JSON / YAML / TOML 整体就是一个围栏代码块）
        // 与大段注释代码都会被截断成 420px 的小窗，要看全得在框里上下滚——
        // 这不是预览该有的行为。长就让它长，外层虚拟列表本来就能滚。
        // （这里曾加过 max_size + Overflow::Scroll，见 806a5b8，已回退）

        let view = div()
            .id("preview-panel")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .font_family(font.family.clone())
            .text_size(px(font.size))
            .line_height(px(font.size * font.line_height))
            // 字重接线：之前只应用了字体三件套，weight 配了不生效
            .font_weight(FontWeight(font.weight.value()));

        // 正文最大宽度：用户显式设置优先，其次用当前档位的默认值
        let max_width = if cfg.preview.max_width > 0 {
            cfg.preview.max_width as f32
        } else {
            layout.max_width
        };

        let view = view.child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(max_width))
                .mx_auto()
                .child(
                    TextView::new(&doc)
                        .size_full()
                        .scrollable(true)
                        .px(px(layout.pad_x))
                        .py(px(layout.pad_y))
                        .style(text_style)
                        .code_block_highlighter(move |block| {
                            // 代码块可能没有标注语言
                            let lang = block.lang();
                            let lang = lang.as_ref().map(|s| s.as_ref()).unwrap_or("");
                            // 命中缓存时拿到的是共享的 `Arc<Vec<Span>>`：
                            // 遍历借用即可，不必把整份片段列表克隆出来
                            highlight(lang, block.code().as_ref(), &highlight_theme, dark)
                                .iter()
                                .map(|(range, style)| (range.clone(), to_highlight_style(*style)))
                                .collect()
                        })
                        .on_link_click(|url, _event, window, cx| {
                            handle_link_click(url, window, cx);
                        }),
                ),
        );

        crate::ui::log_slow("PreviewPanel::render", started);
        view
    }
}

/// 领域层的片段样式 → gpui 的 `HighlightStyle`
///
/// 以前只带颜色，粗体 / 斜体 / 下划线被丢掉了；现在统一主题里写了什么，
/// 预览区就呈现什么（与编辑区同一份来源）。
fn to_highlight_style(style: SpanStyle) -> HighlightStyle {
    HighlightStyle {
        color: style.color,
        font_weight: if style.bold {
            Some(gpui_kit::FontWeight::BOLD)
        } else {
            None
        },
        font_style: if style.italic {
            Some(FontStyle::Italic)
        } else {
            None
        },
        underline: if style.underline {
            Some(UnderlineStyle {
                thickness: px(1.),
                color: None,
                wavy: false,
            })
        } else {
            None
        },
        ..Default::default()
    }
}

/// 版面密度菜单（紧凑 / 适中 / 宽松）。
///
/// 挂进面板标题，与"预览"同一行：不额外占一行，且和标题水平对齐。
fn density_menu(density: PreviewDensity) -> impl IntoElement {
    div()
        // 独占点击：避免点菜单时顺带触发标签选中
        .occlude()
        .flex_none()
        .child(
            DropdownButton::new("preview-density")
                .button(
                    Button::new("preview-density-trigger")
                        .small()
                        .ghost()
                        .label(density.label()),
                )
                .dropdown_menu(|menu, _window, _cx| {
                    let mut menu = menu;
                    for option in [
                        PreviewDensity::Compact,
                        PreviewDensity::Normal,
                        PreviewDensity::Relaxed,
                    ] {
                        menu = menu.item(PopupMenuItem::new(option.label()).on_click(
                            move |_, _, cx| {
                                // 走统一入口：改内存 → 落盘 → 刷新窗口，与设置页一致
                                crate::app::update_config(cx, |cfg| {
                                    cfg.preview.density = option;
                                });
                            },
                        ));
                    }
                    menu
                }),
        )
}

/// 点击预览区链接 / 图片的行为
///
/// - 本地图片：预处理时我们把图片包成了指向自身的 `file://` 链接，因此点击图片
///   会走到这里（框架没有图片点击回调，只有链接回调），此时打开图片预览窗口；
/// - 其余链接按 `preview.link_open` 配置处理：应用内打开时尝试作为笔记打开，
///   打不开（非 Markdown / 文件不存在）则退回系统浏览器。
fn handle_link_click(url: &SharedString, window: &mut Window, cx: &mut App) {
    if let Some(path) = path_from_file_url(url) {
        if is_image_path(&path) && path.is_file() {
            // 在预览面板里显示这张图（不再新开窗口）
            crate::app::show_preview_image(path, cx);
            return;
        }
        if crate::app::config(cx).preview.link_open == LinkOpen::InApp
            && crate::ui::tabs::open_note(path, window, cx)
        {
            return;
        }
    }
    cx.open_url(url);
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/preview.rs"]
mod tests;
