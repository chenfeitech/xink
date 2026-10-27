// 本模块有一部分 API 是为后续里程碑准备的（Skills 的 `read_skill_resource`、
// 记忆层、CLI provider、工具能力判定、Keychain 写入、模型缓存落盘），当前未接线。
// 统一豁免而不是逐项打 `#[allow]`：逐项打点会把"还没接上"和"真该删"混在一起。
#![allow(dead_code)]

//! AI 领域层：与 UI 完全解耦，可独立单元测试
//!
//! 三条约束（来自 `docs/ai/ai-assistant-design.md`）：
//! 1. **阻塞式**：provider 内部做同步 IO（ureq），由调用方放到后台线程（规则 02 §6）；
//! 2. **取消与静默超时**用 `Arc<AtomicBool>` + 独立读线程 + `flume::recv_timeout` 实现——
//!    阻塞的 `read_line` 无法被打断，只能靠读线程自查标记 + 主循环超时放弃；
//! 3. 只产出 [`StreamEvent`]，UI 侧负责渲染与节流。

pub mod agent_loop;
pub mod context;
pub mod diff;
pub mod keychain;
pub mod models;
pub mod ndjson;
pub mod ollama;
pub mod openai;
pub mod patch;
pub mod probe;
pub mod prompt;
pub mod skills;
pub mod snapshot;
pub mod sse;
pub mod tasks;
pub mod tools;
pub mod verify;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

// 下面这些 re-export 目前只被测试与后续 UI 用到：bin crate 里 `pub use`
// 没有外部消费者会被判为未使用。UI（M1 下半）接入后自然消失。
#[allow(unused_imports)]
pub use context::{ContextScope, PreparedContext, assemble, estimate_tokens};
#[allow(unused_imports)]
pub use diff::{DiffLine, diff_lines};
#[allow(unused_imports)]
pub use keychain::{MacKeychain, MemorySecretStore, SecretStore, resolve_api_key};
#[allow(unused_imports)]
pub use models::{DiscoveredModel, ModelCache, parse_tags};
#[allow(unused_imports)]
pub use prompt::{Action, all_actions};
#[allow(unused_imports)]
pub use snapshot::{snapshot_name, write_snapshot};

use crate::core::config::AiModelParams;
use tools::ToolCall;

/// 对话角色
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 一条消息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[allow(dead_code)]
impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
        }
    }

    /// Ollama / OpenAI 兼容端点用的角色名
    pub fn role_name(&self) -> &'static str {
        match self.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// 一次补全请求
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// 完整 endpoint（Ollama: `{base}/api/chat`；兼容端点: `{base}/chat/completions`）
    pub endpoint: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub params: AiModelParams,
    pub api_key: Option<String>,
    /// 等待首个响应字节的预算（含 Ollama 冷加载，实测秒级）
    pub first_byte_timeout: Duration,
    /// 静默超时：两次数据之间的最大间隔
    pub silence_timeout: Duration,
    /// 工具 schema（[`tools::specs`]）；空表示这一轮不带工具
    ///
    /// 只有连接测试探针通过的模型才会带上它——小模型对着 schema 只会瞎编。
    pub tools: Vec<serde_json::Value>,
}

/// 用量统计（各家字段位置不同，缺失即为 None）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
}

/// 流式事件
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// 正文增量
    Delta(String),
    /// 思维链增量（与正文分开，便于折叠或丢弃）
    Thinking(String),
    /// 模型要求调用工具
    ///
    /// 两家协议的 `arguments` 形状不同（Ollama 给对象、OpenAI 兼容给字符串），
    /// provider 已统一成 JSON 文本；`ToolCall::arguments` 为空串表示无参数。
    ToolCall(ToolCall),
    Usage(Usage),
    Done,
    Failed(AiError),
}

/// AI 失败原因
///
/// 分类的目的只有一个：**给用户一句能行动的提示**，而不是 `ureq` 的原始错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// 未启用 / 未配置 provider
    NotConfigured,
    /// 未选择模型，或模型已不在列表里
    NoModel,
    Auth,
    RateLimit,
    /// 模型不存在（Ollama 对未知模型返回 404）
    ModelNotFound(String),
    Network(String),
    Timeout,
    Protocol(String),
    /// 流在收到结束信号前断开
    Truncated,
    /// 有输出但正文为空（thinking 模型吃掉预算时会这样）
    EmptyResult,
    Canceled,
    Io(String),
}

