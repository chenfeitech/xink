//! 中间编辑面板：Editor 编辑 + 防抖自动保存 + 原子写盘

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::base::TextViewState;
use gpui_kit::base::dock::TabGroup;
use gpui_kit::base::input::{Diagnostic, Position};
use gpui_kit::component::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::input::{
    Editor, EditorState, InputEvent, TabSize, TextDecoration, TextDecorationCollection,
};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, Size};
use gpui_kit::*;

use crate::actions::{
    FormatBold, FormatInlineCode, FormatItalic, FormatLink, FormatStrikethrough, Save, ToggleAiChat,
};
use crate::app::{ActiveDocChanged, AppState, EditorScrolled, PreviewScrolled};
use crate::core::config::{DataFileConfig, IndentConfig, ScrollSyncMode};
// 与 gpui 的 `Action` trait 同名，取别名避免"看起来是类型、其实是 trait"
use crate::core::ai::prompt::Action as AiAction;
use crate::core::datafile::{DataKind, Parsed, ValidationError, data_kind};
use crate::core::markdown::{
    ImageOptions, SpanKind, decoration_spans, resolve_image_urls, rewrite_wikilinks,
    sanitize_for_preview,
};
use crate::core::media::editor_language;
use crate::core::note::is_markdown;
use crate::core::sync::{AnchorTable, GestureGuard, ScrollAnchor, Side, SyncGuard};
use crate::core::{Note, tab_key};
use crate::ui::{AiChat, AiChatEvent};
use std::sync::Arc;

/// 行首标记（`#`、列表符号）的淡化程度，语义等同 CSS `opacity`
const MARKER_FADE_OUT: f32 = 0.45;

pub struct EditorPanel {
    focus_handle: FocusHandle,
    note: Option<Note>,
    editor: Entity<EditorState>,
    /// 已下发给编辑控件的缩进配置，用于检测配置变更并增量应用
    applied_indent: IndentConfig,
    /// 已下发的行号开关，用于检测配置变更并增量应用
    applied_line_numbers: bool,
    /// 上次生成预览源文所用的图片选项，用于检测配置变更并重算预览
    applied_image_opts: Option<ImageOptions>,
    /// 上次生成预览源文所用的数据文件配置，用于检测配置变更并重算预览
    applied_data_cfg: DataFileConfig,
    /// 当前数据文件（json/yaml/toml）的校验结果；Markdown 笔记恒为 None
    validation: Option<ValidationError>,
    /// 本篇笔记自己的预览文档。切换标签时整体替换引用，已解析过的不再重解析。
    doc: Option<Entity<TextViewState>>,
    /// 防抖保存任务。每次编辑都用新任务覆盖，旧任务被 drop 即取消。
    save_task: Option<Task<()>>,
    /// 编辑器下方的 AI 对话条：按需创建一次，关闭只是隐藏（保留历史）
    ai_chat: Option<Entity<AiChat>>,
    ai_open: bool,
    status: SharedString,
    /// 滚动同步方向锁：谁先滚谁在一段时间内独占推动同步，对端回显一律吸收
    /// （取代旧的"单值 EPS 比较"，后者既会 ping-pong 也会吃掉用户滚动）
    guard: SyncGuard,
    /// 刚从预览收到、还没精调到位的锚点。
    ///
    /// 软换行下"第 N 行在哪个像素位置"框架不直接给，只能先按行高粗调、
    /// 再等布局出来用 `range_to_bounds` 精调，因此需要 1~2 帧收敛。
    pending_anchor: Option<ScrollAnchor>,
    /// 精调尝试次数，超过上限就放弃（避免布局迟迟不来时卡在循环里）
    pending_tries: u8,
    /// 上一帧读到的顶部源码行：下一帧以它为起点做局部探测，避免全表扫描
    last_top_line: f32,
    /// 用户手势静默窗口：窗口内**绝不**向编辑区写滚动位置。
    ///
    /// 滚轮 / 触控板惯性期间，任何外部写入都会表现为"滚不动 / 被拉回"。
    /// 框架的 `set_scroll_offset` 只是种 `deferred_scroll_offset`，绘制时它会
    /// 压过同一帧里用户滚轮的结果（`input/base/element.rs:671-679`），
    /// 所以必须在应用层让位。
    gesture: GestureGuard,
    /// 上一帧的滚动位置，用来识别"位置变了但不是我们写的" = 用户在滚
    last_scroll_y: Pixels,
    /// 帧计数：实时模式下推送降频用
    sync_tick: u8,
    /// 实时模式下最近一次推送出去的锚点（死区判定）
    last_pushed: Option<ScrollAnchor>,
    /// 有滚动发生过、但还没同步出去（"停止后对齐"模式下用）
    ///
    /// 抖动根治策略：滚动过程中**绝不写对端**，停下（静默窗口结束）后对齐一次。
    /// 滚动中写对端必然产生跳变——换算再准也躲不掉（块内渲染非线性、
    /// 虚拟列表高度在变），取舍下来"停止后对齐"是唯一能彻底不抖的方案。
    pending_push: bool,
    /// 上一次"每源行多少像素"的采样点（顶部行号，当时的滚动位置）
    ///
    /// 只用于比例模式下的内容高度学习，不参与同步判定。
    last_probe: Option<(f32, Pixels)>,
    /// 用户最后一次在这篇笔记里输入的时刻
    ///
    /// 编辑期间的编辑区是"权威端"：预览此刻的位置并不可信（重解析会把它
    /// 打回顶部），一旦听它的就会被拽到文章开头。
    last_edit_at: Option<Instant>,
    /// 最近一次**由同步写入**滚动位置的时刻
    ///
    /// 刚被对端写过的一端，短时间内不再反向推（[`Self::PUSH_SUPPRESS`]）。
    /// 写入是 deferred 的、落点还会被精调修正，误差超过回显阈值时就会
    /// 触发一次反推 → 对端再写回来 = 两端互推（持续抖动的真正来源）。
    last_write_at: Option<Instant>,
    /// 一个源码行大约占多少像素（软换行下大于行高）
    ///
    /// 粗调目标行位置时要用到：软换行下按 `行号 × 行高` 估会系统性偏上，
    /// 靠实测值能让粗调一步到位，减少精调帧数与可见跳动。
    px_per_source_line: Pixels,
    /// 已下发的软换行开关，用于检测配置变更并增量应用
    applied_soft_wrap: bool,
    /// 上次推送给预览的内容标识（路径 + 版本），用于跳过无变化的重算
    last_published_path: Option<PathBuf>,
    last_published_version: Option<u64>,
    /// 自己的弱引用，供关闭按钮精确移除本面板
    self_entity: WeakEntity<EditorPanel>,
    /// 双链补全浮层是否打开：避免每敲一个字符都重复弹一次
    completion_open: bool,
    /// 已排了一次"稍后回填外部改动"的任务
    ///
    /// 打字期间（`EDIT_PROTECT`）收到外部改动只登记一次延后任务，
    /// 否则每个监听事件都叠一个定时器，回填会被无限期推迟。
    pending_external: bool,
    /// 最近一次**重算预览源文**的时刻（[`Self::refresh_doc`] 真重算时）
    ///
    /// 重算必然让预览 `set_text` 重解析，而重解析会把预览打回顶部。
    /// 那一瞬间预览的位置是假信号，跟"用户滚到了顶部"长得一模一样，
    /// 推回编辑区就是"写完字被拽回文章开头"。这段时间内顶部锚点不采纳。
    content_changed_at: Option<Instant>,
    /// Markdown 装饰集合。范围随编辑自动平移，但内容变了要由我们重设。
    decorations: Option<TextDecorationCollection>,
}

impl EditorPanel {
    /// 双链补全浮层是否打开（供 `ui::completion` 回写状态）
    pub fn set_completion_open(&mut self, open: bool) {
        self.completion_open = open;
    }

