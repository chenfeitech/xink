//! 单模型连接测试
//!
//! 需求 A1.4 / 设计 D5：**能力声明不可信**。本机 `qwen3.5:0.8b` 就在
//! `/api/tags` 里声明了自己支持 tools，真发工具过去却返回 500。因此
//! 「是否支持工具」一律以一次真实探针为准：
//! - Ollama：走 `ollama::probe`（已有，带 noop 工具）；
//! - OpenAI 兼容：发一次 `max_tokens=1` 的最小请求，同样带 noop 工具，
//!   看回包里有没有 `tool_calls`。
//!
//! 探针结论写回 `ModelEntry.tools_enabled`，agent loop 只看这个字段。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;

use super::ollama::{ProbeResult, probe as probe_ollama};
use super::{AiError, ChatMessage, CompletionRequest, StreamEvent};
use crate::core::ai::Provider;
use crate::core::ai::keychain::{MacKeychain, resolve_api_key};
use crate::core::ai::openai::OpenAiProvider;
use crate::core::config::{AiConfig, AiProviderKind, ModelEntry, ResolvedParams};

/// 测试一个模型：通不通、支不支持工具、花了多久
pub fn probe_model(cfg: &AiConfig, model: &ModelEntry) -> ProbeResult {
    let timeout = probe_timeout(cfg.timeout_secs);
    match model.kind {
        AiProviderKind::Ollama => {
            let base = model.base_url_or(cfg);
            probe_ollama(&base, &model.id, timeout, true)
        }
        AiProviderKind::OpenAiCompatible => probe_compatible(
            &cfg.endpoint_for(model),
            &model.id,
            resolve_api_key(model, &MacKeychain),
            timeout,
        ),
    }
}

/// OpenAI 兼容端点：一次最小请求 + 工具探针
fn probe_compatible(
    endpoint: &str,
    model: &str,
    api_key: Option<String>,
    timeout: Duration,
) -> ProbeResult {
    let started = std::time::Instant::now();
    let request = CompletionRequest {
        endpoint: endpoint.to_string(),
        model: model.to_string(),
        messages: vec![ChatMessage::user("ping")],
        params: ResolvedParams {
            // 只要 1 个 token：探针看的是"通不通"和"会不会调工具"
            num_predict: 1,
            think: false,
            ..ResolvedParams::default()
        },
        api_key,
        first_byte_timeout: timeout,
        silence_timeout: timeout,
        tools: vec![noop_tool()],
    };

    let mut tool_called = false;
    let mut got_text = false;
    let mut failed = None;
    // 探针是一次性的，不需要取消标记
    let _ = OpenAiProvider.stream(
        &request,
        Arc::new(AtomicBool::new(false)),
        &mut |ev| match ev {
            StreamEvent::Delta(text) => got_text |= !text.is_empty(),
            StreamEvent::ToolCall(_) => tool_called = true,
            StreamEvent::Failed(err) => failed = Some(err),
            _ => {}
        },
    );

    probe_outcome(
        failed,
        got_text,
        tool_called,
        started.elapsed().as_millis() as u64,
    )
}

/// 把一次探针的观察结果变成结论（纯判定，抽出来好单测）
///
/// 这是"能力声明不可信"这条设计里**唯一**的决策点：判定错了的表现是
/// "设置里显示支持工具，跑起来却永远不调"，很难查。
fn probe_outcome(
    failed: Option<AiError>,
    got_text: bool,
    tool_called: bool,
    elapsed_ms: u64,
) -> ProbeResult {
    match failed {
        Some(err) => ProbeResult {
            ok: false,
            error: Some(err),
            supports_tools: false,
            elapsed_ms,
        },
        None => ProbeResult {
            // 既没正文也没工具调用：多半是端点回了空 choices，算不通
            ok: got_text || tool_called,
            error: None,
            supports_tools: tool_called,
            elapsed_ms,
        },
    }
}

/// 探针的超时预算：冷启动要几秒，太短只会得到假阴性
fn probe_timeout(timeout_secs: u64) -> Duration {
    Duration::from_secs(timeout_secs.max(5))
}

/// 探针用的空工具：真实调用它没有任何意义，只为看模型会不会调
///
/// 与 `tools.rs` 的 schema 形状必须一致，否则探针测的是"另一套协议"。
fn noop_tool() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "noop",
            "description": "连接测试用：不需要调用它",
            "parameters": { "type": "object", "properties": {} }
        }
    })
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/probe.rs"]
mod tests;
