//! Agent loop：步数、工具回灌、校验重试、取消、无工具时退化

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{LoopEvent, LoopRequest, MAX_VERIFY_RETRY, run};
use crate::core::ai::tools::ToolHost;
use crate::core::ai::verify::LinkIndex;
use crate::core::ai::{AiError, ChatMessage, CompletionRequest, Provider, StreamEvent, Usage};
use crate::core::config::AiModelParams;
use crate::core::index::Resolution;
use crate::core::search::Hit;

/// 每一"步"要发什么事件
type Step = Vec<StreamEvent>;

/// 脚本化的 provider：按步返回预设事件，并记录收到的请求
///
/// `Provider: Send + Sync`，所以这里不能用 `Rc<RefCell<_>>`。
struct FakeProvider {
    steps: Mutex<Vec<Step>>,
    seen: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl FakeProvider {
    fn new(steps: Vec<Step>) -> (Self, Arc<Mutex<Vec<CompletionRequest>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                steps: Mutex::new(steps),
                seen: seen.clone(),
            },
            seen,
        )
    }
}

impl Provider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn stream(
        &self,
        req: &CompletionRequest,
        _cancel: Arc<AtomicBool>,
        on: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), AiError> {
        self.seen.lock().unwrap().push(req.clone());
        let mut steps = self.steps.lock().unwrap();
        let step = if steps.is_empty() {
            vec![StreamEvent::Delta("脚本用尽".into()), StreamEvent::Done]
        } else {
            steps.remove(0)
        };
        for ev in step {
            on(ev);
        }
        Ok(())
    }
}

struct Host;

impl ToolHost for Host {
    fn read_note(&self, _path: &Path) -> Option<String> {
        Some("被读到的正文".into())
    }
    fn search(&self, _query: &str, _limit: usize) -> Vec<Hit> {
        vec![Hit {
            path: PathBuf::from("/vault/a.md"),
            line: 0,
            text: "命中一行".into(),
        }]
    }
    fn tags(&self) -> Vec<(String, usize)> {
        vec![("rust".into(), 1)]
    }
    fn backlinks(&self, _path: &Path) -> Vec<crate::core::index::LinkRef> {
        Vec::new()
    }
    fn list_notes(&self, _limit: usize) -> Vec<(PathBuf, String)> {
        vec![(PathBuf::from("/vault/a.md"), "A".into())]
    }
    fn root(&self) -> Option<PathBuf> {
        Some(PathBuf::from("/vault"))
    }
}

/// 双链一律解析不到：用来触发校验失败
struct NoLinks;

impl LinkIndex for NoLinks {
    fn resolve(&self, _target: &str) -> Resolution {
        Resolution::Missing
    }
}

fn request<'a>(
    host: &'a dyn ToolHost,
    index: Option<&'a dyn LinkIndex>,
    tools_enabled: bool,
    max_steps: u8,
) -> LoopRequest<'a> {
    LoopRequest {
        endpoint: "http://localhost/fake".into(),
        model: "fake".into(),
        params: AiModelParams::default(),
        api_key: None,
        messages: vec![ChatMessage::user("帮我看看")],
        tools_enabled,
        skills_enabled: true,
        max_steps,
        max_tool_chars: 4000,
        host,
        index,
        first_byte_timeout: Duration::from_secs(1),
        silence_timeout: Duration::from_secs(1),
    }
}

fn collect(events: &Arc<Mutex<Vec<LoopEvent>>>) -> impl FnMut(LoopEvent) + '_ {
    let sink = events.clone();
    move |ev| sink.lock().unwrap().push(ev)
}

