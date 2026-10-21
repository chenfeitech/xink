//! 右侧半透明抽屉：承载大纲 / 反链 / 标签。
//!
//! 设计见 `docs/right-rail-drawer-design.md`。三条硬约束（都是踩过的坑）：
//!
//! 1. **内容实体必须建在 `open_sheet_at` 外面**：sheet 的 build 闭包与 dialog
//!    一样每次重绘都会重放（`root.rs:221-223`），在里面 `cx.new` 等于每帧重建。
//! 2. **不要在 `open_sheet_at` 之后再 focus**：它内部已经聚焦过自己的句柄
//!    （`root.rs:394`），再聚焦一次会同帧出现两个"已聚焦"节点，命中框架的
//!    a11y 断言（`a11y.rs:529`）。本抽屉没有输入框，因此完全不碰焦点。
//! 3. **全高要把 `theme.sheet.margin_top` 置 0**：`Placement::Right` 的顶边
//!    取自该值，默认是标题栏高度（`sheet.rs:144`）。置零在 `app::apply_theme`
//!    里统一做，避免每次 `Theme::change` 后被主题配置覆盖。

use gpui_kit::base::Placement;
use gpui_kit::component::{ActiveTheme, WindowExt};
use gpui_kit::*;

use crate::app::AppState;
use crate::ui::{BacklinkPanel, OutlinePanel, TagsPanel};

/// 抽屉宽度：只够放下标题与路径，尽量少遮正文
const DRAWER_WIDTH: f32 = 260.;
/// 背景不透明度。框架没有 `backdrop_blur`，只能做纯色半透明
const DRAWER_OPACITY: f32 = 0.88;

/// 抽屉里能装的三类内容
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DrawerKind {
    Outline,
    Backlink,
    Tags,
}

impl DrawerKind {
    /// 按钮条上的顺序即这里的顺序
    pub const ALL: [Self; 3] = [Self::Outline, Self::Backlink, Self::Tags];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Outline => "大纲",
            Self::Backlink => "反链",
            Self::Tags => "标签",
        }
    }

    /// 元素 id 与图标都走内联 SVG：框架默认图标集（101 个）里没有
    /// list / tag / link，语义对不上，与 `ui/file_tree.rs` 的做法保持一致
    pub fn id(&self) -> &'static str {
        match self {
            Self::Outline => "outline",
            Self::Backlink => "backlink",
            Self::Tags => "tags",
        }
    }

    pub fn icon_svg(&self) -> &'static str {
        match self {
            Self::Outline => ICON_LIST,
            Self::Backlink => ICON_LINK,
            Self::Tags => ICON_TAG,
        }
    }
}

const ICON_LIST: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" > <path d="M3 6h18" /> <path d="M3 12h18" /> <path d="M3 18h18" /> </svg>"#;
const ICON_LINK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" > <path d="M9 17H7A5 5 0 0 1 7 7h2" /> <path d="M15 7h2a5 5 0 1 1 0 10h-2" /> <line x1="8" x2="16" y1="12" y2="12" /> </svg>"#;
const ICON_TAG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" > <path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z" /> <circle cx="7.5" cy="7.5" r=".5" fill="currentColor" /> </svg>"#;

/// 抽屉内容：三种面板类型不同，统一成 `AnyElement` 才能塞进同一个闭包
enum DrawerView {
    Outline(Entity<OutlinePanel>),
    Backlink(Entity<BacklinkPanel>),
    Tags(Entity<TagsPanel>),
}

impl DrawerView {
    fn new(kind: DrawerKind, cx: &mut App) -> Self {
        match kind {
            DrawerKind::Outline => Self::Outline(cx.new(OutlinePanel::new)),
            DrawerKind::Backlink => Self::Backlink(cx.new(BacklinkPanel::new)),
            DrawerKind::Tags => Self::Tags(cx.new(TagsPanel::new)),
        }
    }

    /// 每次重绘都要重新包一层 `AnyElement`：build 闭包是 `Fn`，不能消耗 self。
    /// 面板实体始终是同一个，状态不会丢。
    fn element(&self) -> AnyElement {
        match self {
            Self::Outline(view) => view.clone().into_any_element(),
            Self::Backlink(view) => view.clone().into_any_element(),
            Self::Tags(view) => view.clone().into_any_element(),
        }
    }
}

/// 打开某个抽屉。
///
/// 同一时刻只有一个 active sheet，因此切换内容时框架会自动替换，
/// 不需要先关旧的。
pub fn open(kind: DrawerKind, window: &mut Window, cx: &mut App) {
    // 建在外面：build 闭包每帧重放，在里面建会每帧重建面板
    let view = DrawerView::new(kind, cx);
    let title = kind.label();

    window.open_sheet_at(Placement::Right, cx, move |sheet, _window, cx| {
        let body = view.element();
        sheet
            .size(px(DRAWER_WIDTH))
            .resizable(false)
            .overlay(true)
            .overlay_closable(true)
            .title(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(title),
            )
            // 半透明：`refine_style` 在 `.bg(theme.tokens.background)`
            // 之后生效，这里传带 alpha 的背景色即可覆盖
            .bg(cx.theme().background.opacity(DRAWER_OPACITY))
            // 右上角自带收回按钮（点它 → Cancel → close_sheet）；
            // 这里只是同步按钮条的选中态
            .on_close(|_, _window, cx| clear_opened(cx))
            // 给内容一个确定高度，面板内部的列表才能自己滚动
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
    });
}

/// 收起抽屉
pub fn close(window: &mut Window, cx: &mut App) {
    window.close_sheet(cx);
}

/// 抽屉被关闭后清掉按钮条的选中态。
///
/// 走 `RailState` 实体而不是直接改 global：这里可能处在 sheet 的关闭回调里，
/// 而 `update_global` 期间 global 会被移出表（租借语义），再访问会拿到 None。
fn clear_opened(cx: &mut App) {
    let Some(rail) = cx.try_global::<AppState>().map(|state| state.rail.clone()) else {
        return;
    };
    rail.update(cx, |rail, cx| {
        rail.opened = None;
        cx.notify();
    });
}
