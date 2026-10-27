//! 右侧抽屉：三类抽屉的元数据表
//!
//! 与 `tests/ui/settings.rs` 同一思路——查表类的静态数据最容易悄悄写错
//! （id 撞车、SVG 漏了闭合标签），而这些错在渲染时只会表现为"图标不显示"。

use std::collections::HashSet;

use super::DrawerKind;

#[test]
fn 按钮条顺序是固定的三类() {
    // 顺序即按钮条上的顺序，改了要同步 `ui/rail.rs`
    assert_eq!(
        DrawerKind::ALL,
        [DrawerKind::Outline, DrawerKind::Backlink, DrawerKind::Tags]
    );
    assert_eq!(DrawerKind::ALL.len(), 3);
}

#[test]
fn 每类都有非空且不重复的标签() {
    let labels: Vec<&str> = DrawerKind::ALL.iter().map(|kind| kind.label()).collect();
    for label in &labels {
        assert!(!label.is_empty());
    }
    let unique: HashSet<&str> = labels.into_iter().collect();
    assert_eq!(unique.len(), 3, "标签不能重名");
}

#[test]
fn 元素_id唯一且能直接拼进按钮_id() {
    let ids: Vec<&str> = DrawerKind::ALL.iter().map(|kind| kind.id()).collect();
    for id in &ids {
        assert!(!id.is_empty());
        // id 里不能有空格：它会拼成 `rail-btn-{id}`
        assert!(!id.contains(' '), "{id} 不该有空格");
    }
    let unique: HashSet<&str> = ids.into_iter().collect();
    assert_eq!(unique.len(), 3, "id 撞车会让两个按钮共用同一个元素 id");
}

#[test]
fn 图标都是完整的内联_svg() {
    let mut seen = HashSet::new();
    for kind in DrawerKind::ALL {
        let svg = kind.icon_svg();
        assert!(svg.starts_with("<svg"), "{} 的图标不是 SVG", kind.label());
        assert!(
            svg.trim_end().ends_with("</svg>"),
            "{} 的 SVG 没闭合",
            kind.label()
        );
        assert!(seen.insert(svg), "{} 的图标和别人重复", kind.label());
    }
}