#[test]
fn 没有工具调用时一轮就结束() {
    let (provider, seen) = FakeProvider::new(vec![vec![
        StreamEvent::Delta("答案".into()),
        StreamEvent::Usage(Usage::default()),
        StreamEvent::Done,
    ]]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let outcome = run(
        &request(&Host, None, true, 8),
        &provider,
        Arc::new(AtomicBool::new(false)),
        &mut collect(&events),
    )
    .unwrap();

    assert_eq!(outcome.text, "答案");
    assert_eq!(outcome.steps, 1);
    assert!(outcome.tools.is_empty());
    // 开了工具就该把 schema 带上
    assert!(!seen.lock().unwrap()[0].tools.is_empty());
    assert!(matches!(
        events.lock().unwrap().last(),
        Some(LoopEvent::Done)
    ));
}

#[test]
fn 关掉工具时不带_schema() {
    let (provider, seen) = FakeProvider::new(vec![vec![
        StreamEvent::Delta("答案".into()),
        StreamEvent::Done,
    ]]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let outcome = run(
        &request(&Host, None, false, 8),
        &provider,
        Arc::new(AtomicBool::new(false)),
        &mut collect(&events),
    )
    .unwrap();

    assert_eq!(outcome.text, "答案");
    // 与不进 loop 的表现完全一致：请求里没有 tools
    assert!(seen.lock().unwrap()[0].tools.is_empty());
}

#[test]
fn 工具结果会回灌到下一轮() {
    let (provider, seen) = FakeProvider::new(vec![
        // 第一步：要一个工具
        vec![StreamEvent::ToolCall(crate::core::ai::tools::ToolCall {
            name: "read_note".into(),
            arguments: r#"{"path":"a.md"}"#.into(),
        })],
        // 第二步：给最终答案
        vec![StreamEvent::Delta("读完了".into()), StreamEvent::Done],
    ]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let outcome = run(
        &request(&Host, None, true, 8),
        &provider,
        Arc::new(AtomicBool::new(false)),
        &mut collect(&events),
    )
    .unwrap();

    assert_eq!(outcome.text, "读完了");
    assert_eq!(outcome.steps, 2);
    assert_eq!(outcome.tools, vec!["read_note".to_string()]);
    // 第二轮的消息里应带着工具结果
    let second = &seen.lock().unwrap()[1];
    assert!(second.messages.len() >= 2);
    assert!(
        second
            .messages
            .iter()
            .any(|m| m.content.contains("被读到的正文")),
        "工具结果没回灌：{:?}",
        second.messages
    );
    // 过程事件：调用 + 结果
    let events = events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, LoopEvent::ToolCall { name, .. } if name == "read_note"))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, LoopEvent::ToolResult { name, .. } if name == "read_note"))
    );
    // UI 要"展开看细节"，因此结果事件必须带上正文（不是只有行数 / 字符数）
    assert!(
        events.iter().any(|e| matches!(
            e,
            LoopEvent::ToolResult { content, .. } if content.contains("被读到的正文")
        )),
        "ToolResult 没带结果正文：{:?}",
        events
    );
}

#[test]
fn 步数用尽就放弃() {
    // 每一步都只要工具，永远不给答案
    let steps = vec![
        vec![StreamEvent::ToolCall(crate::core::ai::tools::ToolCall {
            name: "list_tags".into(),
            arguments: "{}".into(),
        })];
        // 复制 3 次
        3
    ];
    let steps: Vec<Step> = steps.into_iter().cycle().take(3).collect();
    let (provider, _) = FakeProvider::new(steps);
    let events = Arc::new(Mutex::new(Vec::new()));
    let err = run(
        &request(&Host, None, true, 3),
        &provider,
        Arc::new(AtomicBool::new(false)),
        &mut collect(&events),
    )
    .unwrap_err();
    assert_eq!(err, AiError::EmptyResult);
}

#[test]
fn 校验不过会回灌重试() {
    // 三次都输出"断链"文本：重试 MAX_VERIFY_RETRY 次后仍不过也照常返回
    let bad = "见 [[不存在]]\n";
    let steps = vec![
        vec![StreamEvent::Delta(bad.into()), StreamEvent::Done],
        vec![StreamEvent::Delta(bad.into()), StreamEvent::Done],
        vec![StreamEvent::Delta("这次没断链".into()), StreamEvent::Done],
    ];
    let (provider, seen) = FakeProvider::new(steps);
    let events = Arc::new(Mutex::new(Vec::new()));
    let outcome = run(
        &request(&Host, Some(&NoLinks), true, 8),
        &provider,
        Arc::new(AtomicBool::new(false)),
        &mut collect(&events),
    )
    .unwrap();

    // 前两轮回灌了问题，第三轮才通过
    assert_eq!(outcome.text, "这次没断链");
    assert!(seen.lock().unwrap().len() >= 2);
    assert!(
        seen.lock().unwrap()[1]
            .messages
            .iter()
            .any(|m| m.content.contains("结构问题")),
        "校验反馈没回灌：{:?}",
        seen.lock().unwrap()[1].messages
    );
    let retries = events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, LoopEvent::VerifyFailed(_)))
        .count();
    assert_eq!(retries as u8, MAX_VERIFY_RETRY);
}

#[test]
fn 取消后立即停下() {
    let (provider, _) = FakeProvider::new(vec![vec![
        StreamEvent::Delta("答案".into()),
        StreamEvent::Done,
    ]]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let err = run(
        &request(&Host, None, true, 8),
        &provider,
        Arc::new(AtomicBool::new(true)),
        &mut collect(&events),
    )
    .unwrap_err();
    assert_eq!(err, AiError::Canceled);
    assert!(events.lock().unwrap().is_empty(), "取消后不该再有事件");
}

#[test]
fn 正文为空算失败() {
    let (provider, _) = FakeProvider::new(vec![vec![StreamEvent::Done]]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let err = run(
        &request(&Host, None, true, 8),
        &provider,
        Arc::new(AtomicBool::new(false)),
        &mut collect(&events),
    )
    .unwrap_err();
    assert_eq!(err, AiError::EmptyResult);
}
