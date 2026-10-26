//! OpenAI 兼容端点的帧解析（通过 `frame_to_events` 的行为覆盖）

use crate::core::config::AiModelParams;

/// 这里的用例走"一次请求"的黑盒路径：构造 endpoint，验证拼接规则。
/// 流式帧解析本身由 `sse.rs` 的分帧 + `openai.rs` 的映射共同完成，
/// 映射逻辑无 IO，单独抽成断言需要把它 pub 出去，暂以 endpoint 规则与
/// 请求体字段的最小约定代替（真实联调见人工验收清单）。
#[test]
fn endpoint_按整体_base_url_拼接() {
    // 智谱不是 /v1，DeepSeek 是 /v1：base_url 必须整体可配
    assert_eq!(
        crate::core::ai::openai::chat_endpoint("https://api.deepseek.com/v1"),
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(
        crate::core::ai::openai::chat_endpoint("https://open.bigmodel.cn/api/paas/v4/"),
        "https://open.bigmodel.cn/api/paas/v4/chat/completions"
    );
}

/// 分片累积：同一个调用的 name 与 arguments 分三帧到达
#[test]
fn 工具调用的参数按_index_跨帧拼接() {
    use crate::core::ai::openai::frame_to_events;
    use crate::core::ai::{StreamEvent, Usage};

    let mut usage = Usage::default();
    let mut events = Vec::new();
    let mut drafts = Vec::new();

    // 第 1 帧只给函数名
    frame_to_events(
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"search_notes","arguments":""}}]}}]}"#,
        &mut usage,
        &mut events,
        &mut drafts,
    );
    assert!(events.is_empty(), "没到 finish_reason 前不该发事件");

    // 第 2、3 帧给参数碎片（真实端点会切成更细的片）
    frame_to_events(
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"query\":"}}]}}]}"#,
        &mut usage,
        &mut events,
        &mut drafts,
    );
    frame_to_events(
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"rust\"}"}}]}}]}"#,
        &mut usage,
        &mut events,
        &mut drafts,
    );

    // 收尾时才把完整的调用发出来
    frame_to_events(
        r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        &mut usage,
        &mut events,
        &mut drafts,
    );
    assert!(matches!(events.first(), Some(StreamEvent::ToolCall(_))));
    assert!(matches!(events.last(), Some(StreamEvent::Done)));
    // ToolCall 必须在 Done 之前，否则上层收尾了才收到调用
    assert!(events.len() >= 2);
}

#[test]
fn 多次工具调用各自独立() {
    use crate::core::ai::openai::frame_to_events;
    use crate::core::ai::{StreamEvent, Usage};

    let mut usage = Usage::default();
    let mut events = Vec::new();
    let mut drafts = Vec::new();

    for (index, name) in [(0, "read_note"), (1, "list_tags")] {
        frame_to_events(
            &format!(
                r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":{index},"function":{{"name":"{name}","arguments":"{{}}"}}}}]}}}}]}}"#
            ),
            &mut usage,
            &mut events,
            &mut drafts,
        );
    }
    frame_to_events(
        r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        &mut usage,
        &mut events,
        &mut drafts,
    );

    let calls: Vec<String> = events
        .iter()
        .filter_map(|ev| match ev {
            StreamEvent::ToolCall(call) => Some(call.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec!["read_note".to_string(), "list_tags".to_string()]
    );
}

#[test]
fn 默认生成参数给足预算() {
    let params = AiModelParams::default();
    // thinking 模型会把 num_predict 用在思维链上，太小会一个正文字都不出
    assert!(params.num_predict >= 1024);
    assert!(!params.think, "默认关闭思考");
}
