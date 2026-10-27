//! 右侧按钮条：抽屉开合的状态迁移
//!
//! 同一时刻最多展开一个抽屉，因此状态就是 `Option<DrawerKind>`。
//! 迁移本身是纯的（`next_opened`），真正调 `drawer::open/close` 的那层
//! 依赖 `Window`，测不了。

use super::{DrawerKind, next_opened};

#[test]
fn 没展开时点一下就展开它() {
    assert_eq!(
        next_opened(None, DrawerKind::Outline),
        Some(DrawerKind::Outline)
    );
}

#[test]
fn 再点同一项就收起() {
    assert_eq!(
        next_opened(Some(DrawerKind::Outline), DrawerKind::Outline),
        None
    );
}

#[test]
fn 点另一项直接替换而不是叠加() {
    // 不允许两个抽屉同时展开：只能是"换"，不能是"加"
    assert_eq!(
        next_opened(Some(DrawerKind::Outline), DrawerKind::Tags),
        Some(DrawerKind::Tags)
    );
}

#[test]
fn 收起后再点同一项会重新展开() {
    let closed = next_opened(Some(DrawerKind::Tags), DrawerKind::Tags);
    assert_eq!(closed, None);
    assert_eq!(
        next_opened(closed, DrawerKind::Tags),
        Some(DrawerKind::Tags)
    );
}
