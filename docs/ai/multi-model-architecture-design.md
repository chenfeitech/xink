# 多模型可插拔架构

> 需求：把"单一模型"扩成可插拔多模型，至少接入 deepseek；明确注册与配置、
> 统一调用接口、能力差异兼容、错误与重试、按场景路由，并做到新增模型不改核心代码。
> 状态：**已拍板并实施完成**（P0–P4 全做完，2026-10-28）。
> 门禁：`cargo build` / `clippy --all-targets` 0 warning、`fmt --check` 干净、
> `cargo test` 615 passed / 0 failed。

## 1 现状查证（先纠正前提）

需求描述里"当前仅支持单一模型"**部分不成立**。准确说法是：

> **配置上支持多模型，路由上只支持单一模型，UI 上只暴露了 Ollama。**

### 已经有的

| 能力 | 证据 |
|---|---|
| 统一 `trait Provider`（`id()` + `stream()`） | `src/core/ai/mod.rs:214-223` |
| 两个实现：Ollama native（NDJSON）/ OpenAI 兼容（SSE） | `ollama.rs:116`、`openai.rs:38` |
| 多模型列表，条目可跨 provider、独立 `base_url` / `params` / `api_key_source` | `config.rs:681`、`ModelEntry` `config.rs:512-534` |
| 统一入参 `CompletionRequest` | `mod.rs:111-126` |
| 模型发现（`/api/tags`）+ 只更新能力字段、不动用户参数 | `models.rs:111-126` |
| 连接测试探针，结论写回 `tools_enabled`（"能力声明不可信"已落地） | `probe.rs:26-40`、`settings.rs:1910-1927` |
| 不支持工具的模型自动退化单轮 | `ui/ai.rs:812`、`agent_loop.rs:95-99` |
| 工具调用跨模型形状差异消化在 provider 内 | `ollama.rs:271-276`、`openai.rs:180-213` |
| 会话级模型切换下拉 | `ui/ai.rs:1324-1349` |

### 真正缺的（本次要补的）

| # | 缺口 | 证据 |
|---|---|---|
| G1 | 无 provider 注册表 / 工厂：加一个 provider 要改 3 处硬编码 `match`（枚举 / `endpoint_for` / 分派） | `config.rs:464`、`config.rs:832-838`、`ui/ai.rs:796-799` |
| G2 | 只有一个全局 `default_model`；无按场景路由；`AiActionConfig` 连 `model` 字段都没有 | `config.rs:680`、`config.rs:576-589`、`ui/ai.rs:779` |
| G3 | **UI 建不出非 Ollama 模型**：按钮硬编码"添加为 Ollama 模型"，新建条目 `kind` 恒 Ollama | `settings.rs:1966`、`:1974-1978` |
| G4 | 凭据：`Plain` 恒空、Keychain 无写入口、`resolve_api_key` 被 Ollama 全局短路 | `config.rs:854-857`、`keychain.rs:36-61`、`config.rs:841-846` |
| G5 | 无请求级重试 / 退避 / fallback | 全仓无 backoff |
| G6 | 上下文只按全局 `max_context_chars` 截字符，不看模型 `context_limit`；OpenAI 端点完全丢掉 `num_ctx` | `context.rs:50-81`、`openai.rs:222-228` |
| G7 | 能力探测只有 `tools`，无 thinking / vision / 实测上下文 | `ollama.rs:49-55` |

**G3 + G4 是 deepseek 接不进来的直接原因**——deepseek 是 OpenAI 兼容端点，
协议层早已够用，缺的是"能在界面上建出来并填上 key"。

## 2 目标 / 非目标

**目标**

- 新增一个**模型**：改配置即可，不动核心代码
- 新增一个 **provider**：加一个文件 + 注册一行，不动分派逻辑
- 统一调用接口规范（入参 / 出参 / 事件）写清楚
- 能力差异（流式、上下文、工具）有明确降级路径
- 错误可分类、可重试、可退避、可 fallback
- 支持按场景路由到不同模型

**非目标**

- 不做 embedding（P2，见 `ai-capabilities-eval.md:59`）
- 不做多用户（本地优先，`ai-assistant-requirements.md:60`）
- 不做模型计费 / 用量统计
- 不做 `Cli` provider（M4，无 `cli_events.rs`）

## 3 分层

```
路由层   Route: 场景 → 模型 id            ← 新增（G2）
   ↓
注册表   ProviderRegistry: kind → Provider ← 新增（G1）
   ↓
抽象层   trait Provider（已存在）
   ↓
实现层   OllamaProvider / OpenAiProvider / 未来新增
```

## 4 统一调用接口规范

沿用现有 `trait Provider`（`mod.rs:214-223`），不另起炉灶：

