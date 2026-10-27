//! 最右侧的按钮条：大纲 / 反链 / 标签，上下垂直排列。
//!
//! 设计见 `docs/right-rail-drawer-design.md`。两个要点：
//!
//! 1. **是浮层，不占布局宽度**：绝对定位贴在窗口最右侧，内容区保持全宽；
//!    代价是会盖住预览右边缘一点。
//! 2. **渲染顺序在 sheet 层之后、dialog 层之前**：抽屉展开时按钮条仍可点
//!    （用来切换/收起），而命令面板等模态打开时会被遮住（模态该有的语义）。
//!    顺序由 `Workspace::render` 里 children 的先后决定。

use gpui_kit::component::{ActiveTheme, Icon, Sizable, Size};
use gpui_kit::*;

use crate::ui::drawer::{self, DrawerKind};

/// 按钮条宽度
const RAIL_WIDTH: f32 = 44.;
/// 距窗口顶部：让开标题栏
const RAIL_TOP: f32 = 48.;
/// 距窗口右边缘
const RAIL_RIGHT: f32 = 6.;

/// 按钮条状态：当前展开的是哪个抽屉
pub struct RailState {
    pub opened: Option<DrawerKind>,
}

impl RailState {
    pub fn new() -> Self {
        Self { opened: None }
    }

    /// 点击某个入口：同项再点即收起，异项直接替换
    fn toggle(&mut self, kind: DrawerKind, window: &mut Window, cx: &mut Context<Self>) {
        let next = next_opened(self.opened, kind);
        if next.is_none() {
            drawer::close(window, cx);
        } else {
            drawer::open(kind, window, cx);
        }
        self.opened = next;
        cx.notify();
    }
}

/// 点击某个入口后该展开哪个抽屉（纯状态迁移，抽出来好单测）
///
/// 三条：没开 → 开它；开的正是它 → 关掉；开的是别的 → 直接换。
/// 不存在"两个抽屉同时展开"，因此用 `Option` 而不是集合。
fn next_opened(current: Option<DrawerKind>, kind: DrawerKind) -> Option<DrawerKind> {
    if current == Some(kind) {
        None
    } else {
        Some(kind)
    }
}

impl Render for RailState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let opened = self.opened;
        let entity = cx.entity();

        let mut rail = div()
            .id("right-rail")
            .absolute()
            .top(px(RAIL_TOP))
            .right(px(RAIL_RIGHT))
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .p_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background.opacity(0.85))
            .shadow_lg();

        for kind in DrawerKind::ALL {
            rail = rail.child(rail_button(kind, opened == Some(kind), &entity, cx));
        }
        rail
    }
}

/// 单个入口：图标在上、文字在下
fn rail_button(
    kind: DrawerKind,
    selected: bool,
    entity: &Entity<RailState>,
    cx: &App,
) -> impl IntoElement {
    let entity = entity.clone();
    let fg = if selected {
        cx.theme().accent_foreground
    } else {
        cx.theme().muted_foreground
    };

    let mut button = div()
        .id(format!("rail-btn-{}", kind.id()))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .w(px(RAIL_WIDTH - 8.))
        .py_1()
        .gap_0p5()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .text_color(fg)
        .hover(|this| this.bg(cx.theme().accent.opacity(0.15)));
    // 选中态：框架没有给 Stateful 提供 `when`，这里用普通分支
    if selected {
        button = button.bg(cx.theme().accent);
    }

    button
        .child(
            Icon::default()
                .data(kind.icon_svg().as_bytes())
                .with_size(Size::Size(px(18.))),
        )
        .child(div().text_xs().child(kind.label()))
        .on_click(move |_, window, cx| {
            entity.update(cx, |state, cx| state.toggle(kind, window, cx));
        })
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/ui/rail.rs"]
mod tests;
