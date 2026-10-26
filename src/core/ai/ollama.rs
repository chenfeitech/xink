//! Ollama provider：走 native `/api/chat`（NDJSON）
//!
//! 为什么不走 `/v1/chat/completions`：实测兼容端点**忽略 `think:false`**，
//! thinking 模型会把生成预算全花在思维链上、正文为空；native 端点才认 `think`，
//! 且带 `keep_alive` / `num_ctx` 与 `/api/tags` 发现。详见
//! `docs/ai/ollama-provider-design.md` §2.3。

use serde_json::json;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::models::{DiscoveredModel, parse_tags};
use super::ndjson::NdjsonParser;
use super::tools::ToolCall;
use super::{
    AiError, CompletionRequest, Provider, StreamEvent, Usage, classify_ureq_error, http_agent,
    pump_stream,
};

pub struct OllamaProvider;

/// 去掉结尾斜杠后拼出 endpoint
pub fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), path)
}

pub fn chat_endpoint(base_url: &str) -> String {
    endpoint(base_url, "/api/chat")
}

/// 拉取本机模型列表
pub fn list_models(base_url: &str, timeout: Duration) -> Result<Vec<DiscoveredModel>, AiError> {
    let agent = http_agent(timeout);
    let mut resp = agent
        .get(endpoint(base_url, "/api/tags").as_str())
        .call()
        .map_err(|e| classify_ureq_error(&e))?;
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| AiError::Network(e.to_string()))?;
    parse_tags(&text)
}

/// 连接测试的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub ok: bool,
    pub error: Option<AiError>,
    /// 是否真的产出过一次工具调用
    pub supports_tools: bool,
    pub elapsed_ms: u64,
}

/// 连接测试：一条最小消息 + 可选工具探针
///
/// 能力声明不可信（本机 `qwen3.5:0.8b` 声明 tools 却在探针时返回 500），
/// 所以「是否支持工具」一律以探针结果为准。
pub fn probe(base_url: &str, model: &str, timeout: Duration, with_tools: bool) -> ProbeResult {
    let started = std::time::Instant::now();
    let tools = if with_tools {
        vec![json!({
            "type": "function",
            "function": {
                "name": "noop",
                "description": "连接测试用：不需要调用它",
                "parameters": { "type": "object", "properties": {} }
            }
        })]
    } else {
        Vec::new()
    };

    let mut body = json!({
        "model": model,
        "stream": false,
        "think": false,
        "options": { "num_predict": 8 },
        "messages": [ {"role": "user", "content": "ping"} ]
    });
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }

    let result = http_agent(timeout)
        .post(chat_endpoint(base_url).as_str())
        .send_json(body);

    match result {
        Ok(mut resp) => {
            let text = resp.body_mut().read_to_string().unwrap_or_default();
            let parsed: Option<serde_json::Value> = serde_json::from_str(&text).ok();
            let tool_called = parsed
                .as_ref()
                .and_then(|v| v["message"]["tool_calls"].as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            ProbeResult {
                ok: true,
                error: None,
                supports_tools: tool_called,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }
        }
        Err(e) => ProbeResult {
            ok: false,
            error: Some(classify_ureq_error(&e)),
            supports_tools: false,
            elapsed_ms: started.elapsed().as_millis() as u64,
        },
    }
}

