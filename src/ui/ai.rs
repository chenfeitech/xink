//! AI 面板（底部 dock）
//!
//! 设计见 `docs/ai/ai-assistant-design.md` §5.1 与 `docs/ai/ollama-provider-design.md` §4.5。
//! 三条硬约束：
//! 1. **模型可随时切换**：`model_override` 只对本次对话生效，不写配置；
//! 2. **网络全在后台**：provider 是阻塞式（ureq），跑在 `background_spawn`，
//!    事件经 flume 回到实体（与 `core/watcher.rs` 同一套写法）；
//! 3. **写盘必须经 diff**：AI 只产出文本，落盘前写快照，由用户点「接受」。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::{Button, ButtonVariants, DropdownButton};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::Sizable;
use gpui_kit::component::dock::{BasePanel, Panel, PanelEvent};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::*;

use crate::app::{AppState, config};
use crate::core::ai::context::{ContextScope, assemble};
use crate::core::ai::diff::{diff_lines, DiffLine};
use crate::core::ai::keychain::{MacKeychain, resolve_api_key};
use crate::core::ai::ollama::OllamaProvider;
use crate::core::ai::openai::OpenAiProvider;
use crate::core::ai::prompt::{Action, all_actions};
use crate::core::ai::snapshot::write_snapshot;
use crate::core::ai::{
    AiError, ChatMessage, CompletionRequest, Provider, StreamEvent, Usage,
};
use crate::core::config::AiProviderKind;
use crate::state::workspace::app_data_dir;

/// 等待首个响应字节的预算：Ollama 冷启动实测数秒
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(30);

/// 面板状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiStatus {
    Idle,
    Running,
    Done,
    Failed,
}

/// 一次动作的"作用对象"快照
///
/// 流式期间用户可能切标签、改内容，因此触发时就把原文与选区记下来，
/// 回填时按路径找回同一个标签，不做"当前是谁就用谁"的推测。
struct Target {
    path: PathBuf,
    /// 参与替换的原文（选中或整篇）
    before: String,
    /// 整篇原文（快照用）
    whole_before: String,
    whole: bool,
}

pub struct AiPanel {
    focus_handle: FocusHandle,
    status: AiStatus,
    action: Action,
    scope: ContextScope,
    target: Option<Target>,
    output: String,
    thinking: String,
    diff: Option<Vec<DiffLine>>,
    /// 本次对话用的模型；None 表示跟随设置里的默认模型
    model_override: Option<String>,
    error: Option<String>,
    notice: Option<String>,
    usage: Option<Usage>,
    elapsed_ms: Option<u64>,
    started: Option<Instant>,
    cancel: Arc<AtomicBool>,
    task: Option<Task<()>>,
}

