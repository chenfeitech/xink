//! xink — 本地优先 Markdown 笔记桌面应用

mod actions;
mod app;
mod core;
mod state;
mod ui;

use crate::core::config::AppConfig;
use actions::key_bindings;
use app::Workspace;
use gpui_kit::component::Root;
use gpui_kit::component::dock::{panel_handle, register_panel};
use gpui_kit::*;
use ui::{BacklinkPanel, EditorPanel, FilesPanel, OutlinePanel, PreviewPanel, TagsPanel};

/// 注册各类 Panel 的重建工厂。
///
/// DockArea 从布局文件恢复时，靠这里按 panel_name 重建面板；
/// 未注册的面板无法恢复，会得到空白面板。新增面板时务必在此登记。
fn register_panels(cx: &mut App) {
    register_panel(cx, "FilesPanel", |_, _, cx| {
        panel_handle(cx.new(FilesPanel::new))
    });
    register_panel(cx, "EditorPanel", |_, window, cx| {
        panel_handle(cx.new(|cx| EditorPanel::new(window, cx)))
    });
    register_panel(cx, "TagsPanel", |_, _, cx| {
        panel_handle(cx.new(TagsPanel::new))
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

            // 键位来自 `core/keymap.rs` 的表 + `editor.keymap` 的覆盖；
            // 没配过 keymap 时结果与之前硬编码的表逐条一致（有单测锁定）。
            // 绑定只在启动时下发一次，改键位要重启（L3，见设置项说明）。
            let cfg = AppConfig::load();
            cx.bind_keys(key_bindings(&cfg.editor.keymap));

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
