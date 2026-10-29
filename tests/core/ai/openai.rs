//! OpenAI 兼容端点的帧解析（通过 `frame_to_events` 的行为覆盖）

use std::time::Duration;

use crate::core::ai::{ChatMessage, CompletionRequest};
use crate::core::config::{AiModelParams, ResolvedParams};

use super::build_body;

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

// ── 生成预算耗尽（`finish_reason`）──────────────────────────
//
// 与 Ollama 的 `done_reason` 同一件事：预算耗尽（`"length"`）和正常收尾
// （`"stop"`）都是"收到字符串"，只判断"有没有值"会把两者混为一谈。

#[test]
fn finish_reason_为_length_时先发截断再发完成() {
    use crate::core::ai::openai::frame_to_events;
    use crate::core::ai::{StreamEvent, Usage};

    let mut usage = Usage::default();
    let mut events = Vec::new();
    let mut drafts = Vec::new();
    frame_to_events(
        r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#,
        &mut usage,
        &mut events,
        &mut drafts,
    );
    assert_eq!(
        events,
        vec![StreamEvent::Truncated, StreamEvent::Done],
        "截断必须在 Done 之前"
    );
}

#[test]
fn finish_reason_为_stop_或_null_时不发截断() {
    use crate::core::ai::openai::frame_to_events;
    use crate::core::ai::{StreamEvent, Usage};

    for payload in [
        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        // 非末帧的 finish_reason 恒为 null：`.as_str()` 取不到，不该有任何事件
        r#"{"choices":[{"delta":{"content":"你"},"finish_reason":null}]}"#,
    ] {
        let mut usage = Usage::default();
        let mut events = Vec::new();
        let mut drafts = Vec::new();
        frame_to_events(payload, &mut usage, &mut events, &mut drafts);
        assert!(
            !events.iter().any(|ev| matches!(ev, StreamEvent::Truncated)),
            "{payload} 不该判为截断：{events:?}"
        );
    }
}

// ── 请求体 ────────────────────────────────────────────────────
//
// 请求体是与远端之间的契约：字段写错不会在本地报错，只会表现为"对端 400"
// 或"工具永远不被调用"。改坏了只能连真模型才发现，所以在这里钉住。
// `ChatMessage::system/user` 见 `core/ai/mod.rs`；`role_name()` 是它写进
// 请求体的角色字符串。

fn request(tools: Vec<serde_json::Value>) -> CompletionRequest {
    CompletionRequest {
        endpoint: crate::core::ai::openai::chat_endpoint("https://api.example.com/v1"),
        model: "gpt-x".into(),
        messages: vec![ChatMessage::system("你是助手"), ChatMessage::user("你好")],
        params: ResolvedParams {
            temperature: 0.5,
            num_predict: 512,
            ..ResolvedParams::default()
        },
        api_key: Some("sk-test".into()),
        first_byte_timeout: Duration::from_secs(30),
        silence_timeout: Duration::from_secs(60),
        tools,
    }
}

#[test]
fn 请求体不带工具时完全没有_tools_字段() {
    let body = build_body(&request(Vec::new()));
    // 空数组也要整个字段消失：有小模型见到空 tools 就开始瞎编调用
    assert!(body.get("tools").is_none(), "不该出现 tools：{body}");
}

#[test]
fn 请求体带工具时原样透传() {
    // `tools` 是"每个元素一个工具定义"，与 `tools::specs()` 的形状一致
    let tool = serde_json::json!({
        "type": "function",
        "function": { "name": "read_note" },
    });
    let body = build_body(&request(vec![tool.clone()]));
    assert_eq!(body["tools"], serde_json::json!([tool]));
}

#[test]
fn 请求体始终流式() {
    // 全链路依赖流式：非流式会等到超时才出字
    assert_eq!(build_body(&request(Vec::new()))["stream"], true);
}

#[test]
fn 请求体保持消息顺序与角色() {
    let body = build_body(&request(Vec::new()));
    let messages = body["messages"].as_array().expect("messages 是数组");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "你是助手");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "你好");
}

#[test]
fn 生成参数映射到对端字段名() {
    let body = build_body(&request(Vec::new()));
    // 兼容端点叫 max_tokens，不叫 num_predict
    assert_eq!(body["max_tokens"], 512);
    assert_eq!(body["temperature"], 0.5);
    assert_eq!(body["model"], "gpt-x");
}

#[test]
fn 密钥只走请求头不进请求体() {
    let body = build_body(&request(Vec::new()));
    let text = body.to_string();
    assert!(!text.contains("sk-test"), "密钥泄露到请求体：{text}");
    assert!(body.get("api_key").is_none());
    assert!(body.get("authorization").is_none());
}
