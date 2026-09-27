//! 图片预览窗口
//!
//! 两个入口都会用到它：点击 Markdown 里嵌入的图片、点击目录树中的图片文件。
//!
//! **为什么用 `img(PathBuf)` 而不是 URL**：gpui 对 `Resource::Path` 会直接
//! `fs::read`（gpui-pre `elements/img.rs:634`），而 `file://` 与裸路径都会被当成
//! `Resource::Uri` 交给 HTTP 客户端，桌面端没有 HTTP 客户端，必然加载不出来。

use std::path::PathBuf;

use gpui_kit::component::Root;
use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

/// 图片预览窗口的内容：顶部一行文件名，中部按容器缩放显示原图
pub struct ImageViewer {
    path: PathBuf,
}

impl ImageViewer {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Render for ImageViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "图片".to_string());

        div()
            .id("image-viewer")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex_none()
                    .p_2()
                    .text_sm()
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
                        img(self.path.clone())
                            // 等比缩放并完整放进窗口，不做裁切
                            .object_fit(ObjectFit::Contain)
                            .max_w(relative(1.))
                            .max_h(relative(1.)),
                    ),
            )
    }
}

/// 打开图片预览窗口；`path` 应是本地存在的图片文件
pub fn open_image_viewer(path: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        match cx.open_window(WindowOptions::default(), |window, cx| {
            let view = cx.new(|_| ImageViewer::new(path.clone()));
            cx.new(|cx| Root::new(view, window, cx))
        }) {
            Ok(_) => {}
            Err(err) => eprintln!("[xink] 打开图片预览失败: {err:?}"),
        }
    })
    .detach();
}
