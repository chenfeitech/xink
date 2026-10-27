//! 库管理窗口：按库数量算窗口高度
//!
//! 窗口高度是纯算术（`layout::window_height`），但它是"框架没有测量 API"这个
//! 约束下的手算结果，写错了只会表现为窗口被裁或留一大块空白，因此固定住。
//! 库的增删改本身在 `core/vault.rs`（已有 `tests/core/vault.rs` 覆盖）。

use super::layout;

#[test]
fn 没有库时走空态高度并被夹到下限() {
    // HEADER 44 + EMPTY 64 + FOOTER 52 = 160 < MIN_H 220
    assert_eq!(layout::window_height(0), layout::MIN_H);
}

#[test]
fn 库很少时高度不低于下限() {
    // 1 个库：HEADER 44 + ROW 96 + FOOTER 52 = 192，仍然小于下限 220
    assert_eq!(layout::window_height(1), layout::MIN_H);
    // 2 个库刚好越过下限，能看出下限确实在起作用
    assert!(layout::window_height(2) > layout::MIN_H);
}

#[test]
fn 中等数量按行高线性增长() {
    let two = layout::window_height(2);
    let three = layout::window_height(3);
    assert_eq!(two, 44. + 2. * 96. + 52.);
    assert_eq!(three - two, layout::ROW_H, "多一个库就高一个行高");
}

#[test]
fn 库很多时被夹到上限() {
    // 100 个库按行高算远超上限，必须被夹住，否则窗口会撑出屏幕
    assert_eq!(layout::window_height(100), layout::MAX_H);
    // 7 个库就已经顶到上限了（44 + 7×96 + 52 = 768 > 720）
    assert_eq!(layout::window_height(7), layout::MAX_H);
}

#[test]
fn 高度随库数量单调不减() {
    let mut last = 0.;
    for rows in 0..20 {
        let height = layout::window_height(rows);
        assert!(height >= last, "{rows} 个库的高度反而变小了");
        last = height;
    }
}