impl Provider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn stream(
        &self,
        req: &CompletionRequest,
        cancel: Arc<AtomicBool>,
        on: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), AiError> {
        let body = build_body(req);
        let endpoint = req.endpoint.clone();
        let model = req.model.clone();
        let first_byte = req.first_byte_timeout;

        pump_stream(
            cancel,
            req.silence_timeout,
            move |cancel, tx| {
                let agent = http_agent(first_byte);
                let mut resp = match agent.post(endpoint.as_str()).send_json(body) {
                    Ok(r) => r,
                    Err(e) => {
                        let err = classify_ureq_error(&e);
                        // Ollama 对未知模型返回 404，这里把模型名补进提示
                        let err = match err {
                            AiError::ModelNotFound(_) => AiError::ModelNotFound(model.clone()),
                            other => other,
                        };
                        let _ = tx.send(StreamEvent::Failed(err));
                        return;
                    }
                };

                let mut reader = BufReader::new(resp.body_mut().as_reader());
                let mut parser = NdjsonParser::new();
                let mut lines: Vec<String> = Vec::new();
                let mut line = String::new();
                let mut usage = Usage::default();
                let mut events: Vec<StreamEvent> = Vec::new();

                loop {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = tx.send(StreamEvent::Failed(AiError::Canceled));
                        return;
                    }
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            parser.push(&line, &mut lines);
                            for raw in lines.drain(..) {
                                frame_to_events(&raw, &mut usage, &mut events);
                                for ev in events.drain(..) {
                                    let finished =
                                        matches!(ev, StreamEvent::Done | StreamEvent::Failed(_));
                                    if tx.send(ev).is_err() {
                                        return;
                                    }
                                    if finished {
                                        return;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(StreamEvent::Failed(AiError::Network(e.to_string())));
                            return;
                        }
                    }
                }

                // 正常读到 EOF：把残留帧处理掉再收尾
                parser.finish(&mut lines);
                for raw in lines.drain(..) {
                    frame_to_events(&raw, &mut usage, &mut events);
                    for ev in events.drain(..) {
                        let finished = matches!(ev, StreamEvent::Done | StreamEvent::Failed(_));
                        if tx.send(ev).is_err() {
                            return;
                        }
                        if finished {
                            return;
                        }
                    }
                }
                let _ = tx.send(StreamEvent::Usage(usage));
                let _ = tx.send(StreamEvent::Done);
            },
            on,
        )
    }
}

/// 请求体：正文 / 思考分离，`think` 由参数控制
fn build_body(req: &CompletionRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .map(|m| json!({ "role": m.role_name(), "content": m.content }))
        .collect();

    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "think": req.params.think,
        "options": {
            "num_ctx": req.params.num_ctx,
            "num_predict": req.params.num_predict,
            "temperature": req.params.temperature,
        }
    });
    if !req.params.keep_alive.trim().is_empty() {
        body["keep_alive"] = json!(req.params.keep_alive);
    }
    if !req.tools.is_empty() {
        body["tools"] = json!(req.tools);
    }
    body
}

/// 一帧 NDJSON → 若干事件
///
/// 实测字段：`message.content`（正文）、`message.thinking`（思维链）、
/// `done:true` 时带 `prompt_eval_count` / `eval_count` / `eval_duration`（纳秒）。
fn frame_to_events(line: &str, usage: &mut Usage, out: &mut Vec<StreamEvent>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        out.push(StreamEvent::Failed(AiError::Protocol(err.to_string())));
        return;
    }
    if let Some(text) = value["message"]["content"].as_str() {
        if !text.is_empty() {
            out.push(StreamEvent::Delta(text.to_string()));
        }
    }
    if let Some(text) = value["message"]["thinking"].as_str() {
        if !text.is_empty() {
            out.push(StreamEvent::Thinking(text.to_string()));
        }
    }
    // 工具调用：Ollama 整帧给完整的 `tool_calls`（不像 OpenAI 那样分片），
    // 而且 `arguments` 是**对象**；统一成 JSON 文本给上层
    if let Some(calls) = value["message"]
        .get("tool_calls")
        .and_then(|v| v.as_array())
    {
        for call in calls {
            let Some(name) = call["function"]["name"].as_str() else {
                continue;
            };
            let arguments = match call["function"].get("arguments") {
                Some(v) if v.is_string() => v.as_str().unwrap_or_default().to_string(),
                // 对象（Ollama 的形状）→ 序列化回文本；没有该字段视为空参数
                Some(v) => v.to_string(),
                None => String::new(),
            };
            out.push(StreamEvent::ToolCall(ToolCall {
                name: name.to_string(),
                arguments,
            }));
        }
    }
    if value["done"].as_bool() == Some(true) {
        usage.prompt_tokens = value["prompt_eval_count"].as_u64();
        usage.completion_tokens = value["eval_count"].as_u64();
        usage.duration_ms = value["eval_duration"]
            .as_u64()
            .map(|ns| ns / 1_000_000)
            .or_else(|| value["total_duration"].as_u64().map(|ns| ns / 1_000_000));
        out.push(StreamEvent::Done);
    }
}

// 帧解析单测（不需要本机 Ollama，常跑）
#[cfg(test)]
#[path = "../../../tests/core/ai/ollama_parse.rs"]
mod parse_tests;

// 端到端测试（需本机 Ollama，默认 ignore）
#[cfg(test)]
#[path = "../../../tests/core/ai/ollama.rs"]
mod e2e;
