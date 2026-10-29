//! 全局视觉规范：间距 / 圆角 / 字号 / 语义色 / 图标
//!
//! 以前这些值散在各处：AI 区自己用 `hsla()` 调气泡底色、圆角写死 12px、
//! 间距 `gap_1` 与 `gap_2` 混用，和大纲 / 搜索 / 反链 / 标签对不上。
//! 现在统一从这里取值，面板之间才像一套东西。
//!
//! 取值全部来自 `cx.theme()`，明暗两套自动跟随，禁止再手写颜色。

use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, Hsla, Pixels, px};

// ── 间距 ──────────────────────────────────────────────
//
// 对齐 base 的 `SpacingTokens`（`gpui-base/src/theme_tokens.rs:132-154`）。

/// 图标与文字之间
pub const SPACE_XS: f32 = 4.;
/// 行内元素之间、分区左右内边距
pub const SPACE_SM: f32 = 8.;
/// 气泡内边距
pub const SPACE_MD: f32 = 12.;

// ── 圆角 ──────────────────────────────────────────────

/// 气泡圆角
///
/// 跟主题走（默认 `radius` 6 → 12，与原来的写死值一致），但兜一个下限：
/// 主题把 `radius` 设成 0 时它是 0，气泡就成了方块（`ai.rs` 的旧注释里
/// 记着这个坑），所以这里给 10px 兜底。
pub fn bubble_radius(cx: &App) -> Pixels {
    let base = cx.theme().radius;
    if base < px(4.) { px(10.) } else { base * 2. }
}

/// 整行可点元素的小圆角（折叠占位、块标题）：直接跟主题
pub fn row_radius(cx: &App) -> Pixels {
    cx.theme().radius
}

// ── 语义色 ────────────────────────────────────────────

/// 悬停底色：全项目统一口径
///
/// 既有 7 处（大纲 / 搜索 / 反链 / 标签 ×2 / 右侧按钮条 / 设置侧栏）都是
/// `accent.opacity(0.12)`，AI 区以前一处都没有，现在共用这一个函数。
pub fn hover_bg(cx: &App) -> Hsla {
    cx.theme().accent.opacity(0.12)
}

/// 用户气泡底色
///
/// 以前是 `hsla()` 手搓的两组 HSL 常量（`ai.rs` 的 `bubble_bg`），
/// 全项目唯一一处不跟主题的颜色。
pub fn user_bubble_bg(cx: &App) -> Hsla {
    bubble_bg_from_accent(cx.theme().accent)
}

/// 从 accent 色算气泡底色：纯函数，便于单测（`user_bubble_bg` 走这里）
pub fn bubble_bg_from_accent(accent: Hsla) -> Hsla {
    accent.opacity(0.12)
}

/// 比 hover 再重一档：气泡自身底色已经很淡，hover 用它才看得出变化
pub fn selected_bg(cx: &App) -> Hsla {
    cx.theme().accent.opacity(0.18)
}

/// AI 气泡底色：以前**没有**底色，只靠左右对齐和用户气泡区分
pub fn ai_bubble_bg(cx: &App) -> Hsla {
    cx.theme().muted
}

// ── AI 对话条尺寸 ─────────────────────────────────────
//
// 集中在这里，免得散在 `ai.rs` 里被当成"随手写的数字"。

/// 对话区最大高度：超过就让对话区自己滚，编辑区不被压扁
///
/// 180 太小：一轮问答就撑满，更早的轮次全在视口外，看起来像"只有一轮"。
pub const AI_HISTORY_MAX_H: f32 = 260.;
/// 单个展开块的最大高度
///
/// 块内自带垂直滚动：工具结果动辄几百行，不收口会把对话区撑很高，
/// 外层那 260px 再怎么滚也看不全。
pub const AI_BODY_MAX_H: f32 = 220.;
/// 气泡最大宽度（占对话区宽度的比例）：太长的一行读起来累
pub const AI_BUBBLE_MAX_W: f32 = 0.78;
/// 输入框起始高度：多行，写长了由它自己长高
pub const INPUT_MIN_H: f32 = 52.;