    /// 把光标前的 `[[…` 替换成 `[[目标]]`，并把焦点收回到编辑区。
    ///
    /// 由 `ui::completion` 在用户选定候选后调用。
    pub fn apply_wikilink(
        &mut self,
        start: usize,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = format!("[[{target}]]");
        self.editor.update(cx, |state, cx| {
            let end = state.cursor();
            // 起点可能落在文本之外（例如期间删了字），夹一下避免 panic。
            // 用 `text().len()` 而不是 `value().len()`：后者会物化整篇字符串
            let start = start.min(end).min(state.text().len());
            state.set_selected_range(start..end, cx);
            state.replace(&text, window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    /// 光标前刚输入了 `[[`（未闭合、未换行）时弹出补全浮层。
    fn maybe_complete_wikilink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.completion_open || self.note.is_none() {
            return;
        }
        // 判定只需要"光标前紧贴 `[[`"，因此只读光标前的两个字符。
        // 旧做法 `value().to_string()` 会物化整篇文本——每次按键多付一次全文拷贝。
        let (cursor, opens_here) = {
            let state = self.editor.read(cx);
            let text = state.text();
            let at = state.cursor().min(text.len());
            let opens = text.is_char_boundary(at) && at >= 2 && {
                let mut before = text.chars_at(at).reversed();
                matches!((before.next(), before.next()), (Some('['), Some('[')))
            };
            (at, opens)
        };
        // 只认"光标紧贴 `[[`"：中间已有其它字符说明用户在自己写，不打扰
        if !opens_here {
            return;
        }
        let start = cursor - 2;
        let Some(panel) = self.self_entity.upgrade() else {
            return;
        };
        self.completion_open = true;
        crate::ui::completion::open_wikilink_completion(panel, start, window, cx);
    }

    /// 空面板，供布局恢复的重建工厂使用。
    ///
    /// 想打开笔记请用 `crate::ui::tabs::open_note`——它是唯一入口，
    /// 负责去重、认领空白页、登记与落盘。本构造函数只造一个空白面板。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::create(window, cx)
    }

    fn create(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = crate::app::config(cx);
        let show_line_numbers = cfg.editor.show_line_numbers;
        let indent = cfg.editor.indent;

        // 编辑内核为 `EditorState`（代码编辑模式）：行号、诊断等能力只有它提供。
        // Markdown 写作不需要括号/引号自动补全与回车自动缩进，这里显式关闭；
        // 折叠与缩进参考线的交互尚未验证，一并关闭，避免未预期的行为。
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(show_line_numbers)
                .auto_close(false)
                .smart_indent(false)
                .folding(false)
                .indent_guides(false)
        });
        // 缩进配置接线：`set_tab_size` 定义在 `impl<M: MultiLineMode>`
        // （gpui-base input/editor/indent.rs:492），两种内核都可用。
        editor.update(cx, |state, cx| {
            state.set_tab_size(
                TabSize {
                    tab_size: indent.tab_size as usize,
                    hard_tabs: indent.use_tabs,
                },
                cx,
            );
        });
        let self_entity = cx.weak_entity();

        cx.subscribe_in(&editor, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::Change) {
                this.on_change(window, cx);
            }
        })
        .detach();

        let mut this = Self {
            focus_handle: cx.focus_handle(),
            note: None,
            editor,
            applied_indent: indent,
            applied_line_numbers: show_line_numbers,
            applied_image_opts: None,
            applied_data_cfg: crate::app::config(cx).editor.data_file.clone(),
            validation: None,
            doc: None,
            save_task: None,
            ai_chat: None,
            ai_open: false,
            status: SharedString::default(),
            guard: SyncGuard::default(),
            pending_anchor: None,
            pending_tries: 0,
            last_top_line: 0.0,
            gesture: GestureGuard::default(),
            sync_tick: 0,
            last_pushed: None,
            last_probe: None,
            last_edit_at: None,
            last_write_at: None,
            pending_push: true,
            last_scroll_y: px(0.),
            px_per_source_line: px(0.),
            applied_soft_wrap: crate::app::config(cx).editor.soft_wrap,
            last_published_path: None,
            last_published_version: None,
            self_entity: self_entity.clone(),
            completion_open: false,
            pending_external: false,
            content_changed_at: None,
            decorations: None,
        };

        // 空白页：登记到集中列表，等待被笔记认领或统一清理。
        // 一旦被认领，`tabs::open_note` 会把它从该列表中摘除。
        cx.update_global::<AppState, _>(|state, _| {
            state.empty_panels.push(self_entity);
        });

        // 预览滚动 → 同步本编辑器（仅活动编辑器跟随，避免多标签互相干扰）
        if let Some(preview) = cx.try_global::<AppState>().map(|s| s.preview.clone()) {
            cx.subscribe_in(
                &preview,
                window,
                move |this, _, ev: &PreviewScrolled, window, cx| {
                    let PreviewScrolled(anchor) = ev;
                    let is_active = cx.try_global::<AppState>().is_some_and(|s| {
                        s.preview.read(cx).active_doc.as_ref() == this.doc.as_ref()
                    });
                    if is_active {
                        this.apply_anchor(*anchor, window, cx);
                    }
                },
            )
            .detach();
        }

        // 启动编辑区滚动同步轮询（框架在实体移除后不再回调，循环自动停止）
        this.schedule_scroll_tick(window, cx);
        this
    }

    /// 把自己的内容更新进**自己的**预览文档。
    ///
    /// 每篇笔记持有独立的 TextViewState，因此这里是增量更新（只重算变动的块），
    /// 而不是把整篇重新塞进同一个共享文档。
    fn refresh_doc(&mut self, cx: &mut Context<Self>) {
        let started = Instant::now();
        let data_cfg = crate::app::config(cx).editor.data_file.clone();
        // 先用「路径 + 版本 + 图片选项」判断是否值得重算。
        // note.text() 会把整个 Rope 转成 String，再加上图片路径正则重扫，
        // 若在 render 里每帧执行，切换标签时会明显卡顿。
        //
        // 图片选项必须参与判断：只改图片配置时路径与版本都不变，
        // 若不看它，改了图片宽度也永远不会重算源文（配置表现为"不生效"）。
        let opts = image_options(cx);
        let (path, version) = match &self.note {
            Some(note) => (note.path().to_path_buf(), note.version()),
            None => {
                // 没有笔记时无需重算，但要记下当前选项，避免每帧重复进入
                self.applied_image_opts = Some(opts);
                return;
            }
        };
        if self.last_published_path.as_ref() == Some(&path)
            && self.last_published_version == Some(version)
            && self.applied_image_opts == Some(opts)
            && self.applied_data_cfg == data_cfg
        {
            return;
        }
        self.last_published_path = Some(path);
        self.last_published_version = Some(version);
        self.applied_image_opts = Some(opts);
        self.applied_data_cfg = data_cfg.clone();

        let Some(note) = &self.note else {
            return;
        };
        // 锚点表要用行数与"是否 Markdown"，先取出来，
        // 后面 `sync_diagnostics` 会拿走 `self` 的可变借用
        let line_count = note.line_count();
        let is_md = is_markdown(note.path());
        let (source, validation) = Self::preview_source(note, opts, &data_cfg, cx);
        self.validation = validation;
        self.sync_diagnostics(cx);
        if let Some(doc) = &self.doc {
            doc.update(cx, |doc, cx| doc.set_text(source.as_str(), cx));
        }
        // 预览刚被重解析：它此刻的滚动位置不可信（会被打回顶部）。
        // 记下时刻，供 [`Self::is_fake_top`] 过滤随后推来的"顶部"锚点。
        self.content_changed_at = Some(Instant::now());

        // 滚动同步的锚点表：预览面板拿不到源文本，只能由编辑区把"块 ↔ 源码行"
        // 的映射放进全局。数据文件预览是格式化后的文本，行号对不上 → 不发布，
        // 两端自动退回比例同步。
        let anchors =
            is_md.then(|| Arc::new(AnchorTable::from_source(source.as_str(), line_count)));
        cx.update_global::<AppState, _>(|state, cx| {
            state.preview.update(cx, |preview, cx| {
                preview.anchors = anchors.clone();
                cx.notify();
            });
        });

        // 大纲跟手：标题一改就重新解析（纯内存操作，成本可忽略）。
        // 不走索引——索引是"保存之后"的语义，等它更新大纲会慢半拍。
        if let Some(note) = &self.note {
            crate::ui::outline::update_outline(note.path(), note.text().as_str(), cx);
        }

        crate::ui::log_slow("refresh_doc", started);
    }

    /// 把校验结果下发成编辑区诊断（波浪线）。
    ///
    /// 只有数据文件会走到这里（Markdown 的 `validation` 恒为 None，效果是清空）。
    ///
    /// 关键是**先 `reset` 再 `push`**：`DiagnosticSet` 内部缓存了一份 Rope 用来
    /// 把行列换算成字节区间，而我们是在 `set_value` / 编辑之后才下发诊断的，
    /// 缓存可能已经过期。不 reset 的话换算会对着旧文本做，标记位置错乱。
    fn sync_diagnostics(&mut self, cx: &mut Context<Self>) {
        let error = self.validation.clone();
        self.editor.update(cx, |state, cx| {
            let text = state.text().clone();
            let Some(set) = state.diagnostics_mut() else {
                return;
            };
            set.reset(&text);
            if let Some(err) = &error {
                let line = err.line as u32;
                let column = err.column as u32;
                set.push(Diagnostic::new(
                    Position::new(line, column)..Position::new(line, column + 1),
                    err.message.clone(),
                ));
            }
            cx.notify();
        });
    }

    /// 跳到指定行（0-based）：光标落行首，并把该行滚进视口。
    ///
    /// 大纲点击、搜索结果跳转都走这里。滚动偏移要夹到
    /// `[-(内容高 - 视口高), 0]`：越界会被框架回弹，表现是"跳过去又弹回来"。
    pub fn goto_line(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = &self.note else {
            return;
        };
        let total = note.line_count();
        let (line_h, viewport) = {
            let state = self.editor.read(cx);
            (state.line_height(), state.input_bounds().size.height)
        };

        // 光标位置不依赖布局，先落下去。
        //
        // 以前这行排在行高检查之后，而 `line_height()` 是
        // `last_layout.map(...)`——首次布局前恒为 None，于是函数直接 return，
        // 光标和滚动一个都没动，表现就是"点了大纲没反应"。
        self.editor.update(cx, |state, cx| {
            state.set_cursor_position(Position::new(line as u32, 0), window, cx);
        });

        // 滚动量要按行高算，布局没就绪就算不出来。调用方
        // （`tabs::goto_line_of`）会等本帧绘制完成后再补一次。
        let Some(line_h) = line_h else {
            cx.notify();
            return;
        };
        if viewport <= px(0.) {
            cx.notify();
            return;
        }

        let target_y = scroll_offset_for_line(line, total, line_h, viewport);
        self.trace_write(cx, "goto_line", point(px(0.), target_y));
        self.editor.update(cx, |state, cx| {
            state.set_scroll_offset(point(px(0.), target_y), cx);
        });
        cx.notify();
    }

    /// 让预览面板显示自己的文档。
    /// 切换标签时只替换引用，已解析过的笔记不会重新解析。
    fn activate_preview(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.clone() else {
            return;
        };
        cx.update_global::<AppState, _>(|state, cx| {
            state.preview.update(cx, |preview, cx| {
                if preview.active_doc.as_ref() != Some(&doc) {
                    preview.active_doc = Some(doc.clone());
                    cx.emit(ActiveDocChanged(doc));
                }
            });
        });
    }

    /// 生成预览源文，并返回数据文件的校验结果（Markdown 时为 None）。
    ///
    /// Markdown：本地图片转 data URL，并按配置决定显示方式与宽度；
    /// 数据文件（json/yaml/toml）：拼成围栏代码块展示格式化结果。
    fn preview_source(
        note: &Note,
        opts: ImageOptions,
        data_cfg: &DataFileConfig,
        cx: &App,
    ) -> (String, Option<ValidationError>) {
        let text = note.text();
        if let Some(kind) = data_kind(note.path()) {
            return data_preview_source(&text, kind, data_cfg);
        }
        let dir = note.path().parent().unwrap_or(Path::new(""));
        // 先做防御性预处理，避免畸形 Markdown 让预览解析器 panic 导致应用退出
        let safe = sanitize_for_preview(&text);
        // `[[双链]]` 在这里变成指向目标笔记的链接：改的只是预览源文副本，
        // 磁盘上的原文一个字节都不动。点击侧复用既有的 handle_link_click
        // （库内 .md → open_note），因此仍然只有一个"打开笔记"的出口。
        let linked = rewrite_wikilinks(&safe, |target| resolve_wikilink(cx, target));
        (resolve_image_urls(&linked, dir, opts), None)
    }

    /// 当前打开的是数据文件时返回其种类
    fn data_kind(&self) -> Option<DataKind> {
        self.note.as_ref().and_then(|note| data_kind(note.path()))
    }

    /// 载入一篇笔记。也用于把空白占位页"填"成真正的笔记。
    ///
    /// 返回是否成功——调用方（[`crate::ui::tabs::open_note`]）据此决定
    /// 是否把它登记为"已打开"：失败时不能登记，否则空白页永不出现，
    /// 下次启动还会尝试恢复一个打不开的路径。
    pub fn load_note(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        path: PathBuf,
    ) -> bool {
        let data_cfg = crate::app::config(cx).editor.data_file.clone();
        let loaded = match Note::load(path) {
            Ok(note) => {
                let text = note.text();
                // set_value 不触发 Change 事件，因此加载不会被误判为用户编辑
                self.editor.update(cx, |state, cx| {
                    state.set_value(text, window, cx);
                });
                // 数据文件按各自语言高亮，Markdown 走 markdown 语法，其余纯文本。
                // 代码块内的 ```json / ```yaml 由 tree-sitter injection 着色，
                // 前提是那门语言自己也在上面的 features 里启用。
                //
                // 每次载入都显式设置：面板会被复用（空白页认领机制），
                // 不重置的话"关掉 json 再打开 md"会残留 json 的高亮配色。
                let lang = editor_language(note.path());
                self.editor.update(cx, |state, cx| {
                    state.set_highlighter(lang, cx);
                });
                // 软换行：只在需要"关闭"时下发。框架默认开启，再下发一次 true
                // 是多余的一次重排（display_map 会重算换行），打开笔记时没必要
                let soft_wrap = crate::app::config(cx).editor.soft_wrap;
                self.applied_soft_wrap = soft_wrap;
                if !soft_wrap {
                    self.editor.update(cx, |state, cx| {
                        state.set_soft_wrap(false, window, cx);
                    });
                }

                // 关掉"最后一行之后的空白"：`EditorMode::CODE_EDITOR = true`
                // （gpui-base `input/editor/mod.rs:15`），框架会按代码编辑器的习惯在
                // 滚动区尾部加**半屏空白**（`empty_bottom_height`，
                // `input/base/element.rs:377-390`）。写 Markdown 时它的表现是：
                // 内容明明已经到底了还能继续滚半屏（内容不动）→ "滚不动 / 卡住"。
                // 同时这段空白会让编辑区与预览区的可滚动范围对不上，破坏同步。
                self.editor.update(cx, |state, cx| {
                    state.set_scroll_beyond_last_line(Some(0), window, cx);
                });
                // 载入即上一次装饰，不等防抖，否则新标签会先闪一下无装饰的样子
                self.refresh_decorations(cx);
                self.status = format!("已打开 {}", note.path().display()).into();
                // 每篇笔记一个预览文档，首次解析后切回来就无需再解析
                let (source, validation) =
                    Self::preview_source(&note, image_options(cx), &data_cfg, cx);
                self.applied_data_cfg = data_cfg;
                self.validation = validation;
                self.doc = Some(cx.new(|cx| TextViewState::markdown(source.as_str(), cx)));
                self.note = Some(note);
                // 新笔记：滚动同步状态必须归零，否则会带着上一篇的位置跳一下（R6）
                self.reset_scroll_sync();
                true
            }
            Err(err) => {
                // 载入失败不能留下上一篇的校验结果
                self.validation = None;
                self.status = format!("打开失败: {err}").into();
                false
            }
        };
        self.sync_diagnostics(cx);
        // 打开即出预览，不必等到第一次编辑
        self.activate_preview(cx);
        cx.notify();
        loaded
    }

    /// 清空为空白占位页。
    ///
    /// 关闭最后一个标签时调用：不移除面板，而是把它清空保留下来。
    /// 否则编辑区会整体消失，下次打开笔记也失去落点。
    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 先记住路径：清空后笔记已不在，但要从"已打开"列表里摘掉，
        // 否则重启时会把它错误地恢复出来。
        let path = self.note.as_ref().map(|n| n.path().to_path_buf());
        self.note = None;
        self.doc = None;
        self.validation = None;
        self.last_published_path = None;
        self.last_published_version = None;
        self.status = SharedString::default();
        self.reset_scroll_sync();

        self.editor
            .update(cx, |state, cx| state.set_value("", window, cx));
        // 清空后不留上一篇的波浪线与装饰
        self.sync_diagnostics(cx);
        self.refresh_decorations(cx);

        let self_entity = self.self_entity.clone();
        cx.update_global::<AppState, _>(|state, cx| {
            if let Some(path) = &path {
                state.tabs.remove(&tab_key(path));
            }
            // 大纲与预览同源：笔记没了，两个面板都要回到空态
            state.outline.update(cx, |outline, cx| {
                outline.clear();
                cx.notify();
            });
            // 预览不再有内容
            state.preview.update(cx, |preview, cx| {
                preview.active_doc = None;
                cx.notify();
            });
            // 变回空白页，重新登记供下次复用
            state.empty_panels.push(self_entity);
        });

        // 落盘，避免下次启动恢复出已关闭的笔记
        crate::ui::tabs::persist_open_notes(cx);
        cx.notify();
    }

    /// 滚动同步状态归零（切换笔记 / 清空面板时）
    fn reset_scroll_sync(&mut self) {
        self.guard.reset();
        self.pending_anchor = None;
        self.pending_tries = 0;
        self.last_top_line = 0.0;
        self.last_scroll_y = px(0.);
        // 换行宽度与文档都换了，上一份"每行多少像素"的实测值不再适用
        self.px_per_source_line = px(0.);
        self.gesture.reset();
        self.last_write_at = None;
        self.last_pushed = None;
        self.last_probe = None;
        // 换文档后要对齐一次（否则预览停在上一篇的位置）
        self.pending_push = true;
    }

    /// 实时模式的推送降频倍率（每 N 帧推一次）
    const SYNC_EVERY_N_FRAMES: u8 = 2;
    /// 实时模式的死区：锚点变化小于这个值不推送（单位：源码行）
    const DEAD_ZONE: f32 = 0.2;
    /// 每几帧采样一次"每源行多少像素"（软换行下内容高度估算用）
    const LEARN_EVERY_N_FRAMES: u8 = 8;
    /// 用户最后一次输入后，这段时间内编辑区不接受对端的滚动指令
    ///
    /// 要覆盖"输入防抖（默认 300ms）+ 预览重解析 + 重新测量"的完整往返。
    const EDIT_PROTECT: Duration = Duration::from_millis(800);
    /// 刚被对端写过之后，这段时间内不反向推（斩断互推）
    const PUSH_SUPPRESS: Duration = Duration::from_millis(400);
    /// 内容重算后，这段时间内"来自预览的顶部锚点"视为假信号
    ///
    /// 时间线：最后一次输入 → 防抖 300ms → 预览 `set_text` 重解析（被
    /// 打回顶部）→ 预览侧静默 800ms（`PreviewPanel::CONTENT_SETTLE`）
    /// → 才把位置推过来。也就是说假信号要到 **1.1 秒之后**才到，
    /// 光靠 800ms 的 `EDIT_PROTECT` 拦不住——这正是"写完一会儿被
    /// 拽回文章开头"的成因。这里按整条往返给窗口。
    const TOP_GUARD: Duration = Duration::from_millis(2000);

    /// 是否处于"用户正在滚编辑区"的静默窗口内
    fn is_user_scrolling(&self, now: Instant) -> bool {
        self.gesture.is_quiet(now)
    }

    /// 本面板是否是当前活动笔记的编辑区
    ///
    /// 后台标签的编辑器也在跑帧轮询，若不判断就会把预览带跑（设计文档 §2 R5）。
    fn is_active(&self, cx: &App) -> bool {
        let Some(doc) = self.doc.as_ref() else {
            return false;
        };
        cx.try_global::<AppState>()
            .is_some_and(|state| state.preview.read(cx).active_doc.as_ref() == Some(doc))
    }

    /// 每帧轮询编辑区滚动位置，换算成**源码行锚点**推给预览。
    ///
    /// 与旧实现的两点区别（详见 `docs/preview/scroll-sync-design.md`）：
    /// 1. 推的是"视口顶部在第几源码行"而不是滚动条百分比——两端内容结构不同，
    ///    百分比必然错位（R1）。
    /// 2. 防循环用 [`SyncGuard`] 方向锁，不再拿固定阈值比较单值（R4）。
    fn tick_scroll_sync(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // 编辑器侧也判断开关：关闭后不再向预览推送。
        // 配置在每次触发时读取，因此改设置后下一次滚动即生效（L2）。
        if !crate::app::config(cx).preview.sync_scroll {
            return;
        }
        if !self.is_active(cx) {
            return;
        }

        let now = Instant::now();
        // 识别用户手势：滚动位置变了，而我们这一帧并没有写入 → 只能是用户滚的
        // （滚轮、触控板惯性、拖滚动条、光标移动引起的自动滚动）。
        // 识别到就开一个静默窗口，期间同步不再写编辑区（详见字段注释）。
        let off_now = self.editor.read(cx).scroll_offset().y;
        if self.pending_anchor.is_none()
            && (off_now - self.last_scroll_y).abs() > px(0.5)
            && !self.is_own_write(cx)
        {
            self.gesture.note_user_movement(now);
        }
        let prev_scroll_y = self.last_scroll_y;
        let prev_top_line = self.last_top_line;
        self.last_scroll_y = off_now;

        // 探针（仅调试开关下）：位置突然跑到顶部，而我们这一帧既没写过、
        // 也不像用户滚的——"被拽回文章开头"不管最后查出是谁干的，
        // 都会在这里留下一条带时间关系的现场。
        if crate::app::config(cx).preview.debug_scroll_sync
            && off_now.abs() <= px(1.)
            && prev_scroll_y < px(-50.)
            && !self.is_own_write(cx)
            && !self.is_user_scrolling(now)
        {
            eprintln!(
                "[xink][jump] 编辑区被带到顶部：上一帧 {prev_scroll_y:?} → 现在 {off_now:?}；\
                 距上次写入 {:?}，距上次输入 {:?}，距上次内容重算 {:?}",
                self.last_write_at.map(|at| now.duration_since(at)),
                self.last_edit_at.map(|at| now.duration_since(at)),
                self.content_changed_at.map(|at| now.duration_since(at)),
            );
        }

        let mode = crate::app::config(cx).preview.sync_mode;
        let scrolling = self.is_user_scrolling(now);

        // 用户滚动中一律让位：预览推过来的精调也别写了（会与手势抢）
        if scrolling && self.pending_anchor.is_some() {
            self.pending_anchor = None;
            self.pending_tries = 0;
        }

        match mode {
            ScrollSyncMode::OnStop => {
                // 滚动中：不读（省掉每帧的布局探测）、不写对端，
                // 只记"待对齐"，等停下后补一次 —— 绝对不抖的方案
                if scrolling {
                    self.pending_push = true;
                    return;
                }
                if self.pending_anchor.is_some() {
                    self.refine_pending(cx);
                    return;
                }
                if !self.pending_push {
                    return;
                }
                self.pending_push = false;
            }
            // 比例 / 块锚点：实时跟随（隔帧推送，观感仍然跟手）
            ScrollSyncMode::Ratio | ScrollSyncMode::Anchor => {
                if self.pending_anchor.is_some() {
                    self.refine_pending(cx);
                    return;
                }
                // 软换行下"每源行多少像素"决定内容高度估算准不准，
                // 比例方案也依赖它（估低了就是"不同步"）。不必每帧探，
                // 隔几帧学一次即可。
                if self.applied_soft_wrap
                    && self.sync_tick.is_multiple_of(Self::LEARN_EVERY_N_FRAMES)
                {
                    self.learn_px_per_line(cx);
                }
                // 降频：滚动时每帧都让预览重排不划算（虚拟列表 + 代码块高亮），
                // 隔帧推送在观感上仍然跟手
                self.sync_tick = self.sync_tick.wrapping_add(1);
                if !self.sync_tick.is_multiple_of(Self::SYNC_EVERY_N_FRAMES) {
                    return;
                }
            }
        }

        let Some(anchor) = self.current_anchor(cx) else {
            return;
        };

        // 软换行下顺便学"一个源码行约多少像素"，供粗调使用
        if let (ScrollAnchor::Line(line), true) = (anchor, self.applied_soft_wrap) {
            let d_line = (line - prev_top_line).abs();
            let d_off = (off_now - prev_scroll_y).abs();
            if d_line >= 1.0 && d_off > px(0.) {
                let sampled = d_off / d_line;
                let current = if self.px_per_source_line > px(0.) {
                    self.px_per_source_line
                } else {
                    sampled
                };
                // 指数平滑：单次异常值不至于把估算带偏
                self.px_per_source_line = current * 0.8 + sampled * 0.2;
            }
        }

        // 实时模式下的死区：亚像素级变化不推。同步不需要像素级精度，
        // 反倒是这些微小抖动会让两端来回微调，看起来就是"抖"。
        //
        // 注意单位：行锚点用"行"（0.2 行），比例锚点是 0~1，
        // 同一个 0.2 在这里等于 20% 的滚动范围——会把同步几乎完全掐死。
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

        // 回显：我刚按对端的目标滚过 → 吸收，不再反向推。
        // 阈值按锚点种类给：源码行用"行"为单位——探测会把锚点量化到整行边界，
        // 加上块内插值误差，1.5 行内都认为是同一次同步的回显；比例沿用 0.002。
        let tol = match anchor {
            ScrollAnchor::Line(_) => 1.5,
            ScrollAnchor::Ratio(_) => 0.002,
        };
        if self.guard.is_echo(anchor, tol) {
            return;
        }
        // 刚被对端写过：这一帧的变化很可能是对端写入的余波，别反推回去。
        // 用户在滚时不拦（`scrolling` 为真说明确实是自己的手势）。
        if !scrolling
            && let Some(at) = self.last_write_at
            && now.duration_since(at) < Self::PUSH_SUPPRESS
        {
            return;
        }
        // 对端正持有方向锁 → 这次变化是对端同步的余波
        if !self.guard.claim(Side::Editor, Instant::now()) {
            return;
        }
        if let ScrollAnchor::Line(line) = anchor {
            self.last_top_line = line;
        }

        let debug = crate::app::config(cx).preview.debug_scroll_sync;
        cx.update_global::<AppState, _>(|state, cx| {
            state.preview.update(cx, |preview, cx| {
                if debug {
                    let mut pair = preview.debug_anchors.unwrap_or((0.0, 0.0));
                    pair.0 = anchor.value();
                    preview.debug_anchors = Some(pair);
                }
                cx.emit(EditorScrolled(anchor));
            });
        });
    }

    /// 编辑区内容高度的估算值
    ///
    /// 软换行下"源码行数 × 行高"会系统性低估（一个源码行可能占几个显示行），
    /// 低估的直接后果是比例换算失真 → 看着就是"不同步"。这里改用滚动中实测的
    /// `px_per_source_line`（见 `tick_scroll_sync` 的学习逻辑）来估，
    /// 没有实测值时退回行高。
    ///
    /// **读和写必须用同一个函数**：比例方案的稳定性来自两端各自对称，
    /// 只要读写同式，回显就一定落在容差内，不会互推。
    fn content_height(&self, cx: &App) -> Option<Pixels> {
        let st = self.editor.read(cx);
        let line_h = st.line_height()?;
        let viewport_h = st.input_bounds().size.height;
        let total = self.note.as_ref()?.line_count() as f32;
        let px_per_line = if self.px_per_source_line > px(0.) {
            self.px_per_source_line.max(line_h)
        } else {
            line_h
        };
        // 至少比视口高一像素，避免短文档下分母为 0 导致比例恒为 0
        Some((px_per_line * total).max(viewport_h + px(1.)))
    }

    /// 可滚动范围（内容高度 − 视口高度）
    ///
    /// 视口还没布局、或内容不足一屏时为 `None`：这时比例恒等于 0，
    /// 推出去会把对端拽到文章开头（反过来对端推回来也会把编辑区拽到开头）。
    fn scroll_range(&self, cx: &App) -> Option<Pixels> {
        let viewport_h = self.editor.read(cx).input_bounds().size.height;
        if viewport_h <= px(0.) {
            return None;
        }
        let max = self.content_height(cx)? - viewport_h;
        (max > px(1.)).then_some(max)
    }

    /// 当前滚动位置在编辑区可滚动范围内的比例（0~1）
    fn ratio_now(&self, cx: &App) -> Option<f32> {
        let off = self.editor.read(cx).scroll_offset().y;
        let max = self.scroll_range(cx)?;
        Some((-off / max).clamp(0.0, 1.0))
    }

    /// 当前滚动位置对应的锚点
    fn current_anchor(&mut self, cx: &mut Context<Self>) -> Option<ScrollAnchor> {
        let note = self.note.as_ref()?;
        let total = note.line_count();
        let is_md = is_markdown(note.path());

        let (line_h, viewport_h, viewport_top, off) = {
            let st = self.editor.read(cx);
            (
                st.line_height()?,
                st.input_bounds().size.height,
                st.input_bounds().origin.y,
                st.scroll_offset().y,
            )
        };
        if line_h <= px(0.) || viewport_h <= px(0.) {
            return None;
        }

        // 数据文件：预览是格式化后的文本，源码行号与预览块对不上 → 只能用比例
        let by_ratio = !is_md || crate::app::config(cx).preview.sync_mode != ScrollSyncMode::Anchor;
        if by_ratio {
            return Some(ScrollAnchor::Ratio(self.ratio_now(cx)?));
        }

        // 估算值：不换行时它就是精确值，软换行时作为探测失败后的兜底
        let estimated = (-off / line_h).clamp(0.0, (total.saturating_sub(1)) as f32);
        let line = if self.applied_soft_wrap {
            // 软换行：一个源码行可能占多个显示行，只能靠真实布局探测
            self.probe_top_line(cx, viewport_top, estimated)
                .unwrap_or(estimated)
        } else {
            estimated
        };
        Some(ScrollAnchor::Line(line))
    }

    /// 这一帧的位置变化，是不是"我们自己刚写入的余波"
    ///
    /// `set_scroll_offset` 是 deferred 的，落点还会被 clamp、被软换行重排、
    /// 被精调修正，所以"写入目标"和"下一帧读到的位置"经常差几像素。
    /// 只按像素判定就会把自己写的结果当成用户手势 → 开静默窗口 →
    /// 窗口结束后再推一次 → 对端又写回来 → **两端互推 = 持续抖动**。
    /// 这也是"停止后对齐"仍然抖的原因：那个模式下只写一次，但这一次
    /// 足以启动一轮互推。
    fn is_own_write(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(anchor) = self.current_anchor(cx) else {
            return false;
        };
        // 阈值按锚点种类给，与下面推送前的回显判定保持一致
        let tol = match anchor {
            ScrollAnchor::Line(_) => 1.5,
            ScrollAnchor::Ratio(_) => 0.002,
        };
        self.guard.is_echo(anchor, tol)
    }

    /// 采样"一个源码行大约占多少像素"（软换行下内容高度估算用）
    ///
    /// 比例方案的准确度全看内容高度估算：软换行下按"行数 × 行高"算会系统性
    /// 低估，滚动到底部时两端比例对不上，表现就是"不同步"。这里用真实布局
    /// 探两次顶部行，用位移 ÷ 行数得到实测值，再指数平滑。
    fn learn_px_per_line(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.note.as_ref() else {
            return;
        };
        let total = note.line_count();
        let (line_h, viewport_top, off) = {
            let st = self.editor.read(cx);
            let Some(line_h) = st.line_height() else {
                return;
            };
            (line_h, st.input_bounds().origin.y, st.scroll_offset().y)
        };
        if line_h <= px(0.) {
            return;
        }
        let estimated = (-off / line_h).clamp(0.0, total.saturating_sub(1) as f32);
        let Some(line) = self.probe_top_line(cx, viewport_top, estimated) else {
            return;
        };
        if let Some((prev_line, prev_off)) = self.last_probe {
            let d_line = (line - prev_line).abs();
            let d_off = (off - prev_off).abs();
            // 只取跨了足够多行的样本，单行的测量噪声太大
            if d_line >= 2.0 && d_off > px(0.) {
                let sampled = d_off / d_line;
                let current = if self.px_per_source_line > px(0.) {
                    self.px_per_source_line
                } else {
                    sampled
                };
                self.px_per_source_line = current * 0.8 + sampled * 0.2;
            }
        }
        self.last_probe = Some((line, off));
        self.last_top_line = line;
    }

    /// 软换行下探测"视口顶部落在第几源码行"
    ///
    /// `range_to_bounds` 只对**已布局且可见**的区间返回 `Some`，所以不能二分全表，
    /// 只能以 `last_top_line`（上一帧结果）为起点在附近探几个候选行。
    fn probe_top_line(
        &self,
        cx: &mut Context<Self>,
        viewport_top: Pixels,
        estimated: f32,
    ) -> Option<f32> {
        // 候选行越少越省（每帧都要问一次布局），代价是跳转后收敛慢一帧。
        // 滚动是连续的，上一帧的行号基本就在附近，2/4 足够。
        const BACK: isize = 2;
        const FORWARD: isize = 4;
        /// 估算值与上一帧差距超过这么多行，说明是一次大跳转，探测中心改用估算值
        const JUMP: f32 = 12.0;

        let note = self.note.as_ref()?;
        let st = self.editor.read(cx);
        let center = if (estimated - self.last_top_line).abs() > JUMP {
            estimated as isize
        } else {
            self.last_top_line as isize
        };
        // 视口顶上方最近的一行 / 下方最近的一行，优先取上方
        let mut above: Option<(usize, Pixels, Pixels)> = None;
        let mut below: Option<(usize, Pixels, Pixels)> = None;

        for delta in -BACK..=FORWARD {
            let line = center + delta;
            if line < 0 {
                continue;
            }
            let line = line as usize;
            let Some((start, end)) = note.line_byte_range(line) else {
                continue;
            };
            let Some(bounds) = st.range_to_bounds(&(start..end)) else {
                continue;
            };
            let entry = (line, bounds.top(), bounds.size.height);
            if bounds.top() <= viewport_top {
                match above {
                    Some((_, top, _)) if top >= bounds.top() => {}
                    _ => above = Some(entry),
                }
            } else {
                match below {
                    Some((_, top, _)) if top <= bounds.top() => {}
                    _ => below = Some(entry),
                }
            }
        }

        let (line, top, height) = above.or(below)?;
        let frac = if height > px(0.) {
            ((viewport_top - top) / height).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Some(line as f32 + frac)
    }

    /// 这是不是"预览重解析被打回顶部"冒出来的假顶部信号
    ///
    /// 三条同时成立才算：内容刚重算过（[`Self::TOP_GUARD`] 内）、锚点指向
    /// 顶部、而编辑区此刻明显不在顶部。用户真把预览滚到顶部时编辑区通常
    /// 也在附近，不会被误伤；万一误伤也只是少同步一次，比被拽回开头轻得多。
    fn is_fake_top(&self, anchor: ScrollAnchor, now: Instant, cx: &App) -> bool {
        let Some(at) = self.content_changed_at else {
            return false;
        };
        if now.duration_since(at) > Self::TOP_GUARD {
            return false;
        }
        let at_top = match anchor {
            ScrollAnchor::Line(line) => line <= 0.5,
            ScrollAnchor::Ratio(ratio) => ratio <= 0.002,
        };
        if !at_top {
            return false;
        }
        match anchor {
            ScrollAnchor::Line(_) => self.last_top_line > 2.0,
            ScrollAnchor::Ratio(_) => self.ratio_now(cx).is_some_and(|r| r > 0.02),
        }
    }

    /// 应用预览推送过来的锚点
    fn apply_anchor(&mut self, anchor: ScrollAnchor, _window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        // 用户正在滚（滚轮 / 触控板惯性 / 拖滚动条）：一律让位。
        // 写滚动位置会覆盖同一帧的手势结果，表现就是"滚不动"或"被拉回"。
        if self.is_user_scrolling(now) {
            return;
        }
        // 用户正在打字：编辑区是权威端，这一刻预览的位置不可信
        // （输入防抖后预览会重解析，位置可能被打回顶部），听了就会被拽回开头。
        if let Some(at) = self.last_edit_at
            && now.duration_since(at) < Self::EDIT_PROTECT
        {
            return;
        }
        // 打字保护只有 800ms，而预览的假顶部信号要等"重解析 + 自身静默
        // 800ms"之后才推过来（约 1.1 秒）——正好落在保护窗口之外。
        // 这条单独拦那种"写完一会儿被拽回开头"。
        if self.is_fake_top(anchor, now, cx) {
            if crate::app::config(cx).preview.debug_scroll_sync {
                eprintln!(
                    "[xink][jump] 忽略预览推来的顶部锚点 {anchor:?}：内容刚重算过，\
                     编辑区顶部行 {:.1} / 比例 {:?}",
                    self.last_top_line,
                    self.ratio_now(cx)
                );
            }
            return;
        }

        if !matches!(anchor, ScrollAnchor::Line(_)) {
            // 比例锚点：一次到位
            self.apply_ratio(anchor.value(), cx);
            self.guard.expect(anchor);
            return;
        }
        // 收到行号却不在块锚点模式（对端刚切过来）→ 忽略，
        // 拿行号去粗调会让编辑区跳到不相干的位置
        if crate::app::config(cx).preview.sync_mode != ScrollSyncMode::Anchor {
            return;
        }

        let Some(note) = self.note.as_ref() else {
            return;
        };
        let total = note.line_count();
        let anchor = anchor.clamped(total.saturating_sub(1) as f32);
        let ScrollAnchor::Line(line) = anchor else {
            return;
        };

        let (line_h, viewport_h) = {
            let st = self.editor.read(cx);
            (st.line_height(), st.input_bounds().size.height)
        };
        let Some(line_h) = line_h else {
            return;
        };
        if viewport_h <= px(0.) {
            return;
        }

        // 粗调：把目标行带进视口。软换行下用实测的"每行像素数"，
        // 比"行号 × 行高"准得多（后者在折行时系统性偏上，会多花 1~2 帧精调，
        // 肉眼可见地跳一下）。
        let px_per_line = if self.px_per_source_line > px(0.) {
            self.px_per_source_line.max(line_h)
        } else {
            line_h
        };
        let content = px_per_line * total as f32;
        let max_scroll = (content - viewport_h).max(px(1.));
        let target_y = -(px_per_line * line).clamp(px(0.), max_scroll);
        self.trace_write(cx, "apply_anchor粗调", point(px(0.), target_y));
        self.editor.update(cx, |st, cx| {
            st.set_scroll_offset(point(px(0.), target_y), cx);
        });
        // 我们刚写过：记为当前位置，免得下一帧被当成"用户在滚"
        self.last_scroll_y = target_y;
        self.last_write_at = Some(Instant::now());

        if self.applied_soft_wrap {
            // 等布局出来后再精调（下一帧 refine_pending）
            self.pending_anchor = Some(anchor);
            self.pending_tries = 0;
        } else {
            self.guard.expect(anchor);
        }
    }

    /// 精调刚收到的锚点：用真实布局把目标行精确对齐到视口顶部
    fn refine_pending(&mut self, cx: &mut Context<Self>) {
        const MAX_TRIES: u8 = 3;

        // 用户在滚：放弃精调，把手势让给用户（tick 里已清过一次，这里再兜一道，
        // 因为手势可能在两次 tick 之间开始）
        if self.is_user_scrolling(Instant::now()) {
            self.pending_anchor = None;
            self.pending_tries = 0;
            return;
        }

        let Some(ScrollAnchor::Line(line)) = self.pending_anchor else {
            self.pending_anchor = None;
            return;
        };

        // 目标行的实时 bounds（只有它已经可见时才拿得到）
        let measured = {
            let Some(note) = self.note.as_ref() else {
                self.pending_anchor = None;
                return;
            };
            let st = self.editor.read(cx);
            let Some((start, end)) = note.line_byte_range(line as usize) else {
                self.pending_anchor = None;
                return;
            };
            st.range_to_bounds(&(start..end))
                .map(|bounds| (bounds.top(), bounds.size.height))
                .map(|(top, height)| {
                    (
                        top,
                        height,
                        st.input_bounds().origin.y,
                        st.scroll_offset().y,
                    )
                })
        };

        let Some((top, height, viewport_top, off)) = measured else {
            // 粗调的滚动还没生效：再等一帧，别无限等
            self.pending_tries += 1;
            if self.pending_tries >= MAX_TRIES {
                self.pending_anchor = None;
            }
            return;
        };

        // 目标行的顶边 + 行内比例 × 该行实际显示高度
        let target_y = top + (line - line as usize as f32) * height;
        let delta = target_y - viewport_top;
        if delta.abs() <= px(1.) {
            self.settle_pending(cx);
            return;
        }

        self.trace_write(cx, "精调", point(px(0.), off - delta));
        self.editor.update(cx, |st, cx| {
            st.set_scroll_offset(point(px(0.), off - delta), cx);
        });
        self.last_scroll_y = off - delta;
        self.last_write_at = Some(Instant::now());
        self.pending_tries += 1;
        if self.pending_tries >= MAX_TRIES {
            self.settle_pending(cx);
        }
    }

    /// 精调结束：以**实际落点**作为回显基准，下一帧读到它就吸收，不反向推
    fn settle_pending(&mut self, cx: &mut Context<Self>) {
        self.pending_anchor = None;
        self.pending_tries = 0;
        if let Some(actual) = self.current_anchor(cx) {
            self.guard.expect(actual);
        }
        // 精调是我们自己写的位置：记下来，免得被误判成用户在滚
        self.last_scroll_y = self.editor.read(cx).scroll_offset().y;
    }

    /// 按比例设置滚动位置
    ///
    /// 与 [`Self::ratio_now`] 共用 [`Self::content_height`]：读到的比例再写回去
    /// 一定回到同一个像素位置，回显天然落在容差内，不会互推。
    fn apply_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        // 可滚动范围拿不到（没布局 / 内容不足一屏）就别写：
        // 此时比例会被夹成 0，编辑区会被拽到文章开头
        let Some(max) = self.scroll_range(cx) else {
            return;
        };
        let target_y = -(ratio.clamp(0.0, 1.0) * max);
        self.trace_write(cx, "apply_ratio", point(px(0.), target_y));
        self.editor.update(cx, |st, cx| {
            st.set_scroll_offset(point(px(0.), target_y), cx);
        });
        self.last_scroll_y = target_y;
        self.last_write_at = Some(Instant::now());
    }

    /// 调度下一帧滚动同步轮询。框架在实体被移除后不再回调，循环自动停止。
    fn schedule_scroll_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, move |this, window, cx| {
            this.tick_scroll_sync(window, cx);
            this.schedule_scroll_tick(window, cx);
        });
    }

    /// 把编辑控件当前的文本同步进 Note
    ///
    /// 走 `text()` 借出 `&Rope`，不走 `value()`：后者的实现是
    /// `SharedString::new(self.text.to_string())`，每次调用都物化整个字符串，
    /// 而它每次按键会被调两次（这里一次、`maybe_complete_wikilink` 一次）。
    fn sync_note(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.note.as_mut() else {
            return;
        };
        let state = self.editor.read(cx);
        note.sync_from_rope(state.text());
    }

    fn on_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_note(cx);
        // 用户正在这篇笔记里打字：记为"编辑中"，期间不受对端的滚动指令
        // （见 [`Self::EDIT_PROTECT`]），否则一边打字一边被拽回开头。
        self.last_edit_at = Some(Instant::now());
        self.schedule_save(window, cx);
        // 光标前刚出现 `[[` 时给候选（只在这一刻弹，后续输入由浮层自己处理）
        self.maybe_complete_wikilink(window, cx);
        cx.notify();
    }

    /// 防抖任务：自动保存 + 预览刷新
    ///
    /// 延迟取自配置。**关闭自动保存只影响写盘，不影响预览刷新**——
    /// 否则用户关掉自动保存后编辑将完全没有预览反馈。
    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cfg = crate::app::config(cx);
        let delay = std::time::Duration::from_millis(cfg.editor.auto_save_delay_ms);
        let auto_save = cfg.editor.auto_save;
        self.save_task = Some(cx.spawn_in(window, async move |this, window| {
            window.background_executor().timer(delay).await;
            _ = this.update_in(window, |this, _window, cx| {
                // 这一整段是"编辑操作之后"的全部副作用：写盘、重算预览、
                // 重设装饰。其中任一步都可能让编辑控件重新布局并把滚动位置
                // 打回顶部——用户看到的就是"打完字跳回文章开头"。
                // 这里整段前后比对滚动位置与光标，被动过就还原（同步块内不会有
                // 用户操作插进来，所以还原是安全的）。
                let (before, before_cursor) = {
                    let state = this.editor.read(cx);
                    (state.scroll_offset(), state.cursor())
                };
                if auto_save {
                    this.save_now(cx);
                }
                // 保存、预览、装饰共用同一个防抖
                this.refresh_doc(cx);
                this.refresh_decorations(cx);
                let (after, after_cursor) = {
                    let state = this.editor.read(cx);
                    (state.scroll_offset(), state.cursor())
                };
                if (after.y - before.y).abs() > px(0.5) {
                    this.editor.update(cx, |state, cx| {
                        state.set_scroll_offset(before, cx);
                    });
                    this.last_scroll_y = before.y;
                    if crate::app::config(cx).preview.debug_scroll_sync {
                        eprintln!(
                            "[xink][scroll] 编辑后的刷新把滚动从 {before:?} 改成 {after:?}，已还原"
                        );
                    }
                }
                // 光标同理：被副作用挪走（常见是挪到 0）就还原，
                // 否则用户接着输入的字会落在文章开头。
                if after_cursor != before_cursor {
                    let at = before_cursor;
                    this.editor.update(cx, |state, cx| {
                        state.set_selected_range(at..at, cx);
                    });
                    if crate::app::config(cx).preview.debug_scroll_sync {
                        eprintln!(
                            "[xink][cursor] 编辑后的刷新把光标从 {before_cursor} 改成 \
                             {after_cursor}，已还原"
                        );
                    }
                }

                // 隔一帧再复核一次：控件对滚动/光标的改写常常是**下一帧才
                // 生效**的（布局在绘制阶段才落下去，落点还会被 clamp），
                // 本帧的 before/after 比对根本看不见，于是"写完一会儿跳回
                // 开头"就漏过去了。这里补一刀。
                let entity = this.self_entity.clone();
                let edit_at = this.last_edit_at;
                let expect_scroll = before;
                let expect_cursor = before_cursor;
                _window.defer(cx, move |window, cx| {
                    let Some(entity) = entity.upgrade() else {
                        return;
                    };
                    entity.update(cx, |this, cx| {
                        this.restore_view(expect_scroll, expect_cursor, edit_at, window, cx);
                    });
                });
            });
        }));
    }

    /// 隔一帧复核滚动与光标，被副作用挪动过就还原（[`Self::schedule_save`] 用）
    ///
    /// 两个前提不成立就直接放弃：期间用户又输入过（`edit_at` 变了，光标与
    /// 滚动本来就会跟着变）、或者用户正在滚（手势优先，绝不跟他抢）。
    fn restore_view(
        &mut self,
        scroll: Point<Pixels>,
        cursor: usize,
        edit_at: Option<Instant>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 期间又有输入：不能再插手，否则会把用户的新光标拽回去
        if self.last_edit_at != edit_at {
            return;
        }
        if self.is_user_scrolling(Instant::now()) {
            return;
        }
        let (now_scroll, now_cursor) = {
            let state = self.editor.read(cx);
            (state.scroll_offset(), state.cursor())
        };
        let debug = crate::app::config(cx).preview.debug_scroll_sync;

        // 只纠"明显跳变"，几像素的重排误差不去碰，免得跟框架互相推
        if (now_scroll.y - scroll.y).abs() > px(8.) || (now_scroll.x - scroll.x).abs() > px(8.) {
            let at = scroll;
            self.editor.update(cx, |state, cx| {
                state.set_scroll_offset(at, cx);
            });
            self.last_scroll_y = scroll.y;
            if debug {
                eprintln!(
                    "[xink][jump] 编辑后的刷新让滚动从 {scroll:?} 跳到 {now_scroll:?}，已还原"
                );
            }
        }
        if now_cursor != cursor {
            let at = cursor;
            self.editor.update(cx, |state, cx| {
                state.set_selected_range(at..at, cx);
            });
            if debug {
                eprintln!("[xink][jump] 编辑后的刷新让光标从 {cursor} 跳到 {now_cursor}，已还原");
            }
        }
    }

    /// 重算编辑区的 Markdown 装饰
    ///
    /// 装饰范围会随编辑**自动平移**，但内容变了必须由我们重设，否则 `#`
    /// 与链接的位置会错位。所以这里跟着 [`Self::schedule_save`] 的防抖一起跑。
    ///
    /// 只对 Markdown 笔记生效：数据文件走的是自己的 tree-sitter 高亮，
    /// 再叠一层 Markdown 装饰没有意义。
    fn refresh_decorations(&mut self, cx: &mut Context<Self>) {
        let text = match &self.note {
            Some(note) if is_markdown(note.path()) => note.text(),
            _ => {
                // 没有笔记 / 不是 Markdown：清空，避免残留上一篇的装饰
                if let Some(collection) = &self.decorations {
                    collection.clear(cx);
                }
                return;
            }
        };

        let quote_color = cx.theme().accent;
        let decorations: Vec<TextDecoration> = decoration_spans(&text)
            .into_iter()
            .map(|span| {
                let style = match span.kind {
                    // 行首标记淡化：弱化但不隐藏，源码始终完整可见
                    SpanKind::HeadingMarker | SpanKind::ListMarker => HighlightStyle {
                        fade_out: Some(MARKER_FADE_OUT),
                        ..Default::default()
                    },
                    // 块引用标记上色（框架没有行级绘制，画不出竖线）
                    SpanKind::QuoteMarker => HighlightStyle {
                        color: Some(quote_color),
                        ..Default::default()
                    },
                    // 链接文字下划线；颜色留空即跟随文字色
                    SpanKind::LinkText => HighlightStyle {
                        underline: Some(UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: false,
                        }),
                        ..Default::default()
                    },
                };
                TextDecoration::new(span.range, style)
            })
            .collect();

        if self.decorations.is_none() {
            let collection = self.editor.update(cx, |state, cx| {
                state.create_decorations_collection(Vec::new(), cx)
            });
            self.decorations = Some(collection);
        }
        if let Some(collection) = &self.decorations {
            // 装饰是全量重设的，编辑控件可能因此重新布局并把滚动位置打回顶部。
            // 输入防抖后就会跑一次，表现正是"打完字跳回文章开头"。
            // 这里前后比对，被动了就还原——不猜框架行为，只看事实。
            let before = self.editor.read(cx).scroll_offset();
            collection.set(decorations, cx);
            let after = self.editor.read(cx).scroll_offset();
            if (after.y - before.y).abs() > px(0.5) {
                self.editor.update(cx, |state, cx| {
                    state.set_scroll_offset(before, cx);
                });
                self.last_scroll_y = before.y;
                if crate::app::config(cx).preview.debug_scroll_sync {
                    eprintln!(
                        "[xink][scroll] 装饰刷新把滚动位置从 {:?} 改成 {:?}，已还原",
                        before, after
                    );
                }
            }
        }
    }

    /// 滚动写入的追踪日志（开 `preview.debug_scroll_sync` 时输出到 stderr）
    ///
    /// "编辑后跳回开头"这类问题只能靠落点日志定位：谁写的、写到哪、写之前在哪。
    fn trace_write(&self, cx: &App, tag: &str, target: Point<Pixels>) {
        if crate::app::config(cx).preview.debug_scroll_sync {
            let cur = self.editor.read(cx).scroll_offset();
            eprintln!("[xink][scroll] 写入[{tag}] 目标={target:?} 当前={cur:?}");
        }
    }

    /// 在光标处插入文本（供目录树"插入到当前笔记"使用）。
    ///
    /// 插入后走与用户输入相同的同步：更新笔记、防抖保存、重算预览。
    // ── AI 协助接口 ──────────────────────────────────────────
    //
    // 只暴露 AI 需要的最小面：读选中 / 读整篇与光标 / 回填结果。
    // 回填刻意用 `replace` 而不是 `set_value`：后者会把光标重置到 0，
    // 用户会被甩回文章开头。

    /// 当前选中的文本（无选中返回空串）
    pub fn selected_text(&self, cx: &App) -> String {
        self.editor.read(cx).selected_text().to_string()
    }

    /// 整篇文本与光标偏移（字节）。无笔记时返回 None
    pub fn note_text_and_cursor(&self, cx: &App) -> Option<(String, usize)> {
        let text = self.note.as_ref()?.text();
        let cursor = self.editor.read(cx).cursor();
        Some((text, cursor))
    }

    /// 直接跑一个 AI 动作：命令面板的 `AI:` 入口走这里（需求 A9.1）
    ///
    /// 对话条没开就先开——不然用户看不到 AI 在做什么，结果却已经写进来了。
    pub fn run_ai_action(&mut self, action: AiAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.note.is_none() {
            self.status = "没有打开的笔记：AI 动作要作用在一篇笔记上".into();
            cx.notify();
            return;
        }
        if !self.ai_open {
            self.toggle_ai_chat(window, cx);
        }
        if let Some(chat) = &self.ai_chat {
            chat.update(cx, |chat, cx| chat.run_action(action, window, cx));
        }
    }

    /// 打开 / 关闭编辑器下方的 AI 对话条
    pub fn toggle_ai_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_chat.is_none() {
            // 会话按笔记归属：建好就把这篇笔记的历史读回来
            let note_path = self.note.as_ref().map(|note| note.path().to_path_buf());
            let chat = cx.new(|cx| AiChat::new(window, cx, note_path));
            // 对话条自己请求关闭：这里只负责把开关拨回去
            cx.subscribe(&chat, |this, _, _: &AiChatEvent, cx| {
                this.ai_open = false;
                cx.notify();
            })
            .detach();
            self.ai_chat = Some(chat);
        }
        self.ai_open = !self.ai_open;
        // 开关状态打一行：快捷键到底有没有走到这里，一眼就能看出来
        eprintln!(
            "[xink] AI 对话条：{}",
            if self.ai_open { "打开" } else { "关闭" }
        );
        if self.ai_open {
            if let Some(chat) = &self.ai_chat {
                chat.update(cx, |chat, cx| chat.focus(window, cx));
            }
        }
        cx.notify();
    }

    /// 用 `text` 替换选中区；`whole` 为 true 时替换整篇。
    ///
    /// 返回是否真的改动了（无笔记时为 false）。
    pub fn apply_ai_result(
        &mut self,
        text: &str,
        whole: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.note.is_none() {
            return false;
        }
        self.editor.update(cx, |state, cx| {
            let range = if whole {
                0..state.text().len()
            } else {
                state.selected_range()
            };
            state.set_selected_range(range, cx);
            state.replace(text, window, cx);
        });
        self.on_change(window, cx);
        self.refresh_doc(cx);
        true
    }

    pub fn insert_at_cursor(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.note.is_none() {
            return;
        }
        self.editor
            .update(cx, |state, cx| state.insert(text, window, cx));
        self.editor.update(cx, |state, cx| state.focus(window, cx));
        self.on_change(window, cx);
        self.refresh_doc(cx);
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        let Some(note) = &mut self.note else {
            return;
        };
        if !note.is_dirty() {
            return;
        }
        // 先取路径：save() 需要 &mut note，之后还要用它更新索引
        let path = note.path().to_path_buf();
        match note.save() {
            Ok(()) => {
                self.status = "已保存".into();
                // 内容变更不会被监听当作结构变化，必须在这里把索引刷新
                crate::app::index_note_saved(&path, cx);
            }
            Err(err) => self.status = format!("保存失败: {err}").into(),
        }
        cx.notify();
    }

    /// Cmd+S 立即保存
    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
    }

    /// 磁盘文件被外部改动：没有未保存修改就自动重载，有则保留用户内容并提示
    ///
    /// 两条约束来自"正打字却被甩回文章开头"：
    /// 1. 内容没变就一个字节都不回填（见 [`Note::take_external_text`]）——
    ///    自己的写盘也会被监听报成改动，而 `set_value` 必然把光标重置到 0；
    /// 2. 用户正在打字时不打断，等停下（[`Self::EDIT_PROTECT`]）再回填。
    pub fn handle_external_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = &mut self.note else {
            return;
        };
        if !note.disk_changed() {
            return;
        }

        if note.is_dirty() {
            self.status = "文件已被外部修改，但你本地有未保存的更改".into();
            cx.notify();
            return;
        }

        // 打字期间回填必然打断输入：延后到停下之后。延后会再走一遍本函数，
        // 因此外部改动不会丢，只是晚一点生效。
        if self
            .last_edit_at
            .is_some_and(|at| at.elapsed() < Self::EDIT_PROTECT)
        {
            self.schedule_external_change(window, cx);
            return;
        }

        match note.take_external_text() {
            // 只有 mtime 变了（多为应用自己写盘的回声）：编辑控件无需改动
            Ok(None) => {}
            Ok(Some(text)) => {
                self.set_value_keep_place(text, window, cx);
                self.status = "已加载外部修改".into();
                self.refresh_doc(cx);
                self.activate_preview(cx);
            }
            Err(err) => self.status = format!("重新加载失败: {err}").into(),
        }
        cx.notify();
    }

    /// 整体替换编辑控件的内容，但保住光标与滚动位置。
    ///
    /// 框架的 `set_value` 末尾是 `reset_selection()` + `reset_scroll_to_start()`
    /// （gpui-base `input/base/state.rs::set_value`），多行输入的光标必然回到 0。
    /// 任何"程序在后台回填文本"的场景都必须走这里，否则用户正在打字就会被
    /// 甩回文章开头——字全打在第一行的开头。
    fn set_value_keep_place(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let (cursor, scroll) = {
            let state = self.editor.read(cx);
            (state.cursor(), state.scroll_offset())
        };
        let debug = crate::app::config(cx).preview.debug_scroll_sync;

        self.editor
            .update(cx, |state, cx| state.set_value(text, window, cx));
        self.editor.update(cx, |state, cx| {
            // 新文本可能更短：`set_selected_range` 内部会夹到文本范围内，
            // 这里不重复夹，免得把光标挪到字符边界之外
            state.set_selected_range(cursor..cursor, cx);
            state.set_scroll_offset(scroll, cx);
        });

        if debug {
            eprintln!(
                "[xink][cursor] 回填外部改动：光标 {:?} 滚动 {:?}",
                self.editor.read(cx).cursor(),
                self.editor.read(cx).scroll_offset()
            );
        }
        self.last_scroll_y = scroll.y;
    }

    /// 延后处理外部改动：等用户停下输入（`EDIT_PROTECT`）再回填
    fn schedule_external_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_external {
            return;
        }
        self.pending_external = true;
        let delay = Self::EDIT_PROTECT;
        cx.spawn_in(window, async move |this, window| {
            window.background_executor().timer(delay).await;
            _ = this.update_in(window, |this, window, cx| {
                this.pending_external = false;
                this.handle_external_change(window, cx);
            });
        })
        .detach();
    }

    /// 渲染顶部辅助编辑工具栏。
    ///
    /// - Markdown：把常用操作集中为一组快捷按钮（按钮是 gpui 的原生按钮，
    ///   点击时不会抢走编辑区焦点，选区得以保留，因此既能包裹选中文本，
    ///   也能在光标处插入空标记）；
    /// - 数据文件（json/yaml/toml）：Markdown 按钮全是误操作入口，换成
    ///   语言徽标 + 校验状态。
    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(kind) = self.data_kind() {
            return self.render_data_toolbar(kind, cx).into_any_element();
        }
        // 先取出主题色，避免在后续构建中同时发生不可变与可变借用
        let divider = cx.theme().muted_foreground.opacity(0.25);
        let border = cx.theme().border;
        let sep = || div().w(px(1.)).h(px(16.)).mx_1().bg(divider).flex_none();

        div()
            .flex_none()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .child(Self::toolbar_button(
                "fmt-bold",
                "加粗：包裹选中文本，或插入 ** 标记",
                Format::Bold,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-italic",
                "斜体：包裹选中文本，或插入 * 标记",
                Format::Italic,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-strike",
                "删除线：包裹选中文本，或插入 ~~ 标记",
                Format::Strikethrough,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-inline-code",
                "行内代码：包裹选中文本，或插入 ` 标记",
                Format::InlineCode,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-heading",
                "标题：插入 # 标题行",
                Format::Heading,
                cx,
            ))
            .child(sep())
            .child(Self::toolbar_button(
                "fmt-ul",
                "无序列表：插入 - 列表项",
                Format::BulletList,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-ol",
                "有序列表：插入 1. 列表项",
                Format::NumberedList,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-task",
                "任务列表：插入待办项 - [ ]",
                Format::TaskList,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-quote",
                "引用：插入 > 引用块",
                Format::Quote,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-code",
                "代码块：插入 ``` 代码块",
                Format::CodeBlock,
                cx,
            ))
            .child(sep())
            .child(Self::toolbar_button(
                "fmt-link",
                "链接：插入 [文本](地址)，无选中则插入占位",
                Format::Link,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-image",
                "图片：插入 ![描述](地址)，无选中则插入占位",
                Format::Image,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-table",
                "表格：插入 Markdown 表格模板",
                Format::Table,
                cx,
            ))
            .child(Self::toolbar_button(
                "fmt-hr",
                "分割线：插入 --- 水平分隔线",
                Format::HorizontalRule,
                cx,
            ))
            // AI 入口：与 Cmd-Shift-A 同一个开关。只有快捷键的话，
            // 这个对话条对没看过设计文档的人等于不存在——补个看得见的按钮。
            .child(Self::ai_button(cx))
            .into_any_element()
    }

    /// 数据文件模式的工具栏：语言徽标 + 校验状态。
    ///
    /// 只有这两样，没有"格式化并写入"——格式化结果只进预览，
    /// 程序不改写用户文件（YAML 往返会丢注释）。
    fn render_data_toolbar(&mut self, kind: DataKind, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let warning = cx.theme().warning;

        let (text, color) = match &self.validation {
            None => ("✓ 格式正确".to_string(), muted),
            Some(err) => (
                format!("⚠ {}：{}", err.display_position(), err.message),
                warning,
            ),
        };

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .text_xs()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .flex_none()
                    .px_1p5()
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(muted.opacity(0.15))
                    .text_color(muted)
                    .child(kind.label()),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(color)
                    .child(text),
            )
    }

    /// 工具栏上的 AI 入口，等价于 `Cmd-Shift-A`
    ///
    /// 对话条是"贴着编辑区"的形态（`docs/ai/ai-assistant-design.md` D1），
    /// 因此入口也放在编辑区自己的工具栏上：点了就在正下方展开。
    fn ai_button(cx: &mut Context<Self>) -> impl IntoElement {
        let icon_size = crate::app::config(cx).basic.icon_size;
        Button::new("fmt-ai")
            .small()
            .ghost()
            .icon(Icon::new(IconName::Bot).with_size(Size::Size(px(icon_size as f32))))
            .tooltip_with_action(
                "AI 对话：针对这篇笔记提问或改写（对话条在编辑区下方）",
                &ToggleAiChat,
                None,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_ai_chat(window, cx);
            }))
    }

    /// 创建一个工具栏按钮：仅显示图标，鼠标悬停时通过 tooltip 显示功能说明。
    ///
    /// 图标使用内联 SVG（见 `format_icon`）；按钮不显示文字标签，
    /// 功能说明完全由悬停提示（tooltip）承担，从而保持工具栏紧凑。
    /// 有快捷键的操作用 `tooltip_with_action`，让提示里直接带上键位。
    fn toolbar_button(
        id: &'static str,
        tooltip: &'static str,
        cmd: Format,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let icon_size = crate::app::config(cx).basic.icon_size;
        let button = Button::new(id)
            .small()
            .ghost()
            .icon(format_icon(cmd).with_size(Size::Size(px(icon_size as f32))));

        let button = match cmd.action() {
            Some(action) => button.tooltip_with_action(tooltip, action, None),
            None => button.tooltip(tooltip),
        };

        button.on_click(cx.listener(move |this, _, window, cx| {
            this.apply_format(cmd, window, cx);
        }))
    }

    /// 把常用的 Markdown 片段插入到编辑区光标处（或包裹选中文本）。
    fn apply_format(&mut self, cmd: Format, window: &mut Window, cx: &mut Context<Self>) {
        let Some(_) = &self.note else {
            return;
        };
        // 数据文件不提供 Markdown 工具栏，快捷键同理要挡住：
        // 往 JSON/YAML 里插 `**` 只会把文件写坏，且校验会立刻报错
        if self.data_kind().is_some() {
            return;
        }
        self.editor.update(cx, |state, cx| match cmd.insertion() {
            Insertion::Surround { before, after } => surround(state, window, cx, before, after),
            Insertion::Block(text) => block(state, window, cx, text),
            Insertion::Template { before, after } => {
                insert_template(state, window, cx, before, after)
            }
        });
        // 重新聚焦编辑区：让光标可见，后续输入继续落在笔记里
        self.editor.update(cx, |state, cx| state.focus(window, cx));
        self.status = format!("已插入：{}", cmd.label()).into();
        // 与高亮同理：程序化插入不触发编辑控件的 Change 事件，
        // 必须显式同步（含立即重算预览），否则要等用户下一次输入才刷新
        self.on_change(window, cx);
        self.refresh_doc(cx);
    }
}

