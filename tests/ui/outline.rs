// 只测不依赖 App 上下文的纯函数：由 `DocFacts` 生成大纲条目。
// 面板的点击与滚动定位按设计文档走人工验收。
use super::{OutlineItem, outline_items};
use crate::core::markdown::parse_facts;

fn items_of(text: &str) -> Vec<OutlineItem> {
    outline_items(&parse_facts(text))
}

#[test]
fn headings_become_items_with_level_text_and_line() {
    let items = items_of("正文\n# 一级\n正文\n## 二级\n");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].level, 1);
    assert_eq!(items[0].text, "一级");
    assert_eq!(items[0].line, 1, "行号 0-based，与编辑区一致");
    assert_eq!(items[1].level, 2);
    assert_eq!(items[1].line, 3);
}

#[test]
fn code_fence_headings_are_excluded() {
    let items = items_of("```\n# 不是标题\n```\n# 是标题\n");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text, "是标题");
}

#[test]
fn document_without_headings_yields_nothing() {
    assert!(items_of("只有正文\n").is_empty());
    assert!(items_of("").is_empty());
}

#[test]
fn levels_stay_within_the_heading_range() {
    let items = items_of("# a\n## b\n### c\n#### d\n##### e\n###### f\n");
    for item in &items {
        assert!((1..=6).contains(&item.level), "层级越界: {item:?}");
    }
    assert_eq!(items.len(), 6);
}
