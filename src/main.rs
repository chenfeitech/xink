//! xink — 本地优先 Markdown 笔记桌面应用

mod actions;
mod app;
mod core;
mod state;
mod ui;

use actions::{
    FormatBold, FormatInlineCode, FormatItalic, FormatLink, FormatStrikethrough, NewNote,
    OpenCommandPalette, OpenFolder, OpenSettings, RenameItem, Save, ToggleTheme,
};
use app::Workspace;
use gpui_kit::component::Root;
use gpui_kit::component::dock::{ClosePanel, panel_handle, register_panel};
use gpui_kit::*;
use ui::{BacklinkPanel, EditorPanel, FilesPanel, OutlinePanel, PreviewPanel};

/// 注册各类 Panel 的重建工厂。
///
/// DockArea 从布局文件恢复时，靠这里按 panel_name 重建面板；
/// 未注册的面板无法恢复，会得到空白面板。新增面板时务必在此登记。
fn register_panels(cx: &mut App) {
    register_panel(cx, "FilesPanel", |_, _, cx| {
        panel_handle(cx.new(|cx| FilesPanel::new(cx)))
    });
    register_panel(cx, "EditorPanel", |_, window, cx| {
        panel_handle(cx.new(|cx| EditorPanel::new(window, cx)))
    });
    register_panel(cx, "BacklinkPanel", |_, _, cx| {
        panel_handle(cx.new(BacklinkPanel::new))
    });
    register_panel(cx, "OutlinePanel", |_, _, cx| {
        panel_handle(cx.new(OutlinePanel::new))
    });
    register_panel(cx, "PreviewPanel", |_, window, cx| {
        panel_handle(cx.new(|cx| PreviewPanel::new(window, cx)))
    });
}

fn main() {
    // 重复实例会成倍消耗 CPU（各自一个渲染窗口与监听线程），这里直接挡掉
    if !state::single_instance::try_acquire() {
        eprintln!("[xink] 已有实例在运行，本次启动退出。");
        return;
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            // 必须在使用任何组件之前调用
            gpui_kit::init(cx);
            register_panels(cx);

            cx.bind_keys([
                KeyBinding::new("cmd-o", OpenFolder, None),
                KeyBinding::new("cmd-n", NewNote, None),
                KeyBinding::new("cmd-s", Save, None),
                // 关闭当前笔记标签
                KeyBinding::new("ctrl-w", ClosePanel, None),
                // 在文件树中重命名选中项（仅当焦点位于文件面板时生效）
                KeyBinding::new("f2", RenameItem, Some("FilesPanel")),
                // 打开设置（与 Obsidian 一致）
                KeyBinding::new("cmd-,", OpenSettings, None),
                // Markdown 行内格式（与 Obsidian 一致）；键位来源见
                // `ui::editor::Format::action`（工具栏提示复用同一批 Action）
                KeyBinding::new("cmd-b", FormatBold, None),
                KeyBinding::new("cmd-i", FormatItalic, None),
                KeyBinding::new("cmd-shift-x", FormatStrikethrough, None),
                KeyBinding::new("cmd-e", FormatInlineCode, None),
                KeyBinding::new("cmd-k", FormatLink, None),
                // 切换明暗主题
                KeyBinding::new("cmd-shift-l", ToggleTheme, None),
                // 命令面板（与 Obsidian 一致）
                KeyBinding::new("cmd-p", OpenCommandPalette, None),
            ]);

            cx.spawn(async move |cx| {
                let window = cx
                    .open_window(WindowOptions::default(), |window, cx| {
                        let view = cx.new(|cx| Workspace::new(window, cx));
                        // 窗口第一层必须是 Root
                        cx.new(|cx| Root::new(view, window, cx))
                    })
                    .expect("failed to open window");

                _ = window.update(cx, |_, window, _cx| {
                    window.set_window_title("xink");
                });
            })
            .detach();
        });
}