impl EventEmitter<PanelEvent> for EditorPanel {}

impl Focusable for EditorPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for EditorPanel {
    fn panel_name(&self) -> &'static str {
        "EditorPanel"
    }

    /// 面板被加入标签组时回调（签名由 BasePanel trait 规定）。
    fn on_added_to(
        &mut self,
        _group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    /// 成为当前显示的标签时同步预览，并把它记为活动标签（供下次启动恢复）。
    /// 这里每个标签切换只触发一次，替代原先在 render 里每帧检查焦点的做法。
    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if !active {
            return;
        }
        self.activate_preview(cx);
        let Some(path) = self.note.as_ref().map(|note| note.path().to_path_buf()) else {
            crate::ui::outline::clear_outline(cx);
            return;
        };
        // 切标签：大纲要跟着换成这一篇
        if let Some(note) = &self.note {
            crate::ui::outline::update_outline(&path, note.text().as_str(), cx);
        }
        let key = tab_key(&path);
        cx.update_global::<AppState, _>(|state, _| state.tabs.activate(&key));
        // 目录树跟随：切到这篇笔记时把树定位过去（展开祖先 + 选中 + 滚动）。
        // 需求见 `docs/file-tree/file-tree-reveal-active-note-requirements.md`。
        crate::ui::file_tree::reveal_note(&path, cx);
        crate::ui::tabs::persist_open_notes(cx);
    }

    /// 标签被关闭：从"已打开"登记中移除并落盘，
    /// 否则下次点同一文件会复用到失效的实体，重启也会恢复出已关闭的标签。
    fn on_removed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.note.as_ref().map(|n| n.path().to_path_buf()) else {
            return;
        };
        cx.update_global::<AppState, _>(|state, _| {
            state.tabs.remove(&tab_key(&path));
        });
        crate::ui::tabs::persist_open_notes(cx);
    }
}

