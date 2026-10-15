//! 中间编辑面板：Editor 编辑 + 防抖自动保存 + 原子写盘

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::base::TextViewState;
use gpui_kit::base::dock::TabGroup;
use gpui_kit::base::input::{Diagnostic, Position};
use gpui_kit::component::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::input::{Editor, EditorState, InputEvent, TabSize};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, Size};
use gpui_kit::*;

use crate::actions::{
    FormatBold, FormatInlineCode, FormatItalic, FormatLink, FormatStrikethrough, Save,
};
use crate::app::{ActiveDocChanged, AppState, EditorScrolled, PreviewScrolled};
use crate::core::config::{DataFileConfig, IndentConfig};
use crate::core::datafile::{DataKind, Parsed, ValidationError, data_kind};
use crate::core::markdown::{ImageOptions, resolve_image_urls, sanitize_for_preview};
use crate::core::{Note, tab_key};

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
    status: SharedString,
    /// 上次推送给预览的滚动比例，用于判断是否需要再次同步
    last_emitted_ratio: f32,
    /// 上次从预览接收并应用的滚动比例，用于吸收回显、避免同步循环
    last_received_ratio: f32,
    /// 上次推送给预览的内容标识（路径 + 版本），用于跳过无变化的重算
    last_published_path: Option<PathBuf>,
    last_published_version: Option<u64>,
    /// 自己的弱引用，供关闭按钮精确移除本面板
    self_entity: WeakEntity<EditorPanel>,
}

