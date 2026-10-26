# Ollama Provider 与模型列表管理 — 设计

> 状态：**待确认**（M0 Spike 已完成，数据为本机实测）
> 日期：2026-10-26
> 分支：`feat/ai-assist-research`
> 上游：[`ai-capabilities-eval.md`](./ai-capabilities-eval.md)、[`ai-assistant-design.md`](./ai-assistant-design.md)
> 实测环境：macOS，Ollama **0.40.11**，`ureq` **3.5.0**（rustls），本机 9 个模型

---

## 1. 结论先行

1. **Ollama 走 native `/api/chat`（NDJSON），不走 OpenAI 兼容端点。** 决定性证据：兼容端点**忽略 `think: false`**，
   thinking 模型会把全部生成预算花在 `reasoning` 上、正文为空（实测：正文 0 字 / 思考 356 字）；
   native 端点 `think:false` 生效（思考 0 字 / 正文 66 字）。
2. **工具调用可用**（agent loop 的前提成立）：`tool_calls[0].function.{name,arguments}` + `id`，
   结果以 `{"role":"tool","content":...}` 回灌即可收尾。3 个模型通过、1 个声明支持却返回 500 →
   **必须 per-model 连接测试 + 降级**。
3. **依赖代价可接受**：`ureq` 新增传递依赖共 52 行，其中 rustls / flate2 / base64 / percent-encoding /
   once_cell / log / libc / serde_json **均已在现有 `Cargo.lock` 中存在单份**，不会引入第二份。
   全新 release 编译 12.8s（大头是 `ring`）。
4. 模型列表用 `GET /api/tags` 拉取并**缓存**，配置里以「模型列表」管理，支持多实例 base_url 覆盖。

---

## 2. M0 Spike 实测数据

### 2.1 环境与方法

spike 位于 `/tmp/xink-ai-spike`（独立 crate，不进仓库），依赖仅 `ureq` + `serde_json`。
测试项：`/api/tags` 模型发现、native `/api/chat` 流式、`/v1/chat/completions` SSE 流式、
中途取消、`tools` 两轮调用。`ollama serve` 需监听 `127.0.0.1:11434`。

### 2.2 `/api/tags`（模型发现）

```
tev1:4b                ctx=262144  caps=[tools,thinking,completion]
tev1:0.8b              ctx=262144  caps=[tools,thinking,completion]
ornith-1.5:9b          ctx=262144  caps=[tools,thinking,completion,vision]
qwen3.8:27b            ctx=262144  caps=[completion,vision,tools,thinking]
qwen3.8:latest         ctx=262144  caps=[completion,vision,tools,thinking]
ornith:35b             ctx=262144  caps=[completion,tools,thinking]
qwen3:0.6b             ctx=40960   caps=[completion,tools,thinking]
qwen3.5:0.8b           ctx=262144  caps=[completion,vision,tools,thinking]
lfm2.5-thinking:latest ctx=128000  caps=[completion,tools,thinking]
```

可用字段：`name`、`size`、`details.context_length`、`capabilities`（`completion` / `tools` / `thinking` / `vision`）。
→ 这些字段直接驱动 UI：**`tools` 决定是否开工具阶段、`thinking` 决定是否显示「思考」开关、`context_length` 决定 `num_ctx` 上限**。

### 2.3 流式对比（模型 `qwen3:0.6b`）

| 端点 | 协议 | 首字节（冷） | 首 token（热） | 帧数 | 思考 | 正文 | `think:false` 是否生效 |
|---|---|---|---|---|---|---|---|
| `/api/chat` | NDJSON | 4.02s（含模型加载） | 0.60s | 35 | 0 字 | 66 字 | ✅ 生效 |
| `/v1/chat/completions` | SSE | — | 0.56s | 63 / 197 | 356 字 | **0 字** | ❌ 被忽略 |

其它观察：

- SSE 响应头 `content-type: text/event-stream`，帧格式标准（`data: {...}\n\n`，终帧 `data: [DONE]`），
  且 Ollama 把思维链放在 `choices[0].delta.reasoning`（不是 OpenAI 的 `reasoning_content`）。
- 小模型的 SSE chunk 极碎（197 帧 / 356 字，约 1 字一帧）→ UI 侧**必须做节流合并**（如 16ms 批量刷新），否则重绘开销大。
- `num_predict` 给太小（64）时，thinking 模型会把预算全花在思考上、一个正文字都不出。
  设计上：`num_predict` 默认给足（1024），并对「有正文输出但 0 字」的情况判为 `EmptyResult` 提示重跑。

### 2.4 取消

读到 1 帧后丢弃 reader（等价于用户点「停止」）：**24–26ms** 内连接关闭，无残留回调。
→ 设计里的「`AtomicBool` + 丢弃 reader」取消方案成立，不需要 kill 线程。

### 2.5 工具调用（native `/api/chat`，`stream:false`）

