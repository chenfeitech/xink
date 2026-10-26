//! 对真实 Ollama 的端到端验证
//!
//! 默认 `#[ignore]`：CI 不要求装 Ollama。手动跑：
//! `cargo test -- --ignored core::ai::ollama`
//! 前提：本机 `http://localhost:11434` 有 Ollama 且至少拉了一个模型。

use crate::core::ai::ollama::{list_models, probe, chat_endpoint, OllamaProvider};
use crate::core::ai::{ChatMessage, CompletionRequest, Provider, StreamEvent};
use crate::core::config::AiModelParams;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
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
        params: AiModelParams { think: false, num_predict: 64, ..Default::default() },
        api_key: None,
        first_byte_timeout: Duration::from_secs(30),
        silence_timeout: Duration::from_secs(40),
    };
    let provider = OllamaProvider;
    let mut text = String::new();
    let mut done = false;
    provider
        .stream(&req, Arc::new(AtomicBool::new(false)), &mut |ev| {
            match ev {
                StreamEvent::Delta(s) => text.push_str(&s),
                StreamEvent::Done => done = true,
                StreamEvent::Failed(e) => panic!("{e}"),
                _ => {}
            }
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