impl Panel for EditorPanel {
    /// 标签页标题（含关闭按钮）。
    ///
    /// 关键点：tab 上没有独立的关闭按钮扩展点——只有当 `tab_name()` 返回 None 时，
    /// tab 才会直接渲染 `title()` 返回的元素。这里正是利用这一点把 X 放进标题，
    /// 于是每个标签右侧都有关闭按钮，与 Obsidian 一致。
    // 需要返回两种不同类型（纯文本占位 / 带按钮的元素），故统一为 AnyElement
    #[allow(refining_impl_trait)]
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> AnyElement {
        // 没有打开笔记时是空白占位页：不给关闭按钮，编辑区需要始终保留
        let Some(note) = &self.note else {
            return "编辑".to_string().into_any_element();
        };
        let mark = if note.is_dirty() { " ●" } else { "" };
        let name = format!("{}{}", note.name(), mark);

        let weak = self.self_entity.clone();
        // id 必须按面板区分，否则多个标签的按钮会互相冲突
        let button_id = ("close-tab", self.self_entity.entity_id());

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(name)
            .child(
                div()
                    // 独占点击，避免同时触发标签的选中
                    .occlude()
                    .child(
                        Button::new(button_id)
                            // 图标按钮默认是 32×32（Size::Medium/Large → size_8），
                            // 而图标只有十几像素，四周留白过多显得空旷。
                            // 这里显式把按钮收到 18×18 并指定 13px 图标，
                            // 让按钮尺寸紧贴图标本身（每边留白约 2.5px）。
                            .icon(Icon::new(IconName::Close).with_size(Size::Size(px(13.))))
                            .with_size(Size::Size(px(18.)))
                            .on_click(move |_, window, cx| {
                                close_editor_tab(weak.clone(), window, cx);
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// 关闭一个笔记标签。
///
/// 关键规则：**最后一个标签不清空区域，而是把面板置为空白占位**。
/// 若直接移除，编辑区会整体消失，下次打开笔记也失去落点。
fn close_editor_tab(weak: WeakEntity<EditorPanel>, window: &mut Window, cx: &mut App) {
    let Some(panel) = weak.upgrade() else {
        return;
    };
    let Some(state) = cx.try_global::<AppState>() else {
        return;
    };

    let is_last = state.tabs.len() <= 1;
    let dock_area = state.dock_area.clone();

    if is_last {
        // 保留编辑区，清空内容
        panel.update(cx, |editor, cx| editor.clear(window, cx));
    } else {
        dock_area.update(cx, |area, cx| {
            area.remove_panel(panel, window, cx);
        });
    }

    // 关闭后按不变式同步空白页（幂等，重复调用无副作用）
    crate::app::sync_placeholder(window, cx);
}

/// 用当前索引解析双链目标（预览改写用）。
///
/// 索引只读：解析不到时返回 `None`，`rewrite_wikilinks` 会把原文原样留着。
fn resolve_wikilink(cx: &App, target: &str) -> Option<PathBuf> {
    let index = cx.try_global::<AppState>()?.notes.clone();
    index.read(cx).resolve_path(target)
}

/// 数据文件的预览源文：顶部提示 + 格式化后的围栏代码块。
///
/// 返回 `(源文, 校验结果)`：调用方拿校验结果去更新状态，避免重复解析一遍。
///
/// - 合法 → 展示格式化结果；非法 → 展示原文并在顶部给出行列与原因；
/// - 超过 `max_format_bytes` → 跳过格式化，只展示原文并说明原因；
/// - 格式化结果**只用于预览**，写盘永远用用户原文（见 `core::datafile` 模块文档）。
fn data_preview_source(
    text: &str,
    kind: DataKind,
    cfg: &DataFileConfig,
) -> (String, Option<ValidationError>) {
    let code;
    let mut error = None;
    let mut extra_notice: Option<String> = None;

    if cfg.max_format_bytes > 0 && text.len() > cfg.max_format_bytes {
        // 大文件格式化会卡住预览，宁可不排版
        code = text.to_string();
        extra_notice = Some("文件过大，已跳过格式化".to_string());
    } else {
        match crate::core::datafile::parse(text, kind, cfg.indent) {
            Parsed::Ok { formatted } => code = formatted,
            Parsed::Err { error: err } => {
                code = text.to_string();
                extra_notice = Some(format!("{}：{}", err.display_position(), err.message));
                error = Some(err);
            }
        }
    }

    let mut out = String::new();
    if let Some(notice) = kind.notice() {
        out.push_str(notice);
        out.push_str("\n\n");
    }
    if let Some(notice) = extra_notice {
        out.push_str("⚠️ ");
        out.push_str(&notice);
        out.push_str("\n\n");
    }
    // 已知限制：内容里若含 ``` 会截断代码块。JSON/YAML/TOML 极少出现，
    // 且最坏只是预览排版错乱，不会崩。
    out.push_str(&format!("```{}\n{code}\n```\n", kind.lang()));

    (out, error)
}

impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let started = Instant::now();
        // 滚动同步由帧轮询（schedule_scroll_tick）驱动，不在这里每帧做

        // 缩进配置变更时增量下发给编辑控件，使"改设置"立即生效。
        // 用 applied_indent 守卫，避免每帧重复下发造成无谓重绘。
        let indent = crate::app::config(cx).editor.indent;
        if indent != self.applied_indent {
            self.applied_indent = indent;
            self.editor.update(cx, |state, cx| {
                state.set_tab_size(
                    TabSize {
                        tab_size: indent.tab_size as usize,
                        hard_tabs: indent.use_tabs,
                    },
                    cx,
                );
            });
        }

        // 行号开关变更时同样增量下发，改设置后下一帧即生效
        let show_line_numbers = crate::app::config(cx).editor.show_line_numbers;
        if show_line_numbers != self.applied_line_numbers {
            self.applied_line_numbers = show_line_numbers;
            self.editor.update(cx, |state, cx| {
                state.set_line_number(show_line_numbers, window, cx);
            });
        }

        // 软换行开关变更时增量下发。关掉后"源码行 ↔ 滚动位置"是一次乘除就能算准的，
        // 滚动同步也随之更精确；开启后改走"按真实布局探测"的路径。
        let soft_wrap = crate::app::config(cx).editor.soft_wrap;
        if soft_wrap != self.applied_soft_wrap {
            self.applied_soft_wrap = soft_wrap;
            self.editor.update(cx, |state, cx| {
                state.set_soft_wrap(soft_wrap, window, cx);
            });
            // 换算口径变了，把同步状态清掉重新对齐
            self.reset_scroll_sync();
        }

        // 图片显示方式 / 宽度变化时请求重算预览源文。
        // 是否真的重算由 `refresh_doc` 内部守卫判断（那里也会记录当前选项），
        // 这里只负责在配置变化时触发一次。
        if Some(image_options(cx)) != self.applied_image_opts {
            self.refresh_doc(cx);
        }

        // 数据文件配置（缩进 / 大小阈值）变化时同样请求重算预览
        if crate::app::config(cx).editor.data_file != self.applied_data_cfg {
            self.refresh_doc(cx);
        }

        // 未打开任何笔记时显示占位提示，编辑区本身始终存在
        let body = if self.note.is_some() {
            div().flex_1().min_h_0().child({
                let font = &crate::app::config(cx).editor.font;
                Editor::new(&self.editor)
                    .size_full()
                    .appearance(false)
                    .bordered(false)
                    .font_family(font.family.clone())
                    .text_size(px(font.size))
                    .line_height(px(font.size * font.line_height))
                    // 字重接线：之前只应用了字体三件套，weight 配了不生效
                    .font_weight(FontWeight(font.weight.value()))
            })
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child("没有打开的笔记 —— 在左侧文件树中点开一篇")
        };

        let mut view = div()
            .id("editor-panel")
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(Self::save))
            // Markdown 行内格式快捷键：与工具栏按钮共用 `apply_format`，
            // 不再写第二套插入逻辑（Action → 操作的映射见 `Format::action`）
            .on_action(cx.listener(|this, _: &FormatBold, window, cx| {
                this.apply_format(Format::Bold, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatItalic, window, cx| {
                this.apply_format(Format::Italic, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatStrikethrough, window, cx| {
                this.apply_format(Format::Strikethrough, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatInlineCode, window, cx| {
                this.apply_format(Format::InlineCode, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatLink, window, cx| {
                this.apply_format(Format::Link, window, cx)
            }));

        // 仅在打开笔记时显示辅助编辑工具栏
        if self.note.is_some() {
            view = view.child(self.render_toolbar(cx));
        }

        view = view.child(body);

        // AI 对话条：贴在编辑区下方，聊天对象就是当前这篇笔记
        if self.ai_open {
            if let Some(chat) = &self.ai_chat {
                view = view.child(chat.clone());
            }
        }

        view = view.child(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .child(self.status.clone()),
        );

        crate::ui::log_slow("EditorPanel::render", started);
        view
    }
}

/// 工具栏支持的 Markdown 辅助编辑操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    Heading,
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    CodeBlock,
    Table,
    Link,
    Image,
    HorizontalRule,
}

impl Format {
    /// 用于状态栏提示的中文名称。
    fn label(&self) -> &'static str {
        match self {
            Format::Bold => "加粗",
            Format::Italic => "斜体",
            Format::Strikethrough => "删除线",
            Format::InlineCode => "行内代码",
            Format::Heading => "标题",
            Format::BulletList => "无序列表",
            Format::NumberedList => "有序列表",
            Format::TaskList => "任务列表",
            Format::Quote => "引用",
            Format::CodeBlock => "代码块",
            Format::Table => "表格",
            Format::Link => "链接",
            Format::Image => "图片",
            Format::HorizontalRule => "分割线",
        }
    }

    /// 该操作实际插入的 Markdown 内容。
    ///
    /// 抽成纯数据后，插入逻辑统一由 `apply_format` 驱动，
    /// 也让「每个按钮会插入什么语法」可以被单元测试直接断言。
    fn insertion(&self) -> Insertion {
        match self {
            Format::Bold => Insertion::Surround {
                before: "**",
                after: "**",
            },
            Format::Italic => Insertion::Surround {
                before: "*",
                after: "*",
            },
            Format::Strikethrough => Insertion::Surround {
                before: "~~",
                after: "~~",
            },
            Format::InlineCode => Insertion::Surround {
                before: "`",
                after: "`",
            },
            Format::Link => Insertion::Surround {
                before: "[",
                after: "](url)",
            },
            Format::Image => Insertion::Surround {
                before: "![",
                after: "](url)",
            },
            Format::Heading => Insertion::Block("# "),
            Format::BulletList => Insertion::Block("- "),
            Format::NumberedList => Insertion::Block("1. "),
            Format::TaskList => Insertion::Block("- [ ] "),
            Format::Quote => Insertion::Block("> "),
            Format::HorizontalRule => Insertion::Block("---\n"),
            Format::CodeBlock => Insertion::Template {
                before: "```\n",
                after: "\n```\n",
            },
            Format::Table => Insertion::Template {
                before: "| 列1 | 列2 |\n| --- | --- |\n| ",
                after: " |     |\n",
            },
        }
    }

    /// 该操作对应的应用级 Action，没有快捷键的操作返回 `None`。
    ///
    /// 工具栏按钮拿它去渲染快捷键提示（框架直接从键位表反查，
    /// 因此提示永远不会与实际绑定脱节）；键位本身在 `main.rs` 注册。
    fn action(&self) -> Option<&'static dyn Action> {
        match self {
            Format::Bold => Some(&FormatBold),
            Format::Italic => Some(&FormatItalic),
            Format::Strikethrough => Some(&FormatStrikethrough),
            Format::InlineCode => Some(&FormatInlineCode),
            Format::Link => Some(&FormatLink),
            _ => None,
        }
    }
}

/// 操作的插入方式，决定插入什么文本以及操作后光标如何定位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Insertion {
    /// 行内包裹：有选中文本则整体包裹，否则插入一对标记并把光标置于中间。
    Surround {
        before: &'static str,
        after: &'static str,
    },
    /// 块级前缀：在行首插入（光标不在行首时自动补一个换行）。
    Block(&'static str),
    /// 多行模板：插入 `before` 与 `after`，光标停在两者之间（代码块 / 表格）。
    Template {
        before: &'static str,
        after: &'static str,
    },
}

/// 每个格式化操作对应的工具栏图标（Lucide 风格内联 SVG）。
///
/// 使用 `stroke="currentColor"`，图标颜色随主题文字色变化；通过 `Icon::data`
/// 以字节形式内联，无需向资源包注册新图标。
fn format_icon_svg(cmd: Format) -> &'static str {
    match cmd {
        Format::Bold => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 12h8a4 4 0 0 0 0-8H6v8Z"/><path d="M6 12h9a4 4 0 0 1 0 8H6v-8Z"/></svg>"#
        }
        Format::Italic => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="19" y1="4" x2="10" y2="4"/><line x1="14" y1="20" x2="5" y2="20"/><line x1="15" y1="4" x2="9" y2="20"/></svg>"#
        }
        Format::Strikethrough => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 4H9a3 3 0 0 0-2.83 4"/><path d="M14 12a4 4 0 0 1 0 8H6"/><line x1="4" y1="12" x2="20" y2="12"/></svg>"#
        }
        Format::InlineCode => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="m10 9-2 3 2 3"/><path d="m14 9 2 3-2 3"/></svg>"#
        }
        Format::Heading => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 4v16"/><path d="M18 4v16"/><path d="M6 12h12"/></svg>"#
        }
        Format::BulletList => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="9" y1="6" x2="20" y2="6"/><line x1="9" y1="12" x2="20" y2="12"/><line x1="9" y1="18" x2="20" y2="18"/><circle cx="4.5" cy="6" r="1"/><circle cx="4.5" cy="12" r="1"/><circle cx="4.5" cy="18" r="1"/></svg>"#
        }
        Format::NumberedList => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="10" y1="6" x2="21" y2="6"/><line x1="10" y1="12" x2="21" y2="12"/><line x1="10" y1="18" x2="21" y2="18"/><path d="M4 6h1v4"/><path d="M4 10h2"/><path d="M6 18H4c0-1 2-2 2-3s-1-1.5-2-1"/></svg>"#
        }
        Format::TaskList => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 17 2 2 4-4"/><path d="m3 7 2 2 4-4"/><path d="M13 6h8"/><path d="M13 12h8"/><path d="M13 18h8"/></svg>"#
        }
        Format::Quote => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 7h4v5c0 2.5-2 4.5-5 5"/><path d="M16 7h4v5c0 2.5-2 4.5-5 5"/></svg>"#
        }
        Format::CodeBlock => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m16 18 6-6-6-6"/><path d="m8 6-6 6 6 6"/></svg>"#
        }
        Format::Table => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="3" y1="15" x2="21" y2="15"/><line x1="9" y1="3" x2="9" y2="21"/><line x1="15" y1="3" x2="15" y2="21"/></svg>"#
        }
        Format::Link => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>"#
        }
        Format::Image => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg>"#
        }
        Format::HorizontalRule => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="3" y1="12" x2="21" y2="12"/><polyline points="7 9 12 4 17 9"/><polyline points="7 15 12 20 17 15"/></svg>"#
        }
    }
}