| 模型 | round 0（是否产出 tool_call） | round 1（回灌 tool 结果后） |
|---|---|---|
| `tev1:0.8b` | ✅ `list_tags`，`id` 有值 | ✅ 正文「人工智能,读书,GTD」 |
| `tev1:4b` | ✅ | ✅ |
| `qwen3:0.6b` | ✅ | ✅ |
| `qwen3.5:0.8b` | ❌ **HTTP 500** | — |

- 请求形态：`{"tools":[{ "type":"function","function":{name,description,parameters} }], "messages":[...], "think":false}`。
- 响应形态：`message.tool_calls[0].function = {name, arguments}`（`arguments` 是**对象**不是字符串）+ `id`。
- 回灌形态：`{"role":"tool","content":"..."}`（Ollama 接受字符串 content）。
- 实测 `prompt_eval_count` 145–346 → `num_ctx` 至少 8192 才不至于挤爆（Ollama 默认 num_ctx 偏小）。
- **声明 `tools` 能力 ≠ 真的能用**（`qwen3.5:0.8b` 500）。因此：连接测试必须带一次工具探针，
  失败则把该模型标记为「不支持工具」，loop 自动退回无工具单轮。

### 2.6 依赖与体积

| 项 | 值 |
|---|---|
| 新增直接依赖 | `ureq = { version = "3", features = ["rustls", "json"] }` |
| 传递依赖 | `cargo tree` 52 行；rustls / flate2 / base64 / percent-encoding / once_cell / log / libc / serde_json **均已在 `Cargo.lock` 中存在单份** |
| 全新 release 编译 | 12.8s（`ring` 0.17.14 是大头） |
| spike 二进制 | 2.98 MB（release，含 serde_json） |

---

## 3. Provider 设计

### 3.1 三种 provider

| kind | 端点 | 协议 | 解析模块 | 期 |
|---|---|---|---|---|
| `Ollama`（本地默认） | `{base}/api/chat` | NDJSON | `core/ai/ndjson.rs` | M1 |
| `OpenAiCompatible` | `{base}/chat/completions` | SSE | `core/ai/sse.rs` | M1 |
| `Cli`（M4） | 子进程 | JSONL | `core/ai/cli_events.rs` | M4 |

三者实现同一个阻塞式 trait（`ai-assistant-design.md` §3.3），统一产出 `StreamEvent`，
**解析互不共用**。`Ollama` 额外实现两个发现接口：`list_models()`（`GET /api/tags`）、`probe(model)`（连接测试）。

### 3.2 字段映射

| 语义 | Ollama native | OpenAI 兼容 |
|---|---|---|
| 正文增量 | `message.content` | `choices[0].delta.content` |
| 思维链 | `message.thinking` | `choices[0].delta.reasoning`（Ollama）/ `reasoning_content`（多数云厂商） |
| 结束 | `done: true` | `data: [DONE]` |
| 用量 | `prompt_eval_count` / `eval_count` / `eval_duration`（ns） | `usage`（位置各家不同） |
| 工具调用 | `message.tool_calls[].function.{name,arguments}`，`arguments` 为**对象** | `tool_calls[].function.arguments` 为**字符串**，需再解析一次 |

### 3.3 Ollama 专属参数（这正是走 native 的理由）

| 参数 | 默认 | 说明 |
|---|---|---|
| `think` | `false` | 关闭思考。改写在「思考」开关里，仅 `capabilities` 含 `thinking` 的模型显示 |
| `keep_alive` | `"5m"` | 控制模型常驻显存。冷启动实测 4.0s（0.6b 首次加载），设 `-1` 常驻、`0` 用完即卸 |
| `num_ctx` | `8192` | 必须 ≤ 模型 `context_length`；超出时夹到上限并在设置里提示 |
| `num_predict` | `1024` | 防「思考吃掉预算」 |
| `options.temperature` | 由 `ModelParams` 提供 | — |

---

## 4. 模型列表管理

### 4.1 数据模型（`src/core/config.rs`）

```rust
pub struct AiConfig {
    pub enabled: bool,                  // 默认 false
    pub default_model: Option<String>,  // 模型 id，形如 "ollama:qwen3:0.6b"
    pub models: Vec<ModelEntry>,        // 模型列表（可多个、可跨 provider）
    pub ollama: OllamaConfig,           // Ollama 默认连接参数
    // …… 其余字段（timeout / max_context_chars / max_steps / output_language /
    //     auto_apply / skills_enabled / disabled_skills / memory_enabled）不变
}

pub struct OllamaConfig {
    pub base_url: String,     // 默认 "http://localhost:11434"
    pub keep_alive: String,   // 默认 "5m"
    pub num_ctx: u32,         // 默认 8192
}

pub struct ModelEntry {
    pub id: String,                    // "qwen3:0.6b"
    pub kind: AiProviderKind,          // Ollama | OpenAiCompatible
    pub label: Option<String>,         // 展示名，空则用 id
    pub base_url: Option<String>,      // 覆盖 provider 默认（多实例 / 远端 Ollama）
    pub api_key_source: KeySource,     // Ollama 忽略；云厂商用
    pub params: ModelParams,           // num_ctx / num_predict / temperature / think
    pub context_limit: Option<u32>,    // 来自 /api/tags，可手改
    pub capabilities: Vec<String>,     // 拉取时写入：completion/tools/thinking/vision
    pub tools_enabled: bool,           // 连接测试通过才默认 true
    pub enabled: bool,                 // 列表里停用
    pub last_checked: Option<String>,  // 上次连接测试时间（ISO8601）
}
```