```rust
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn stream(&self, req: &CompletionRequest,
              cancel: Arc<AtomicBool>,
              on: &mut dyn FnMut(StreamEvent)) -> Result<(), AiError>;
}
```

| 项 | 规范 |
|---|---|
| 入参 | `CompletionRequest`（`mod.rs:111-126`）：endpoint / api_key / model / messages / tools / params / 两个超时 |
| 出参 | **不用返回值，用回调 `StreamEvent`**：增量 `Delta`、工具调用、结束、错误 |
| 取消 | `Arc<AtomicBool>`，provider 自己轮询 |
| 超时 | `first_byte_timeout`（等首字节）+ `silence_timeout`（等下一帧），两者语义不同，不得合并 |
| 错误 | 一律 `AiError`（`mod.rs:157-175`），HTTP 状态经 `classify_ureq_error`（`mod.rs:226-238`）转成语义错误 |
| 流式 | **两家都强制流式**（`openai.rs:224`、`ollama.rs:224`）；provider 内部消化 SSE / NDJSON 差异 |

**新增 provider 只需实现这一个方法**；`stream` 之外的能力（列模型、探针）保持模块级自由函数，
不进 trait——不是所有 provider 都有这些能力，塞进 trait 会逼着别人 `unimplemented!()`。

## 5 注册与配置

### 5.1 注册表（消掉 G1）

```rust
pub struct ProviderRegistry {
    builders: HashMap<AiProviderKind, Arc<dyn Fn() -> Arc<dyn Provider> + Send + Sync>>,
}
```

- 内置两个在启动时注册
- `resolve(kind) -> Arc<dyn Provider>` 取代 `ui/ai.rs:796-799` 的 `match`
- `endpoint_for` 同样从注册表取（或留在 `ModelEntry`，见下）
- 未知 kind → `AiError::UnsupportedProvider`，不是 panic

### 5.2 模型配置

`ModelEntry` 字段**不变**（已有 11 个字段够用，`config.rs:512-534`），新增的只有：

```rust
pub struct AiRouteConfig {
    /// 场景 → 模型 id；不配的回落到 default_model
    pub chat: Option<String>,
    pub action: Option<String>,     // 快捷动作
    pub skill: Option<String>,
    pub agent: Option<String>,      // 工具循环
}
```

`AiConfig` 增 `route: AiRouteConfig`（`#[serde(default)]`，旧配置自动补空 → 行为不变）。

**单个动作也可指定模型**：`AiActionConfig` 增 `model: Option<String>`，
优先级 `动作.model > route.action > default_model > model_override` 见 §8。

## 6 能力差异兼容

| 差异 | 处理 |
|---|---|
| 流式 | 两家都流式；不支持的 provider 必须一次 `Delta` 发完再 `Done`，对外表现一致 |
| 上下文长度 | 请求前按 `min(context_limit, params.num_ctx, 全局 max_context_chars)` 取最小者裁剪（修 G6） |
| `num_ctx` | OpenAI 兼容端点目前丢弃（`openai.rs:222-228`）；改为映射到 `max_tokens`，并在不支持时忽略 |
| 工具调用 | 判据是**探针结论** `tools_enabled`，不是能力声明字符串（`config.rs:771` 的 `supports_tools()` 是死代码，应删除）；不支持 → 不进 loop，走单轮 |
| thinking | 探针目前不测；`params.think` 只对 Ollama 有意义，其他 provider 忽略 |
| 视觉 | 非目标 |

## 7 错误与重试

| 层 | 策略 |
|---|---|
| 分类 | 沿用 `AiError` 12 变体 + `classify_ureq_error`（已有） |
| 可重试 | `RateLimit`(429) / `Timeout` / `Network`(IO) / 5xx → 重试；`Auth`(401/403) / `ModelNotFound`(404) / 请求体问题 → **不重试** |
| 退避 | 指数退避 + 抖动，上限 3 次（待拍板） |
| fallback | 重试耗尽后，若配了 `fallback_model` 则切备用模型（待拍板是否要做） |
| 超时 | `first_byte_timeout` 目前硬编码 30s（`ui/ai.rs:52`），改为读配置；`silence_timeout` 已读 `timeout_secs` |
| 呈现 | 失败时显示"重试了 N 次 + 最后一条错误"，不要只显示最后一次 |

## 8 路由

优先级（高 → 低）：

```
1. 会话级覆盖   model_override（已有，ui/ai.rs:451）
2. 动作指定     AiActionConfig.model（新增）
3. 场景路由     route.{chat|action|skill|agent}（新增）
4. 全局默认     default_model（已有）
```

