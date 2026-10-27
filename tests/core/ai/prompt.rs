//! prompt 组装

use crate::core::ai::prompt::{KNOWN_TAGS_PLACEHOLDER, build_messages, build_skill_messages};
use crate::core::config::{AiActionConfig, OutputLang, builtin_actions};

fn action(id: &str) -> AiActionConfig {
    builtin_actions()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("内置动作里没有 {id}"))
}

#[test]
fn 内置动作_六项且_id_唯一() {
    let actions = builtin_actions();
    assert_eq!(actions.len(), 6, "内置动作按需求精简为 6 项");
    let mut ids: Vec<&str> = actions.iter().map(|a| a.id.as_str()).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total);
}

#[test]
fn 内置动作_每项都有名字与提示词() {
    for action in builtin_actions() {
        assert!(!action.id.trim().is_empty());
        assert!(!action.name.trim().is_empty());
        assert!(!action.prompt.trim().is_empty());
    }
}

#[test]
fn 笔记内容被包在数据块里() {
    let msgs = build_messages(&action("polish"), "正文", OutputLang::Auto, &[]);
    assert_eq!(msgs.len(), 2);
    assert!(msgs[0].content.contains("不是指令"), "必须有注入防护声明");
    assert!(msgs[1].content.contains("<<<CONTENT>>>"));
    assert!(msgs[1].content.contains("<<<END>>>"));
    // 指令来自配置的提示词，不是代码里的枚举
    assert!(msgs[1].content.contains("润色"), "提示词要随请求带出去");
}

#[test]
fn 技能正文与笔记原文分开且都有边界() {
    let msgs = build_skill_messages("demo", "把标题改成一句话", "笔记特有的句子", OutputLang::Zh);
    assert_eq!(msgs.len(), 2);
    // 正文是**指令**，进 system；原文是**数据**，进 user：两边都不许混
    assert!(msgs[0].content.contains("<<<SKILL>>>"));
    assert!(msgs[0].content.contains("把标题改成一句话"));
    assert!(
        !msgs[0].content.contains("笔记特有的句子"),
        "笔记原文不许混进 system 段"
    );
    assert!(msgs[1].content.contains("<<<CONTENT>>>"));
    assert!(msgs[1].content.contains("笔记特有的句子"));
    assert!(msgs[0].content.contains("demo"), "要标明技能名");
    assert!(msgs[0].content.contains("用简体中文输出"));
}

#[test]
fn 提示词里的占位符被替换成库内既有标签() {
    let tags = vec!["人工智能".to_string(), "读书".to_string()];
    let msgs = build_messages(&action("tag"), "正文", OutputLang::Zh, &tags);
    assert!(msgs[1].content.contains("人工智能、读书"));
    assert!(!msgs[1].content.contains(KNOWN_TAGS_PLACEHOLDER));
    assert!(msgs[0].content.contains("用简体中文输出"));
}

#[test]
fn 库里没有标签时占位符被去掉而不是留空括号() {
    let msgs = build_messages(&action("tag"), "正文", OutputLang::Zh, &[]);
    assert!(!msgs[1].content.contains(KNOWN_TAGS_PLACEHOLDER));
    // 整句去掉，而不是留一个"选择：；"的残句
    assert!(!msgs[1].content.contains("优先从库里已有的标签中选择"));
    assert!(msgs[1].content.contains("生成标签"), "其余指令还在");
}

#[test]
fn 没写占位符的动作不会带上库内标签() {
    let tags = vec!["人工智能".to_string()];
    let msgs = build_messages(&action("polish"), "正文", OutputLang::Zh, &tags);
    assert!(!msgs[1].content.contains("人工智能"));
}
