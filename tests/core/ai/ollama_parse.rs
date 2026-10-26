//! Ollama 帧解析（不需要本机 Ollama，常跑）
//!
//! 与 `ollama.rs`（端到端、默认 ignore）分开：这里只验证 JSON → 事件的映射。

use super::frame_to_events;
use crate::core::ai::{StreamEvent, Usage};

fn events_of(line: &str) -> Vec<StreamEvent> {
    let mut usage = Usage::default();
    let mut out = Vec::new();
    frame_to_events(line, &mut usage, &mut out);
    out
}

#[test]
fn 工具调用的参数是对象时转成_json_文本() {
    // Ollama 的 `arguments` 是对象，OpenAI 兼容是字符串：这里统一成文本
    let events = events_of(
        r#"{"message":{"tool_calls":[{"function":{"name":"search_notes","arguments":{"query":"rust"}}}]}}"#,
    );
    match &events[0] {
        StreamEvent::ToolCall(call) => {
            assert_eq!(call.name, "search_notes");
            assert!(call.arguments.contains("rust"), "{:?}", call.arguments);
            assert!(call.arguments.starts_with('{'), "{:?}", call.arguments);
        }
        other => panic!("应是 ToolCall：{other:?}"),
    }
}

#[test]
fn 工具调用的参数是字符串时原样透传() {
    let events = events_of(
        r#"{"message":{"tool_calls":[{"function":{"name":"list_tags","arguments":"{}"}}]}}"#,
    );
    match &events[0] {
        StreamEvent::ToolCall(call) => assert_eq!(call.arguments, "{}"),
        other => panic!("应是 ToolCall：{other:?}"),
    }
}

#[test]
fn 没有参数字段时给空串() {
    let events = events_of(r#"{"message":{"tool_calls":[{"function":{"name":"list_notes"}}]}}"#);
    match &events[0] {
        StreamEvent::ToolCall(call) => assert_eq!(call.arguments, ""),
        other => panic!("应是 ToolCall：{other:?}"),
    }
}

#[test]
fn 一帧里可以有多个工具调用() {
    let events = events_of(
        r#"{"message":{"tool_calls":[{"function":{"name":"list_tags","arguments":{}}},{"function":{"name":"outline","arguments":{"path":"a.md"}}}]}}"#,
    );
    let names: Vec<String> = events
        .iter()
        .filter_map(|ev| match ev {
            StreamEvent::ToolCall(call) => Some(call.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["list_tags".to_string(), "outline".to_string()]);
}

#[test]
fn 没有工具调用时不产生事件() {
    assert!(
        events_of(r#"{"message":{"content":"你好"},"done":false}"#)
            .iter()
            .all(|ev| !matches!(ev, StreamEvent::ToolCall(_)))
    );
}