**迁移**：AI 配置从未发布过，历史 `config.json` 里没有 `ai` 段，因此**不需要迁移逻辑**；
`#[serde(default)]` + `sanitize()` 已足够（夹取 `num_ctx`、越界 `temperature`、空 id 丢弃）。

### 4.2 发现与缓存

1. 设置页「AI → 模型」点「从 Ollama 拉取」→ 后台 `GET {base_url}/api/tags`。
2. 结果与时间戳写入 `~/Library/Application Support/xink/ollama-models.json`
   （`{ "fetched_at": "...", "base_url": "...", "models": [...] }`）。
   目的：离线/服务未启动时，列表与 `context_length`、`capabilities` 仍可展示。
3. 与已配置列表合并：已存在同名 id → 只更新 `context_limit` / `capabilities`，**不覆盖用户参数**；
   新增模型默认 `enabled=true`、`tools_enabled=false`（待连接测试）。

### 4.3 连接测试（每个模型一次）

`POST /api/chat`，`stream:false`、`num_predict:1`、一条最小消息；若该模型声称支持 tools，
再发一次带工具探针的请求。记录：成功/失败、首 token 延迟、是否支持 tools、是否支持 think。
失败原因分类与提示：

| 结果 | 处理 |
|---|---|
| 200 且有正文 | 标记可用；`tools_enabled` 按探针结果置位 |
| 模型不存在（404） | 提示「模型未拉取，请先 `ollama pull <id>`」 |
| 工具探针 500 / 无 tool_call | `tools_enabled=false`，提示「该模型不支持工具，将退回单轮模式」 |
| 连接失败 | 提示「Ollama 未启动或 base_url 不对」 |

### 4.4 设置界面（元数据驱动，沿用 `src/ui/settings.rs`）

「AI」分类下新增分组「模型」：

- 表格列：模型 / 类型 / 上下文 / 能力 / 默认 / 启用 / 操作（编辑、测试、删除）。
- 按钮：「从 Ollama 拉取」「新增（手动填 OpenAI 兼容端点）」「设为默认」。
- 单模型编辑项：`num_ctx`（上限提示取 `context_limit`）、`num_predict`、`temperature`、
  `think`（仅 thinking 模型显示）、`tools_enabled`、`keep_alive`（Ollama）。
- AI 面板顶部提供**模型快速切换**（下拉），切换后本次会话立即生效。

### 4.5 运行期行为

- 请求前解析 `default_model` → 找到 `ModelEntry` → 组装请求。未选则禁止发起并提示「未选择模型」。
- **同时只允许一个 AI 任务**（Ollama 单实例串行，且便于取消）；第二个请求提示「有任务进行中」。
- 冷启动可能数秒：面板显示「模型加载中（首次请求）」；`keep_alive` 默认 `5m` 覆盖连续操作。
- 流式渲染节流（16ms 批量刷新），应对小模型逐字 chunk。
- 工具阶段：模型 `tools_enabled=false` 时跳过工具循环，直接单轮；工具调用失败重试 ≤2 次后降级。

---

## 5. 非目标

- Ollama embedding（`/api/embed`）：P2，与向量检索一起评估。
- 多模态（vision 能力）：P2，当前编辑与预览链路不含图片输入。
- 用 Ollama 跑「模型管理」本身（pull / delete / cp）：提供跳转命令提示即可，不做。

---

## 6. 复核清单（Spike 结论要落到代码时逐条确认）

1. `think:false` 在当前 Ollama 版本下对 native 端点生效、对兼容端点无效 → 已实测，写成单测断言的两条基线。
2. 工具调用 `arguments` 在 Ollama 是对象、在 OpenAI 兼容是字符串 → 两处解析分开，别共用。
3. `qwen3.5:0.8b` 工具探针 500 → 记录为「能力声明不可信」的回归用例（用 fake provider 模拟 500 走降级分支）。
4. 冷启动 4s 量级 → UI 必须有「加载中」状态与取消入口。
5. `ring` 编译 12.8s → 可接受；若将来要砍，可评估 `native-tls` 走系统 Security.framework。
