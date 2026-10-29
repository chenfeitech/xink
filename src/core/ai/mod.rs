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
pub mod capability;
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

use std::collections::HashMap;
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
pub use ollama::OllamaProvider;
#[allow(unused_imports)]
pub use openai::OpenAiProvider;
#[allow(unused_imports)]
pub use prompt::KNOWN_TAGS_PLACEHOLDER;
#[allow(unused_imports)]
pub use snapshot::{snapshot_name, write_snapshot};

use crate::core::config::{AiProviderKind, ResolvedParams};
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
    /// 已解析的生成参数（全局配置已回落完毕）
    ///
    /// 用 [`ResolvedParams`] 而不是 `AiModelParams`：后者的 `num_ctx` /
    /// `keep_alive` 是 `Option`，直接发会让这两个字段整个消失。
    /// 走 [`AiConfig::params_for`] 构造。
    pub params: ResolvedParams,
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
    /// 生成预算耗尽：协议侧 `done_reason` / `finish_reason` 为 `"length"`，
    /// 模型是"到顶就停"而不是正常收尾，结果多半是半截的。
    ///
    /// **不是** [`AiError::Truncated`]——那个表示连接中断（并且被判为可重试，
    /// 见 [`is_retryable`]）；预算耗尽重试一遍只是再烧一次 token，没有意义，
    /// 所以它是"已经拿到结果、只是结果不全"，不是错误。
    ///
    /// 必须在 [`Self::Done`] **之前**发出：UI 收到 `Done` 就收尾了
    /// （`ui/ai.rs` 的 `finish()`），之后再来的事件没有落点。
    Truncated,
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

/// 一个 provider 的构造与它的协议细节
///
/// 只放"每种 provider 都必然有"的东西：**怎么造实例**、**聊天端点怎么拼**。
/// 列模型、连接探针这类只有部分 provider 具备的能力不进这里——塞进来会逼着
/// 别人写 `unimplemented!()`，保持它们是模块级自由函数。
pub trait ProviderFactory: Send + Sync {
    fn build(&self) -> Arc<dyn Provider>;
    fn chat_endpoint(&self, base_url: &str) -> String;
}

struct UnitFactory<F, E>
where
    F: Fn() -> Arc<dyn Provider> + Send + Sync,
    E: Fn(&str) -> String + Send + Sync,
{
    build: F,
    endpoint: E,
}

impl<F, E> ProviderFactory for UnitFactory<F, E>
where
    F: Fn() -> Arc<dyn Provider> + Send + Sync,
    E: Fn(&str) -> String + Send + Sync,
{
    fn build(&self) -> Arc<dyn Provider> {
        (self.build)()
    }
    fn chat_endpoint(&self, base_url: &str) -> String {
        (self.endpoint)(base_url)
    }
}

/// 这个错误值得重试吗
///
/// 判据是"再试一次有没有可能成功"：限流 / 超时 / 网络抖动 / 流被截断都可能；
/// 鉴权失败、模型不存在、协议错误再试多少次都一样。
pub fn is_retryable(err: &AiError) -> bool {
    matches!(
        err,
        AiError::RateLimit | AiError::Timeout | AiError::Network(_) | AiError::Truncated
    )
}

/// 带重试与备用的流式调用
///
/// 三条约束：
/// 1. **只重试还没吐出内容的请求**——已经把半截正文交给 UI 了，再跑一遍会把
///    两半拼在一起，宁可直接失败；
/// 2. 退避是指数的（`backoff_ms * 2^n`），不做固定间隔，免得把限流的模型打得更死；
/// 3. 备用模型只在主用**重试耗尽**后才用，且同样受上面两条约束。
///
/// 阻塞函数，**只在后台线程调用**（内部会 `sleep`）。
pub fn stream_with_failover(
    attempts: &[(&dyn Provider, &CompletionRequest)],
    cancel: Arc<AtomicBool>,
    max_attempts: u8,
    backoff_ms: u64,
    on: &mut dyn FnMut(StreamEvent),
) -> Result<(), AiError> {
    use std::sync::atomic::Ordering;
    use std::thread;

    let mut last: Option<AiError> = None;
    for (provider, req) in attempts {
        for attempt in 0..max_attempts.max(1) {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Canceled);
            }
            if attempt > 0 {
                let wait = backoff_ms.saturating_mul(1u64 << (attempt - 1).min(4));
                thread::sleep(Duration::from_millis(wait.min(10_000)));
            }
            let mut progressed = false;
            let result = {
                let mut watch = |ev: StreamEvent| {
                    if matches!(ev, StreamEvent::Delta(_) | StreamEvent::ToolCall(_)) {
                        progressed = true;
                    }
                    on(ev);
                };
                provider.stream(req, cancel.clone(), &mut watch)
            };
            match result {
                Ok(()) => return Ok(()),
                Err(err) => {
                    if !is_retryable(&err) || progressed {
                        return Err(err);
                    }
                    last = Some(err);
                }
            }
        }
    }
    Err(last.unwrap_or(AiError::NoModel))
}

/// Provider 注册表：`kind → 工厂`
///
/// 以前 `kind → provider` 是 UI 层一处硬编码 `match`，端点又在配置层另一处
/// `match`，加一个 provider 要同时改三处。收进注册表后：
/// **新增 provider = 写一个文件实现 `Provider` + 在 `builtin()` 里注册一行**，
/// 请求链路一行都不用动。
#[derive(Default)]
pub struct ProviderRegistry {
    factories: HashMap<AiProviderKind, Arc<dyn ProviderFactory>>,
}

impl ProviderRegistry {
    /// 内置两个：Ollama native 与 OpenAI 兼容
    pub fn builtin() -> Self {
        let mut reg = Self::default();
        reg.register(
            AiProviderKind::Ollama,
            || Arc::new(OllamaProvider) as Arc<dyn Provider>,
            ollama::chat_endpoint,
        );
        reg.register(
            AiProviderKind::OpenAiCompatible,
            || Arc::new(OpenAiProvider) as Arc<dyn Provider>,
            openai::chat_endpoint,
        );
        reg
    }

    pub fn register<B, E>(&mut self, kind: AiProviderKind, build: B, endpoint: E)
    where
        B: Fn() -> Arc<dyn Provider> + Send + Sync + 'static,
        E: Fn(&str) -> String + Send + Sync + 'static,
    {
        self.factories
            .insert(kind, Arc::new(UnitFactory { build, endpoint }));
    }

    /// 取 provider；没注册过就报语义错误，不 panic
    pub fn resolve(&self, kind: AiProviderKind) -> Result<Arc<dyn Provider>, AiError> {
        self.factories
            .get(&kind)
            .map(|f| f.build())
            .ok_or_else(|| AiError::Protocol(format!("未注册的 provider：{kind:?}")))
    }

    /// 拼聊天端点；未注册的 kind 原样返回 base（枚举是唯一入口，实际到不了）
    pub fn endpoint(&self, kind: AiProviderKind, base_url: &str) -> String {
        self.factories
            .get(&kind)
            .map(|f| f.chat_endpoint(base_url))
            .unwrap_or_else(|| base_url.to_string())
    }
}

/// 进程内唯一的注册表
///
/// 两个内置 provider 都是零字段 unit struct，没有可变状态，做成全局单例省得
/// 把 registry 一路透传到每个请求点。
pub fn providers() -> &'static ProviderRegistry {
    static REGISTRY: std::sync::OnceLock<ProviderRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(ProviderRegistry::builtin)
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