impl AiPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 与 FilesPanel 同一套做法：自注册到 AppState，供模型下拉等处定位。
        // AppState 尚未注册时（理论上不会）跳过，绝不 panic。
        if cx.try_global::<AppState>().is_some() {
            let weak = cx.entity().downgrade();
            cx.update_global::<AppState, _>(|state, _| state.ai_panel = Some(weak));
        }
        Self {
            focus_handle: cx.focus_handle(),
            status: AiStatus::Idle,
            action: Action::Polish,
            scope: ContextScope::Selection,
            target: None,
            output: String::new(),
            thinking: String::new(),
            diff: None,
            model_override: None,
            error: None,
            notice: None,
            usage: None,
            elapsed_ms: None,
            started: None,
            cancel: Arc::new(AtomicBool::new(false)),
            task: None,
        }
    }

    /// 当前生效的模型 id（面板覆盖优先）
    fn model_id(&self, cx: &App) -> String {
        let cfg = config(cx);
        self.model_override
            .clone()
            .unwrap_or_else(|| cfg.ai.default_model.clone())
    }

    /// 触发一次动作
    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.action = action;
        self.output.clear();
        self.thinking.clear();
        self.diff = None;
        self.error = None;
        self.notice = None;
        self.usage = None;
        self.elapsed_ms = None;

        let cfg = config(cx);
        if !cfg.ai.enabled {
            self.error = Some(AiError::NotConfigured.message());
            self.status = AiStatus::Failed;
            cx.notify();
            return;
        }
        let Some(model) = cfg.ai.models.iter().find(|m| m.id == self.model_id(cx)) else {
            self.error = Some(AiError::NoModel.message());
            self.status = AiStatus::Failed;
            cx.notify();
            return;
        };

        let Some(state) = cx.try_global::<AppState>() else {
            self.error = Some("还没有打开库".into());
            self.status = AiStatus::Failed;
            cx.notify();
            return;
        };
        let Some(path) = state.tabs.active().cloned() else {
            self.error = Some("没有打开的笔记：先打开一篇笔记".into());
            self.status = AiStatus::Failed;
            cx.notify();
            return;
        };
        let Some(panel) = state.tabs.get(&path) else {
            self.error = Some("找不到活动笔记的编辑页".into());
            self.status = AiStatus::Failed;
            cx.notify();
            return;
        };

        // 取值：选中文本 + 整篇 + 光标
        let selection = panel.read(cx).selected_text(cx);
        let (whole_text, cursor) = panel.read(cx).note_text_and_cursor(cx).unwrap_or_default();
        let whole = matches!(self.scope, ContextScope::WholeNote) || selection.trim().is_empty();

        let prepared = assemble(
            self.scope,
            &selection,
            &whole_text,
            cursor,
            cfg.ai.max_context_chars,
        );
        self.notice = prepared.notice.clone();

        // 生成标签时把库内已有标签一起给模型，避免凭空造同义标签
        let known_tags: Vec<String> = state
            .notes
            .read(cx)
            .tags()
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        let messages: Vec<ChatMessage> =
            crate::core::ai::prompt::build_messages(action, &prepared.text, cfg.ai.output_language, &known_tags);

        let endpoint = cfg.ai.endpoint_for(model);
        let api_key = resolve_api_key(&cfg.ai, &model.id, &MacKeychain);
        let request = CompletionRequest {
            endpoint,
            model: model.id.clone(),
            messages,
            params: model.params.clone(),
            api_key,
            first_byte_timeout: FIRST_BYTE_TIMEOUT,
            silence_timeout: Duration::from_secs(cfg.ai.timeout_secs),
        };
        let provider: Arc<dyn Provider> = match model.kind {
            AiProviderKind::Ollama => Arc::new(OllamaProvider),
            AiProviderKind::OpenAiCompatible => Arc::new(OpenAiProvider),
        };

        self.target = Some(Target {
            path,
            before: if whole { whole_text.clone() } else { selection },
            whole_before: whole_text,
            whole,
        });
        self.cancel.store(false, Ordering::Relaxed);
        self.status = AiStatus::Running;
        self.started = Some(Instant::now());
        cx.notify();

        let (tx, rx) = flume::unbounded::<StreamEvent>();
        let cancel = self.cancel.clone();
        // provider 是阻塞的（同步 IO），必须放后台线程
        let task = cx.background_spawn(async move {
            let mut sink = |ev: StreamEvent| {
                let _ = tx.send(ev);
            };
            if let Err(err) = provider.stream(&request, cancel, &mut sink) {
                let _ = tx.send(StreamEvent::Failed(err));
            }
        });
        self.task = Some(task);

        cx.spawn_in(window, async move |this, window| {
            while let Ok(ev) = rx.recv_async().await {
                let finished = this
                    .update_in(window, |this, window, cx| this.on_event(ev, window, cx))
                    .unwrap_or(true);
                if finished {
                    break;
                }
            }
        })
        .detach();
    }

    /// 处理一个流事件；返回 true 表示本次请求已结束
    fn on_event(&mut self, ev: StreamEvent, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        match ev {
            StreamEvent::Delta(text) => {
                self.output.push_str(&text);
                cx.notify();
                false
            }
            StreamEvent::Thinking(text) => {
                self.thinking.push_str(&text);
                false
            }
            StreamEvent::Usage(usage) => {
                self.usage = Some(usage);
                false
            }
            StreamEvent::Done => {
                self.elapsed_ms = self.started.map(|t| t.elapsed().as_millis() as u64);
                self.task = None;
                if self.output.trim().is_empty() {
                    // thinking 模型把预算全花在思维链上时会这样
                    self.status = AiStatus::Failed;
                    self.error = Some(AiError::EmptyResult.message());
                } else {
                    let before = self.target.as_ref().map(|t| t.before.clone()).unwrap_or_default();
                    self.diff = Some(diff_lines(&before, &self.output));
                    self.status = AiStatus::Done;
                }
                cx.notify();
                true
            }
            StreamEvent::Failed(err) => {
                self.elapsed_ms = self.started.map(|t| t.elapsed().as_millis() as u64);
                self.task = None;
                self.status = AiStatus::Failed;
                self.error = Some(err.message());
                cx.notify();
                true
            }
        }
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.task = None;
        self.status = AiStatus::Idle;
        self.error = Some(AiError::Canceled.message());
        cx.notify();
    }

    /// 接受：写快照 → 回填编辑器 → 走现有保存链路
    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(target), Some(state)) = (self.target.take(), cx.try_global::<AppState>()) else {
            return;
        };
        let text = std::mem::take(&mut self.output);

        // 快照先写：写失败就不改内容，避免"改了却没退路"
        let dir = app_data_dir().join(crate::core::ai::snapshot::SNAPSHOT_DIR);
        let file_name = target
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "note.md".into());
        match write_snapshot(&dir, &file_name, &target.whole_before) {
            Ok(path) => self.notice = Some(format!("改前快照：{}", path.display())),
            Err(err) => {
                self.error = Some(format!("快照写入失败，已中止：{}", err.message()));
                self.status = AiStatus::Failed;
                self.target = Some(target);
                self.output = text;
                cx.notify();
                return;
            }
        }

        if let Some(panel) = state.tabs.get(&target.path) {
            panel.update(cx, |panel, cx| {
                panel.apply_ai_result(&text, target.whole, window, cx);
            });
        }

        self.diff = None;
        self.status = AiStatus::Idle;
        self.thinking.clear();
        cx.notify();
    }

    fn discard(&mut self, cx: &mut Context<Self>) {
        self.target = None;
        self.output.clear();
        self.thinking.clear();
        self.diff = None;
        self.status = AiStatus::Idle;
        self.error = None;
        cx.notify();
    }
}

