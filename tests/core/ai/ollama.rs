//! 对真实 Ollama 的端到端验证
//!
//! 默认 `#[ignore]`：CI 不要求装 Ollama。手动跑：
//! `cargo test -- --ignored core::ai::ollama`
//! 前提：本机 `http://localhost:11434` 有 Ollama 且至少拉了一个模型。

use crate::core::ai::ollama::{OllamaProvider, build_body, chat_endpoint, list_models, probe};
use crate::core::ai::{ChatMessage, CompletionRequest, Provider, StreamEvent};
use crate::core::config::ResolvedParams;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const BASE: &str = "http://localhost:11434";

/// 取本机第一个模型；没有就跳过（不算失败）
fn first_model() -> Option<String> {
    list_models(BASE, Duration::from_secs(5))
        .ok()?
        .into_iter()
        .map(|m| m.name)
        .next()
}

#[test]
#[ignore]
fn 能拉取模型列表() {
    let models = list_models(BASE, Duration::from_secs(5)).expect("拉取 /api/tags 失败");
    assert!(!models.is_empty(), "本机没有模型：先 ollama pull");
    println!("{models:#?}");
}

#[test]
#[ignore]
fn 流式能拿到正文() {
    let Some(model) = first_model() else { return };
    let req = CompletionRequest {
        endpoint: chat_endpoint(BASE),
        model,
        messages: vec![ChatMessage::user("用一句话说明什么是本地优先软件")],
        params: ResolvedParams {
            think: false,
            num_predict: 64,
            ..Default::default()
        },
        api_key: None,
        first_byte_timeout: Duration::from_secs(30),
        silence_timeout: Duration::from_secs(40),
        tools: Vec::new(),
    };
    let provider = OllamaProvider;
    let mut text = String::new();
    let mut done = false;
    provider
        .stream(&req, Arc::new(AtomicBool::new(false)), &mut |ev| match ev {
            StreamEvent::Delta(s) => text.push_str(&s),
            StreamEvent::Done => done = true,
            StreamEvent::Failed(e) => panic!("{e}"),
            _ => {}
        })
        .expect("stream 失败");
    assert!(done, "没有收到结束事件");
    assert!(!text.trim().is_empty(), "正文为空：think 是否没关掉？");
    println!("正文：{text}");
}

#[test]
#[ignore]
fn 未知模型报_model_not_found() {
    let result = probe(BASE, "no-such-model:0b", Duration::from_secs(10), false);
    assert!(!result.ok);
    println!("{result:?}");
}

// ── 请求体（不联网的部分） ────────────────────────────────────
//
// 与 `tests/core/ai/openai.rs` 对称：Ollama native 的字段名是
// `options.num_ctx` / `num_predict` / `keep_alive` / `think`，
// 与兼容端点完全不同，写错不会在本地报错。

fn request(tools: Vec<serde_json::Value>, keep_alive: &str) -> CompletionRequest {
    CompletionRequest {
        endpoint: chat_endpoint("http://localhost:11434"),
        model: "qwen3.5:0.8b".into(),
        messages: vec![ChatMessage::user("你好")],
        params: ResolvedParams {
            num_ctx: 4096,
            num_predict: 1024,
            temperature: 0.3,
            think: false,
            keep_alive: keep_alive.into(),
        },
        api_key: None,
        first_byte_timeout: Duration::from_secs(30),
        silence_timeout: Duration::from_secs(60),
        tools,
    }
}

#[test]
fn 请求体不带工具时完全没有_tools_字段() {
    let body = build_body(&request(Vec::new(), "5m"));
    assert!(body.get("tools").is_none(), "不该出现 tools：{body}");
}

#[test]
fn 请求体带工具时原样透传() {
    let tool = serde_json::json!({
        "type": "function",
        "function": { "name": "read_note" },
    });
    let body = build_body(&request(vec![tool.clone()], "5m"));
    assert_eq!(body["tools"], serde_json::json!([tool]));
}

#[test]
fn 生成参数落在_options_里() {
    let body = build_body(&request(Vec::new(), "5m"));
    assert_eq!(body["options"]["num_ctx"], 4096);
    assert_eq!(body["options"]["num_predict"], 1024);
    // 配置里是 f32，JSON 里是 f64：转回来比，否则 0.3 永远不相等
    assert_eq!(body["options"]["temperature"].as_f64().unwrap() as f32, 0.3);
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "qwen3.5:0.8b");
}

#[test]
fn 常驻时长为空时不写_keep_alive() {
    // 空串会被 Ollama 当成"立刻卸载"，不如干脆不传让它用服务端默认
    let body = build_body(&request(Vec::new(), ""));
    assert!(body.get("keep_alive").is_none(), "{body}");

    let blank = build_body(&request(Vec::new(), "   "));
    assert!(blank.get("keep_alive").is_none(), "纯空格等同空：{blank}");

    assert_eq!(build_body(&request(Vec::new(), "5m"))["keep_alive"], "5m");
}

#[test]
fn 思考开关透传为_think() {
    let mut req = request(Vec::new(), "5m");
    assert_eq!(build_body(&req)["think"], false);
    req.params.think = true;
    assert_eq!(build_body(&req)["think"], true);
}
