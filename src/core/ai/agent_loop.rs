//! Agent loop：gather → act → verify 的推进器
//!
//! 设计：`docs/ai/ai-assistant-design.md` §3.2。这是**阻塞**的一段，
//! 由调用方放到后台线程（规则 02 §6）——与 provider 一样。
//!
//! 两条行为约定：
//! 1. 模型不支持工具（`tools_enabled == false`）时**退化为单轮**，
//!    与不进 loop 的表现完全一致，因此小模型不会因为带了 schema 而变差；
//! 2. 校验（[`verify`]）不过时把问题回灌让模型自己改，最多重试
//!    [`MAX_VERIFY_RETRY`] 次；仍不过也照常返回，**不阻断**。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::tools::{ToolCall, ToolHost, specs};
use super::verify::{LinkIndex, VerifyIssue};
use crate::core::config::AiModelParams;

use super::{
    AiError, ChatMessage, CompletionRequest, Provider, StreamEvent, Usage, tools as tool_mod,
};

/// 校验失败后的重试次数上限
pub const MAX_VERIFY_RETRY: u8 = 2;

/// 一次 loop 的输入
pub struct LoopRequest<'a> {
    pub endpoint: String,
    pub model: String,
    pub params: AiModelParams,
    pub api_key: Option<String>,
    /// 起始消息（system / user 已组装好）
    pub messages: Vec<ChatMessage>,
    /// 模型是否真的支持工具（连接测试探针给的结论）
    pub tools_enabled: bool,
    /// skills 总开关：关掉时不给模型 `read_skill_resource`
    pub skills_enabled: bool,
    /// 步数上限（单篇默认 8）
    pub max_steps: u8,
    /// 单条工具结果回传的字符上限
    pub max_tool_chars: usize,
    pub host: &'a dyn ToolHost,
    /// 没打开库时为 `None`，此时跳过双链校验
    pub index: Option<&'a dyn LinkIndex>,
    pub first_byte_timeout: Duration,
    pub silence_timeout: Duration,
}

/// loop 的过程事件（给 UI 展示"它在干什么"）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopEvent {
    /// 最终文本的增量（中间步骤的文本不在这里，避免和最终稿混在一起）
    Delta(String),
    /// 模型要求调用工具
    ToolCall {
        name: String,
        /// 参数的简短摘要，给 UI 一行显示
        args_summary: String,
    },
    /// 工具执行完了
    ToolResult {
        name: String,
        lines: usize,
        chars: usize,
        /// 结果正文（工具层已按 `max_tool_chars` 截断）
        ///
        /// UI 要"展开看细节"，因此事件里带上原文；它同时也是回灌给模型的那份文本。
        content: String,
    },
    /// 校验没过，已把问题回灌让模型重写
    VerifyFailed(Vec<VerifyIssue>),
    /// 结束（成功）
    Done,
}

/// loop 的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopOutcome {
    pub text: String,
    /// 实际走的步数
    pub steps: u32,
    /// 用过的工具名（按调用顺序，可重复）
    pub tools: Vec<String>,
    pub usage: Option<Usage>,
}

/// 推进一轮：到拿到最终文本、步数用尽或被取消为止
pub fn run(
    req: &LoopRequest,
    provider: &dyn Provider,
    cancel: Arc<AtomicBool>,
    on: &mut dyn FnMut(LoopEvent),
) -> Result<LoopOutcome, AiError> {
    let tools = if req.tools_enabled {
        specs(req.skills_enabled)
    } else {
        Vec::new()
    };
    let max_steps = req.max_steps.max(1) as u32;

    let mut messages = req.messages.clone();
    let mut used_tools: Vec<String> = Vec::new();
    let mut retries = 0u8;
    let mut last_usage = None;

    for step in 1..=max_steps {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Canceled);
        }

        let request = CompletionRequest {
            endpoint: req.endpoint.clone(),
            model: req.model.clone(),
            messages: messages.clone(),
            params: req.params.clone(),
            api_key: req.api_key.clone(),
            first_byte_timeout: req.first_byte_timeout,
            silence_timeout: req.silence_timeout,
            tools: tools.clone(),
        };

        let mut step_text = String::new();
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut failed: Option<AiError> = None;
        provider.stream(&request, cancel.clone(), &mut |ev| match ev {
            StreamEvent::Delta(text) => {
                step_text.push_str(&text);
                on(LoopEvent::Delta(text));
            }
            StreamEvent::ToolCall(call) => calls.push(call),
            StreamEvent::Usage(usage) => last_usage = Some(usage),
            StreamEvent::Failed(err) => failed = Some(err),
            // 思维链与结束信号在 loop 里不用：思维链不回灌，结束由本函数自己判定。
            // 预算耗尽同样不处理：loop 只关心"有没有工具调用"与"正文是否为空"，
            // 半截答案在这里会被照常收下；"结果可能不全"由 UI 侧（`ui/ai.rs`）提示。
            StreamEvent::Thinking(_) | StreamEvent::Truncated | StreamEvent::Done => {}
        })?;
        if let Some(err) = failed {
            return Err(err);
        }

        if calls.is_empty() {
            // 没有工具调用 = 这是最终答案
            if step_text.trim().is_empty() {
                return Err(AiError::EmptyResult);
            }
            if let Some(index) = req.index {
                let issues = crate::core::ai::verify::check(&step_text, Some(index));
                if !issues.is_empty() && retries < MAX_VERIFY_RETRY {
                    retries += 1;
                    on(LoopEvent::VerifyFailed(issues.clone()));
                    messages.push(ChatMessage::user(verify_feedback(&issues)));
                    continue;
                }
            }
            on(LoopEvent::Done);
            return Ok(LoopOutcome {
                text: step_text,
                steps: step,
                tools: used_tools,
                usage: last_usage,
            });
        }

        // 执行工具并把结果回灌
        for call in &calls {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Canceled);
            }
            on(LoopEvent::ToolCall {
                name: call.name.clone(),
                args_summary: summarize(&call.arguments, 60),
            });
            let result = tool_mod::execute(call, req.host, req.max_tool_chars);
            on(LoopEvent::ToolResult {
                name: result.name.clone(),
                lines: result.output.lines().count(),
                chars: result.output.chars().count(),
                content: result.output.clone(),
            });
            used_tools.push(call.name.clone());
            // 用 user 消息回灌而不是 role=tool：后者各家都要求 tool_call_id，
            // 而我们的 ChatMessage 只有 role+content（最小请求面，见 openai.rs）
            messages.push(ChatMessage::user(format!(
                "工具 {} 的结果：\n{}",
                call.name, result.output
            )));
        }
    }

    // 步数用尽：走到这里说明模型一直在要工具，没给最终答案
    Err(AiError::EmptyResult)
}

/// 把校验问题拼成回灌给模型的文本
fn verify_feedback(issues: &[VerifyIssue]) -> String {
    let mut text = String::from("上面的结果有结构问题，请改好后重新输出完整正文：\n");
    for issue in issues {
        text.push_str("- ");
        text.push_str(&issue.as_feedback());
        text.push('\n');
    }
    text
}

/// 参数摘要：给 UI 一行显示，不把整包 JSON 摊在界面上
fn summarize(arguments: &str, max_chars: usize) -> String {
    let flat: String = arguments.split_whitespace().collect::<Vec<_>>().join(" ");
    let total = flat.chars().count();
    if total <= max_chars {
        return flat;
    }
    format!("{}…", flat.chars().take(max_chars).collect::<String>())
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/agent_loop.rs"]
mod tests;