**为什么会话覆盖优先级最高**：用户临时切了模型，说明他这次就要用这个，
不该被场景路由覆盖回去。

`AiPanel::model_id()`（`ui/ai.rs:531-536`）是唯一取模点，路由逻辑全部收在这里，
不散到 send / run_action / run_skill。

## 9 扩展边界

| 想做什么 | 改什么 | 是否动核心 |
|---|---|---|
| 加一个模型（如 deepseek） | 设置里新建条目：填 id / 选 kind=OpenAI 兼容 / 填 base_url / 填 key | **不动** |
| 删 / 停用 / 换默认 | 设置里点按钮 | **不动** |
| 加一个 provider（如 Claude 原生） | 新增 `src/core/ai/<name>.rs` 实现 `trait Provider` + 在注册表注册一行 + `AiProviderKind` 加一个变体 | 动 3 处，但**不动调用链** |
| 改路由策略 | 改 `AiRouteConfig` 配置 | **不动** |

**边界原则**：provider 只负责"把一个 `CompletionRequest` 变成一串 `StreamEvent`"。
业务逻辑（上下文裁剪、工具循环、diff、快照）一律不许进 provider。

## 10 分期

| 期 | 内容 | 解决 | 价值 |
|---|---|---|---|
| **P0** | 模型管理 UI 补全：能选 `kind`、填 `base_url`、选 key 来源并**写入** Keychain / 明文 | G3 + G4 | **deepseek 立刻可用**，不动架构 |
| P1 | Provider 注册表 + `endpoint_for` 收敛 | G1 | 加 provider 不改分派 |
| P2 | 路由层（场景 / 动作级）+ 取模点收敛 | G2 | 对话用便宜模型、润色用好模型 |
| P3 | 重试 / 退避 / fallback + `first_byte_timeout` 可配 | G5 | 网络抖动不再白等 |
| P4 | 上下文按 `context_limit` 裁剪 + `num_ctx` 映射 | G6 | 长文不再被静默截断 |

五期**全部做完**，已落地（见 §12 实现对照）。

## 11 决策记录（用户已拍板）

| # | 问题 | 结论 |
|---|---|---|
| Q1 | 分期怎么走 | **P0–P4 一次性全部做完**，不分批见效 |
| Q2 | 凭据怎么存 | **Keychain 读 + UI 可写入**为主，明文仅作备选（Keychain 不可用时兜底） |
| Q3 | `resolve_api_key` 被 default 模型短路的 bug | **一起修**：改成按"发起请求的那个模型"判断，与 default 无关 |

Q3 的修法直接影响 deepseek 能否用：原实现先看 default 模型是不是 Ollama，
是就直接返回 `None`，导致**所有云模型的 key 都被吞掉**。
现在签名改为 `resolve_api_key(model: &ModelEntry, store)`，
内部用 `AiConfig::needs_key(model)` 只判断入参模型本身。

## 12 实现对照

| 期 | 落地位置 | 关键改动 |
|---|---|---|
| P0 | `src/ui/settings.rs` | 新增模型可选 `kind`（Ollama / OpenAI 兼容）、OpenAI 兼容必填 `base_url`；每行模型可切 kind、可点「密钥」展开编辑面板：来源在 Keychain→Env→明文间轮换，Keychain 走 `MacKeychain::set/delete`（此前是死代码，没有写入入口），明文写 `ModelEntry::plain_key` |
| P1 | `src/core/ai/mod.rs`、`src/core/config.rs` | 新增 `ProviderFactory` / `ProviderRegistry` / 全局 `providers()`；`endpoint_for()` 改为查注册表，UI 层硬编码 `match` 删除，分派点从 3 处收敛到 1 处 |
| P2 | `src/core/config.rs`、`src/ui/ai.rs` | `AiScenario`（Chat/Action/Skill/Agent）+ `AiRouteConfig`；统一入口 `AiConfig::resolve_model()`，优先级：会话 override > 动作指定 > 场景路由 > `default_model`；设置面板新增「场景路由」分组；`sanitize_routing()` 清理悬空引用 |
| P3 | `src/core/ai/mod.rs`、`src/ui/ai.rs` | `is_retryable()` + `stream_with_failover()`（指数退避，只对"还没吐过 Delta/ToolCall"的请求重试，避免重复输出）；备用模型作为第二次尝试；`AiRetryConfig{max_attempts, backoff_ms}`、`first_byte_timeout_secs` 可配（原硬编码 30s） |
| P4 | `src/core/config.rs`、`src/ui/ai.rs` | `ModelEntry::context_chars()`（= `context_limit × 2`），`context_budget()` 取"全局上限 min 模型预算"，三处 `assemble()` 全部改用它 |
