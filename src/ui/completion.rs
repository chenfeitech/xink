//! 双链补全：输入 `[[` 后弹出候选，选中即插入 `[[目标]]`。
//!
//! 为什么自建而不是接框架的补全：`gpui-base` 有 `CompletionProvider` 与
//! `Lsp::completion_provider` 字段，但**没有任何触发链路**（全仓仅两处声明），
//! 接上去也不会被调用。详见 `docs/wikilink-design.md` §2。
//!
//! 这里复用命令面板（T7）的范式：`Root::open_dialog` + `Command`，
//! 键盘导航与 Esc 由组件提供，排序交给 `core::fuzzy`。

use std::cell::Cell;
use std::path::Path;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::WindowExt;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::*;

use crate::app::AppState;
use crate::ui::EditorPanel;

/// 最多展示的候选数
const MAX_CANDIDATES: usize = 30;

/// 双链补全浮层
pub struct WikilinkCompletion {
    state: Entity<CommandState>,
    /// 被补全的编辑器：确认后回写文本并收回焦点
    editor: Entity<EditorPanel>,
    /// `[[` 的字节偏移，替换区间的起点
    start: usize,
    /// 与列表同序的候选：(显示文本, 写入笔记的目标)
    matches: Vec<(String, String)>,
}

impl WikilinkCompletion {
    fn new(
        editor: Entity<EditorPanel>,
        start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.new(|cx| CommandState::new(window, cx));
        // 兜底复位：点遮罩关闭不会走 `on_cancel`，编辑区的 `completion_open`
        // 会永远卡在 true，之后就再也弹不出来。实体一释放就复位。
        cx.on_release(|this: &mut Self, cx| {
            this.editor
                .update(cx, |editor, _| editor.set_completion_open(false));
        })
        .detach();

        let mut this = Self {
            state,
            editor,
            start,
            matches: Vec::new(),
        };
        this.refresh("", cx);
        this
    }

    fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    /// 按查询重算候选：标题与文件名都参与匹配
    fn refresh(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(index) = cx.try_global::<AppState>().map(|state| state.notes.clone()) else {
            self.matches.clear();
            return;
        };
        let index = index.read(cx);

        let mut scored: Vec<(u32, String, String)> = index
            .notes()
            .map(|note| {
                let target = link_target(index.root(), &note.path, &note.title);
                let score = crate::core::fuzzy::fuzzy_score(query, &note.title)
                    .or_else(|| crate::core::fuzzy::fuzzy_score(query, &target))
                    .unwrap_or_default();
                (score, note.title.clone(), target)
            })
            .filter(|(score, _, _)| *score > 0 || query.trim().is_empty())
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.truncate(MAX_CANDIDATES);

        self.matches = scored
            .into_iter()
            .map(|(_, title, target)| (title, target))
            .collect();
    }

    /// 写入 `[[目标]]` 并把焦点还给编辑区
    fn confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, target)) = self.matches.get(row).cloned() else {
            return;
        };
        let start = self.start;
        self.editor.update(cx, |editor, cx| {
            editor.apply_wikilink(start, &target, window, cx);
            editor.set_completion_open(false);
        });
        close(window, cx);
    }
}

impl Render for WikilinkCompletion {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let labels: Vec<String> = self
            .matches
            .iter()
            .map(|(title, _)| title.clone())
            .collect();

        Command::new(&self.state)
            .filterable(false)
            .bordered(false)
            .placeholder("选择要链接的笔记…")
            .max_h(px(280.))
            .items(
                labels
                    .into_iter()
                    .map(|label| CommandItem::new().label(label)),
            )
            .empty(|_, _, cx| {
                div()
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("没有匹配的笔记")
            })
            .on_query({
                let entity = entity.clone();
                move |query: &str, _, cx| {
                    let query = query.to_string();
                    entity.update(cx, |this, cx| this.refresh(&query, cx));
                }
            })
            .on_confirm(move |path, window, cx| {
                let row = path.row;
                entity.update(cx, |this, cx| this.confirm(row, window, cx));
            })
            .on_cancel({
                let editor = self.editor.clone();
                move |window, cx| {
                    // 放弃补全：复位标记（否则之后再也弹不出来），
                    // 编辑区里留下的 `[[` 由用户自己处理
                    editor.update(cx, |editor, _| editor.set_completion_open(false));
                    close(window, cx);
                }
            })
    }
}

/// 打开双链补全浮层
pub fn open_wikilink_completion(
    editor: Entity<EditorPanel>,
    start: usize,
    window: &mut Window,
    cx: &mut App,
) {
    // 浮层实体只建一次：dialog 的 build 闭包每次重绘都会重放，
    // 在里面 `cx.new` 会让候选列表与查询在每次重绘后被重置，既选不中也打不了字。
    let view = cx.new(|cx| WikilinkCompletion::new(editor, start, window, cx));
    // 与命令面板同理：只在本次打开的首次 build 里给焦点
    let initial_focus = Cell::new(true);

    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            view.update(cx, |palette, cx| palette.focus(window, cx));
        }
        let view = view.clone();
        dialog
            .w(px(480.))
            .margin_top(px(80.))
            .close_button(false)
            .overlay(true)
            .overlay_closable(true)
            .content(move |content, _, _| content.child(view.clone()))
    });
}

/// 关闭浮层
fn close(window: &mut Window, cx: &mut App) {
    window.close_dialog(cx);
}

/// 写入双链时用的目标文本：文件名能唯一定位就用文件名，否则退回相对路径
fn link_target(root: Option<&Path>, path: &Path, title: &str) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| title.to_string());
    let Some(root) = root else {
        return stem;
    };
    path.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().to_string())
        .unwrap_or(stem)
}
