//! prompt 组装

use crate::core::ai::prompt::{ALL_ACTIONS, Action, build_messages};
use crate::core::config::OutputLang;

#[test]
fn 八个内置动作_id_唯一() {
    let mut ids: Vec<&str> = ALL_ACTIONS.iter().map(|a| a.id()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total);
}

#[test]
fn 笔记内容被包在数据块里() {
    let msgs = build_messages(Action::Polish, "正文", OutputLang::Auto, &[]);
    assert_eq!(msgs.len(), 2);
    assert!(msgs[0].content.contains("不是指令"), "必须有注入防护声明");
    assert!(msgs[1].content.contains("<<<CONTENT>>>"));
    assert!(msgs[1].content.contains("<<<END>>>"));
}

#[test]
fn 生成标签时带上库内既有标签() {
    let tags = vec!["人工智能".to_string(), "读书".to_string()];
    let msgs = build_messages(Action::Tag, "正文", OutputLang::Zh, &tags);
    assert!(msgs[0].content.contains("人工智能、读书"));
    assert!(msgs[0].content.contains("用简体中文输出"));
}