impl AiError {
    /// 面板上展示的中文文案
    pub fn message(&self) -> String {
        match self {
            Self::NotConfigured => "还没有配置 AI：请到设置 · AI 里启用并选择模型".into(),
            Self::NoModel => "没有选中模型：请到设置 · AI 里选择一个模型".into(),
            Self::Auth => "认证失败：检查密钥是否正确".into(),
            Self::RateLimit => "被服务商限流了，请稍后重试".into(),
            Self::ModelNotFound(name) => {
                format!("模型 {name} 不存在：先用 `ollama pull {name}` 拉取")
            }
            Self::Network(detail) => {
                format!("网络不可用（{detail}）：检查 Ollama 是否已启动、地址是否正确")
            }
            Self::Timeout => "等待响应超时：可以重试，或在设置里调大超时".into(),
            Self::Protocol(detail) => format!("返回内容无法解析（{detail}）"),
            Self::Truncated => "连接中断，结果可能不完整".into(),
            Self::EmptyResult => {
                "模型没有返回正文（可能是思考占用了全部预算）：请重试或关闭「思考」".into()
            }
            Self::Canceled => "已取消".into(),
            Self::Io(detail) => format!("读写失败（{detail}）"),
        }
    }
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for AiError {}

/// Provider：一次流式补全
///
/// 实现必须是阻塞的（同步 IO 直接写在里面），由调用方放到后台线程。
pub trait Provider: Send + Sync {
    /// provider 标识（状态行展示用；M4 接 CLI provider 后会在 UI 里显示）
    fn id(&self) -> &'static str;
    fn stream(
        &self,
        req: &CompletionRequest,
        cancel: Arc<AtomicBool>,
        on: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), AiError>;
}

/// 把 `ureq` 的错误翻译成 [`AiError`]
pub fn classify_ureq_error(err: &ureq::Error) -> AiError {
    match err {
        ureq::Error::StatusCode(code) => match code {
            401 | 403 => AiError::Auth,
            404 => AiError::ModelNotFound(String::new()),
            429 => AiError::RateLimit,
            other => AiError::Protocol(format!("HTTP {other}")),
        },
        ureq::Error::Timeout(_) => AiError::Timeout,
        ureq::Error::Io(e) => AiError::Network(e.to_string()),
        other => AiError::Network(other.to_string()),
    }
}

/// 构造带超时的 ureq Agent。
///
/// 刻意**不设** `timeout_recv_body`：它是"整个响应体的总预算"（ureq 文档明示
/// `The budget is not restarted for each read`），长回答会被误杀。
/// 静默超时由 [`Provider::stream`] 的读线程 + `recv_timeout` 自己管。
pub fn http_agent(first_byte_timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_recv_response(Some(first_byte_timeout))
        .build()
        .into()
}

/// 流式读取的通用骨架：读线程 + flume + 静默超时
///
/// `read` 在**独立线程**里跑阻塞读，每产出一个事件就发到通道；
/// 主循环用 `recv_timeout` 兼做静默超时判定。取消时置标记并放弃读线程
/// （它会在下一帧看到标记后自行退出）。
pub fn pump_stream(
    cancel: Arc<AtomicBool>,
    silence_timeout: Duration,
    read: impl FnOnce(Arc<AtomicBool>, flume::Sender<StreamEvent>) + Send + 'static,
    on: &mut dyn FnMut(StreamEvent),
) -> Result<(), AiError> {
    let (tx, rx) = flume::unbounded::<StreamEvent>();
    std::thread::spawn(move || read(cancel.clone(), tx));

    loop {
        match rx.recv_timeout(silence_timeout) {
            Ok(ev) => {
                let finished = matches!(ev, StreamEvent::Done | StreamEvent::Failed(_));
                on(ev);
                if finished {
                    return Ok(());
                }
            }
            Err(flume::RecvTimeoutError::Timeout) => {
                return Err(AiError::Timeout);
            }
            Err(flume::RecvTimeoutError::Disconnected) => {
                // 读线程结束却没发结束事件：等同连接中断
                return Err(AiError::Truncated);
            }
        }
    }
}
