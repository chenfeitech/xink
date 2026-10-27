//! 预览面板：两处纯换算
//!
//! 滚动同步的主体（`tick_scroll_sync` 等）依赖 `Context<Self>` 与 `ListState`，
//! 测不了；这里只固定两处纯值映射：锚点降级用的行→比例，以及
//! 领域样式 → gpui 样式（验收清单里"预览区粗体/斜体是否生效"靠人眼，
//! 但"要不要加粗/斜体"这个判断本身可以被测住）。

use crate::core::highlight::SpanStyle;
use crate::core::sync::AnchorTable;

use super::{ratio_of_line, to_highlight_style};

/// 造一份锚点表；`total_lines` 直接给，不要求与源文本行数一致
fn anchor_table(total_lines: usize) -> AnchorTable {
    AnchorTable::from_source("# 标题\n\n正文\n", total_lines)
}

#[test]
fn 首行是零末行是一() {
    let table = anchor_table(10);
    assert_eq!(ratio_of_line(0., &table), 0.);
    assert_eq!(ratio_of_line(9., &table), 1.);
    assert_eq!(ratio_of_line(4.5, &table), 0.5);
}

#[test]
fn 越界的行号被夹到零一之间() {
    let table = anchor_table(10);
    assert_eq!(ratio_of_line(100., &table), 1., "超过末行不能算出 >1");
    assert_eq!(ratio_of_line(-3., &table), 0., "负数不能算出 <0");
}

#[test]
fn 空文档不会算出除零() {
    // `total_lines()` 兜底成 1，这里的 `max(1)` 再兜一次：都不能是 NaN
    let table = anchor_table(0);
    let ratio = ratio_of_line(1., &table);
    assert!(ratio.is_finite(), "空文档算出了 {ratio}");
    assert_eq!(ratio, 1.);

    let single = anchor_table(1);
    assert_eq!(ratio_of_line(1., &single), 1.);
}

#[test]
fn 领域样式的三个开关各自独立生效() {
    let plain = to_highlight_style(SpanStyle {
        color: None,
        bold: false,
        italic: false,
        underline: false,
    });
    assert!(plain.font_weight.is_none());
    assert!(plain.font_style.is_none());
    assert!(plain.underline.is_none());

    let all = to_highlight_style(SpanStyle {
        color: None,
        bold: true,
        italic: true,
        underline: true,
    });
    assert!(all.font_weight.is_some(), "粗体没接上");
    assert!(all.font_style.is_some(), "斜体没接上");
    assert!(all.underline.is_some(), "下划线没接上");
}

#[test]
fn 颜色原样透传不自己造色() {
    // 预览区的配色由主题决定：这里只负责把算好的颜色传过去
    let none = to_highlight_style(SpanStyle {
        color: None,
        bold: false,
        italic: false,
        underline: false,
    });
    assert!(none.color.is_none());

    let colored = to_highlight_style(SpanStyle {
        color: Some(gpui_kit::hsla(0.1, 0.2, 0.3, 1.)),
        bold: false,
        italic: false,
        underline: false,
    });
    assert!(colored.color.is_some());
}
