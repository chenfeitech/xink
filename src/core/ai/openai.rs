//! OpenAI 兼容 provider（`POST {base}/chat/completions`，SSE）
//!
//! 只用最保守的请求字段（model / messages / stream / temperature / max_tokens）：
//! `stream_options.include_usage` 各家支持度不一，宁可不发，用量显示为「—」。

use serde_json::json;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::sse::{SseFrame, SseParser};
use super::{
    AiError, CompletionRequest, Provider, StreamEvent, Usage, classify_ureq_error, http_agent,
    pump_stream,
};

pub struct OpenAiProvider;

/// `base_url` 是**整体可配**的：DeepSeek 是 `https://api.deepseek.com/v1`，
/// 智谱是 `https://open.bigmodel.cn/api/paas/v4`——不能假设都有 `/v1`。
pub fn chat_endpoint(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

impl Provider for OpenAiProvider {
    fn id(&self) -> &'static str {
        "openai_compatible"
    }

    fn stream(
        &self,
        req: &CompletionRequest,
        cancel: Arc<AtomicBool>,
        on: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), AiError> {
        let body = build_body(req);
        let endpoint = req.endpoint.clone();
        let api_key = req.api_key.clone();
        let first_byte = req.first_byte_timeout;

        pump_stream(
            cancel,
            req.silence_timeout,
            move |cancel, tx| {
                let agent = http_agent(first_byte);
                let mut request = agent.post(endpoint.as_str());
                if let Some(key) = api_key.as_deref().filter(|k| !k.is_empty()) {
                    request = request.header("Authorization", format!("Bearer {key}"));
                }
                let mut resp = match request.send_json(body) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(StreamEvent::Failed(classify_ureq_error(&e)));
                        return;
                    }
                };

                let mut reader = BufReader::new(resp.body_mut().as_reader());
                let mut parser = SseParser::new();
                let mut frames: Vec<SseFrame> = Vec::new();
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
                            parser.push(&line, &mut frames);
                            if !drain(&mut frames, &mut usage, &mut events, &tx) {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(StreamEvent::Failed(AiError::Network(e.to_string())));
                            return;
                        }
                    }
                }
                parser.finish(&mut frames);
                let _ = drain(&mut frames, &mut usage, &mut events, &tx);
                let _ = tx.send(StreamEvent::Usage(usage));
                let _ = tx.send(StreamEvent::Done);
            },
            on,
        )
    }
}

/// 消费解析出的帧；返回 false 表示已结束（或通道已断），调用方应退出
fn drain(
    frames: &mut Vec<SseFrame>,
    usage: &mut Usage,
    events: &mut Vec<StreamEvent>,
    tx: &flume::Sender<StreamEvent>,
) -> bool {
    for frame in frames.drain(..) {
        match frame {
            SseFrame::Done => {
                let _ = tx.send(StreamEvent::Usage(*usage));
                let _ = tx.send(StreamEvent::Done);
                return false;
            }
            SseFrame::Data(payload) => {
                frame_to_events(&payload, usage, events);
                for ev in events.drain(..) {
                    let finished = matches!(ev, StreamEvent::Done | StreamEvent::Failed(_));
                    if tx.send(ev).is_err() {
                        return false;
                    }
                    if finished {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// 一帧 SSE → 若干事件
///
/// 容错要点（调研确认，见 `docs/ai/ai-capabilities-eval.md` §4.1）：
/// - 首帧可能只有 `delta.role`；usage 帧可能是 `choices: []`，**不能无脑取 `choices[0]`**；
/// - 思维链字段各家不同：Ollama 用 `reasoning`，多数云厂商用 `reasoning_content`。
fn frame_to_events(payload: &str, usage: &mut Usage, out: &mut Vec<StreamEvent>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return;
    };
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        out.push(StreamEvent::Failed(AiError::Protocol(err.to_string())));
        return;
    }
    if let Some(used) = value.get("usage") {
        usage.prompt_tokens = used["prompt_tokens"].as_u64();
        usage.completion_tokens = used["completion_tokens"].as_u64();
    }
    let Some(choice) = value.get("choices").and_then(|c| c.get(0)) else {
        return;
    };
    let delta = &choice["delta"];
    if let Some(text) = delta["content"].as_str() {
        if !text.is_empty() {
            out.push(StreamEvent::Delta(text.to_string()));
        }
    }
    for key in ["reasoning", "reasoning_content"] {
        if let Some(text) = delta[key].as_str() {
            if !text.is_empty() {
                out.push(StreamEvent::Thinking(text.to_string()));
            }
        }
    }
    if choice
        .get("finish_reason")
        .and_then(|v| v.as_str())
        .is_some()
    {
        out.push(StreamEvent::Done);
    }
}

fn build_body(req: &CompletionRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .map(|m| json!({ "role": m.role_name(), "content": m.content }))
        .collect();
    json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "temperature": req.params.temperature,
        "max_tokens": req.params.num_predict,
    })
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/openai.rs"]
mod tests;