impl EditorPanel {
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
            status: SharedString::default(),
            last_emitted_ratio: 0.0,
            last_received_ratio: 0.0,
            last_published_path: None,
            last_published_version: None,
            self_entity: self_entity.clone(),
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
                    let PreviewScrolled(ratio) = ev;
                    let is_active = cx.try_global::<AppState>().map_or(false, |s| {
                        s.preview.read(cx).active_doc.as_ref() == this.doc.as_ref()
                    });
                    if is_active {
                        this.apply_preview_ratio(*ratio, window, cx);
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
        let (source, validation) = Self::preview_source(note, opts, &data_cfg);
        self.validation = validation;
        self.sync_diagnostics(cx);
        if let Some(doc) = &self.doc {
            doc.update(cx, |doc, cx| doc.set_text(source.as_str(), cx));
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
    ) -> (String, Option<ValidationError>) {
        let text = note.text();
        if let Some(kind) = data_kind(note.path()) {
            return data_preview_source(&text, kind, data_cfg);
        }
        let dir = note.path().parent().unwrap_or(Path::new(""));
        // 先做防御性预处理，避免畸形 Markdown 让预览解析器 panic 导致应用退出
        let safe = sanitize_for_preview(&text);
        (resolve_image_urls(&safe, dir, opts), None)
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
                // 数据文件按语言高亮；Markdown 用 "text"（沿用现状：不高亮）。
                //
                // 每次载入都显式设置：面板会被复用（空白页认领机制），
                // 不重置的话"关掉 json 再打开 md"会残留 json 的高亮配色。
                let lang = data_kind(note.path())
                    .map(|kind| kind.lang())
                    .unwrap_or("text");
                self.editor.update(cx, |state, cx| {
                    state.set_highlighter(lang, cx);
                });
                self.status = format!("已打开 {}", note.path().display()).into();
                // 每篇笔记一个预览文档，首次解析后切回来就无需再解析
                let (source, validation) =
                    Self::preview_source(&note, image_options(cx), &data_cfg);
                self.applied_data_cfg = data_cfg;
                self.validation = validation;
                self.doc = Some(cx.new(|cx| TextViewState::markdown(source.as_str(), cx)));
                self.note = Some(note);
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

    /// 在所属标签组中选中自己。

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

        self.editor
            .update(cx, |state, cx| state.set_value("", window, cx));
        // 清空后不留上一篇的波浪线
        self.sync_diagnostics(cx);

        let self_entity = self.self_entity.clone();
        cx.update_global::<AppState, _>(|state, cx| {
            if let Some(path) = &path {
                state.tabs.remove(&tab_key(path));
            }
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

    /// 每帧轮询编辑区滚动比例，变化超阈值则推给预览。
    ///
    /// 同步循环防护：
    /// - `last_received_ratio` 记录"上次从预览接收并应用"的比例。若当前比例与之一致，
    ///   说明本次变化是"预览→编辑器"同步的回显，直接吸收，不再反向推送，避免死循环。
    /// - 否则与 `last_emitted_ratio` 比较，确属真实用户滚动才推送。
    fn tick_scroll_sync(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        const EPS: f32 = 0.002;

        // 编辑器侧也判断开关：关闭后不再向预览推送滚动比例。
        // 配置在每次触发时读取，因此改设置后下一次滚动即生效（L2）。
        if !crate::app::config(cx).preview.sync_scroll {
            return;
        }

        let ratio = if let Some(note) = &self.note {
            let (off, viewport, line_h) = {
                let st = self.editor.read(cx);
                (
                    st.scroll_offset().y,
                    st.input_bounds().size.height,
                    st.line_height(),
                )
            };
            let Some(line_h) = line_h else {
                return;
            };
            if viewport <= px(0.) {
                return;
            }
            // editor 内部滚动，无直接内容高度 API，用行高×行数估算。
            // 左右两侧共用同一估算，保证"相对位置一致"。
            let content = line_h * note.line_count() as f32;
            let max = (content - viewport).max(px(1.));
            (-off / max).clamp(0.0, 1.0)
        } else {
            // 空白占位页：无内容可同步
            self.last_emitted_ratio = 0.0;
            self.last_received_ratio = 0.0;
            return;
        };

        // 回显吸收：本次比例与"预览给我的目标"一致 → 是我刚应用的同步结果
        if (ratio - self.last_received_ratio).abs() <= EPS {
            self.last_emitted_ratio = ratio;
            return;
        }
        // 真实用户滚动：与上一次推送一致则跳过
        if (ratio - self.last_emitted_ratio).abs() <= EPS {
            return;
        }
        self.last_emitted_ratio = ratio;
        cx.update_global::<AppState, _>(|state, cx| {
            state.preview.update(cx, |_, cx| {
                cx.emit(EditorScrolled(ratio));
            });
        });
    }

    /// 应用预览推送过来的滚动比例到本编辑器。
    fn apply_preview_ratio(&mut self, ratio: f32, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = &self.note else {
            return;
        };
        let (viewport, line_h) = {
            let st = self.editor.read(cx);
            (st.input_bounds().size.height, st.line_height())
        };
        let Some(line_h) = line_h else {
            return;
        };
        if viewport <= px(0.) {
            return;
        }
        let content = line_h * note.line_count() as f32;
        let max = (content - viewport).max(px(1.));
        let target_y = -(ratio.clamp(0.0, 1.0) * max);
        self.editor.update(cx, |st, cx| {
            st.set_scroll_offset(point(px(0.), target_y), cx);
        });
        // 记录"我刚按预览目标滚到的位置"，供后续回显吸收
        self.last_received_ratio = ratio;
    }

    /// 调度下一帧滚动同步轮询。框架在实体被移除后不再回调，循环自动停止。
    fn schedule_scroll_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, move |this, window, cx| {
            this.tick_scroll_sync(window, cx);
            this.schedule_scroll_tick(window, cx);
        });
    }

    fn on_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value().to_string();
        if let Some(note) = &mut self.note {
            note.set_text(&text);
        }
        self.schedule_save(window, cx);
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
                if auto_save {
                    this.save_now(cx);
                }
                // 保存与预览共用同一个防抖
                this.refresh_doc(cx);
            });
        }));
    }

    /// 在光标处插入文本（供目录树"插入到当前笔记"使用）。
    ///
    /// 插入后走与用户输入相同的同步：更新笔记、防抖保存、重算预览。
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
        match note.save() {
            Ok(()) => self.status = "已保存".into(),
            Err(err) => self.status = format!("保存失败: {err}").into(),
        }
        cx.notify();
    }

    /// Cmd+S 立即保存
    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
    }

    /// 磁盘文件被外部改动：没有未保存修改就自动重载，有则保留用户内容并提示
    pub fn handle_external_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note) = &mut self.note else {
            return;
        };
        if !note.disk_changed() {
            return;
        }

        if note.is_dirty() {
            self.status = "文件已被外部修改，但你本地有未保存的更改".into();
        } else {
            match note.reload() {
                Ok(()) => {
                    let text = note.text();
                    self.editor.update(cx, |state, cx| {
                        state.set_value(text, window, cx);
                    });
                    self.status = "已加载外部修改".into();
                    self.refresh_doc(cx);
                    self.activate_preview(cx);
                }
                Err(err) => self.status = format!("重新加载失败: {err}").into(),
            }
        }
        cx.notify();
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
            return;
        };
        let key = tab_key(&path);
        cx.update_global::<AppState, _>(|state, _| state.tabs.activate(&key));
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

        view = view.child(body).child(
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

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/editor.rs"]
mod tests;