impl BasePanel for AiPanel {
    fn panel_name(&self) -> &'static str {
        "AiPanel"
    }
}

impl Panel for AiPanel {
    fn title(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        "AI"
    }
}

impl EventEmitter<PanelEvent> for AiPanel {}

impl Focusable for AiPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AiPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border;
        let primary = theme.primary;
        let warning = theme.warning;
        let cfg = config(cx);
        let running = self.status == AiStatus::Running;
        let current_model = self.model_id(cx);

        // ── 顶栏：模型下拉 + 作用域 + 停止 ──
        let models: Vec<(String, String)> = cfg
            .ai
            .models
            .iter()
            .filter(|m| m.enabled)
            .map(|m| (m.id.clone(), m.display_name().to_string()))
            .collect();
        let models_for_menu = models.clone();
        let model_button = if models.is_empty() {
            div()
                .text_sm()
                .text_color(muted)
                .child("未配置模型（设置 · AI）")
                .into_any_element()
        } else {
            DropdownButton::new("ai-model")
                .button(
                    Button::new("ai-model-btn")
                        .small()
                        .outline()
                        .label(format!("模型：{current_model}")),
                )
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (id, label) in models_for_menu.iter() {
                        let id = id.clone();
                        menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                            move |_, _, cx| {
                                // 只改面板状态：本次对话用哪个模型，不落配置。
                                // AppState 持有 AI 面板的弱引用（与 files 同一套做法）。
                                let Some(state) = cx.try_global::<AppState>() else {
                                    return;
                                };
                                let Some(panel) = state.ai_panel.as_ref().and_then(|w| w.upgrade()) else {
                                    return;
                                };
                                let id = id.clone();
                                panel.update(cx, |panel, cx| {
                                    panel.model_override = Some(id);
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    menu
                })
                .into_any_element()
        };

        let mut header = div()
            .flex()
            .items_center()
            .gap_2()
            .p_2()
            .border_b_1()
            .border_color(border)
            .child(model_button)
            .child(
                Button::new("ai-scope")
                    .small()
                    .ghost()
                    .label(if self.scope == ContextScope::Selection {
                        "选中"
                    } else {
                        "整篇"
                    })
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.scope = if this.scope == ContextScope::Selection {
                            ContextScope::WholeNote
                        } else {
                            ContextScope::Selection
                        };
                        cx.notify();
                    })),
            );

        if running {
            header = header.child(
                Button::new("ai-stop")
                    .small()
                    .ghost()
                    .label("停止")
                    .on_click(cx.listener(|this, _, _window, cx| this.stop(cx))),
            );
        }

        // ── 动作按钮 ──
        let mut actions = div().flex().flex_wrap().gap_1().p_2();
        for action in all_actions() {
            let action = *action;
            let selected = self.action == action;
            let button = Button::new(SharedString::from(format!("ai-act-{}", action.id()))).small().label(action.label());
            let button = if selected { button.primary() } else { button.outline() };
            actions = actions.child(
                button.on_click(cx.listener(move |this, _, window, cx| this.run(action, window, cx))),
            );
        }

        // ── 主体 ──
        let mut body = div().size_full().overflow_y_scrollbar().p_3().flex().flex_col().gap_1();
        if let Some(diff) = &self.diff {
            for line in diff {
                let (text, color) = match line {
                    DiffLine::Same(t) => (format!("  {t}"), muted),
                    DiffLine::Add(t) => (format!("+ {t}"), primary),
                    DiffLine::Del(t) => (format!("- {t}"), warning),
                };
                body = body.child(div().text_sm().text_color(color).child(text));
            }
        } else if !self.output.is_empty() {
            body = body.child(div().text_sm().child(self.output.clone()));
        } else if running {
            body = body.child(div().text_sm().text_color(muted).child("正在生成…（首次请求可能要等模型加载）"));
        } else {
            body = body.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("选一段文字或整篇，点上面的动作；结果会先以 diff 呈现，确认后才写入笔记。"),
            );
        }

        // ── 底栏 ──
        let mut footer = div().flex().items_center().gap_2().p_2().border_t_1().border_color(border);
        if self.status == AiStatus::Done {
            footer = footer
                .child(
                    Button::new("ai-accept")
                        .small()
                        .primary()
                        .label("接受")
                        .on_click(cx.listener(|this, _, window, cx| this.accept(window, cx))),
                )
                .child(
                    Button::new("ai-discard")
                        .small()
                        .ghost()
                        .label("丢弃")
                        .on_click(cx.listener(|this, _, _window, cx| this.discard(cx))),
                );
        }
        if let Some(usage) = self.usage {
            if let Some(ms) = self.elapsed_ms {
                footer = footer.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{} ms", ms)),
                );
            }
            if let Some(total) = usage.completion_tokens {
                footer = footer.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("输出 {} tokens", total)),
                );
            }
        }
        if let Some(notice) = &self.notice {
            footer = footer.child(div().text_xs().text_color(muted).child(notice.clone()));
        }
        if let Some(err) = &self.error {
            footer = footer.child(div().text_xs().text_color(warning).child(err.clone()));
        }

        div()
            .id("ai-panel")
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(actions)
            .child(div().flex_1().min_h_0().child(body))
            .child(footer)
    }
}