/// 把操作对应的 SVG 包装为 `Icon`，供工具栏按钮使用。
fn format_icon(cmd: Format) -> Icon {
    Icon::default().data(format_icon_svg(cmd).as_bytes())
}

/// 在光标处插入一对标记并把光标置于中间；若有选中文本则整体包裹。
fn surround(
    state: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
    before: &str,
    after: &str,
) {
    let range = state.selected_range();
    if range.start != range.end {
        let selected: String = state.selected_text().to_string();
        state.replace(format!("{before}{selected}{after}"), window, cx);
    } else {
        state.insert(before, window, cx);
        let between = state.cursor_position();
        state.insert(after, window, cx);
        state.set_cursor_position(between, window, cx);
    }
}

/// 由光标前一个字符判断是否处于行首：没有前驱字符，或前驱是换行。
fn is_line_start(prev_char: Option<char>) -> bool {
    prev_char.is_none_or(|c| c == '\n')
}

/// 光标是否处于行首（偏移为 0 或前一个字符是换行）。
fn at_line_start(state: &EditorState) -> bool {
    let offset = state.cursor();
    if offset == 0 {
        return true;
    }
    let before = state.text().slice(0..offset);
    is_line_start(before.chars().last())
}

/// 在光标处插入一段块级文本，必要时先补一个换行使其自成一块。
fn block(state: &mut EditorState, window: &mut Window, cx: &mut Context<EditorState>, text: &str) {
    if !at_line_start(state) {
        state.insert("\n", window, cx);
    }
    state.insert(text, window, cx);
}

/// 插入多段模板文本（代码块 / 表格共用），并把光标置于 `before` 与 `after` 之间。
///
/// 若光标不在行首，先补一个换行使其自成一块。
fn insert_template(
    state: &mut EditorState,
    window: &mut Window,
    cx: &mut Context<EditorState>,
    before: &str,
    after: &str,
) {
    if !at_line_start(state) {
        state.insert("\n", window, cx);
    }
    state.insert(before, window, cx);
    let between = state.cursor_position();
    state.insert(after, window, cx);
    state.set_cursor_position(between, window, cx);
}

/// 从当前配置取出图片渲染选项（预览源文生成时用）
fn image_options(cx: &App) -> ImageOptions {
    let cfg = crate::app::config(cx);
    ImageOptions {
        display: cfg.preview.image_display,
        width_percent: cfg.preview.image_width_percent,
    }
}

/// 计算"让第 `line` 行（0-based）滚到视口顶部"所需的纵向滚动量。
///
/// 返回值恒为**非正数**（内容向上滚），且不小于 `-(content - viewport)`：
/// 结尾几行滚不到顶部，也不能让视口越过文档底部。
///
/// **坑**：`-(line_h * row).clamp(-max, px(0.))` 是错的。`line_h * row`
/// 是正数，会被 `clamp` 的上界 0 压成 0，再取负得 `-0`，滚动量永远归零
/// （表现就是"点了大纲，编辑区纹丝不动"）。必须**先取负再 clamp**。
fn scroll_offset_for_line(
    line: usize,
    total_lines: usize,
    line_height: Pixels,
    viewport: Pixels,
) -> Pixels {
    let content = line_height * total_lines as f32;
    if content <= viewport {
        // 内容不足一屏：没有可滚动的空间
        return px(0.);
    }
    let max = content - viewport;
    let row = (line as f32).min((total_lines as f32 - 1.).max(0.));
    (-(line_height * row)).clamp(-max, px(0.))
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/editor.rs"]
mod tests;
