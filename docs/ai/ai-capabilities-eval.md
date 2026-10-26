# AI 协助能力：调研与技术选型

> 类型：技术调研（`eval`）
> 日期：2026-10-26
> 分支：`feat/ai-assist-research`
> 状态：**调研结论待确认，尚未写生产代码**（规则 05：确认后才动 `src/`）
> 配套：[`ai-assistant-requirements.md`](./ai-assistant-requirements.md)（功能清单）
> 下一步：确认 §9「需拍板的问题」→ 出 `ai-assistant-design.md` → 开发

---

## 1. 结论先行

| 问题 | 结论 |
|---|---|
| 能不能做 | 能。缺的不是框架能力，是**网络栈**：应用现在一个 HTTP 请求都发不出去 |
| 走哪条路 | **Provider 抽象 + 两个实现**：P0 只做 OpenAI 兼容 HTTP，P1 再加 CLI Agent（codex / claude） |
| 加什么依赖 | `ureq` 3.x（阻塞式，MIT/Apache-2.0），跑在 `background_spawn` 后台线程。不引入 tokio / async runtime |
| Ollama 怎么接 | **走 native `/api/chat`**（`think` / `keep_alive` / `num_ctx` 可控 + `/api/tags` 发现）；兼容端点留给云厂商。实测数据见 [`ollama-provider-design.md`](./ollama-provider-design.md) §1–§2 |
| Skills 怎么做 | 采用 **Agent Skills 的 `SKILL.md` 格式**（frontmatter `name` + `description`），由 xink 自己发现与注入，不依赖 CLI 的自动发现 |
| 检索怎么做 | P0 **不做向量库**。用现有的全文检索 + 索引（双链 / 标签 / 标题）做「检索式上下文」 |
| AI 能不能直接改文件 | P0 **不能**。一律走 diff 预览 → 用户接受 → 复用现有原子写盘 |

推荐分期：**M0 Spike（验证网络与流式）→ M1 单篇动作 → M2 Skills + 命令面板 → M3 库级管理 → M4 会话面板 + CLI Provider**。
MCP 客户端、本地 embedding 检索列入**非目标**（P2，理由见 §4.4、§7.3）。

---

## 2. 现状与问题（附代码证据）

### 2.1 应用当前没有可用的 HTTP 客户端

这是本次调研最重要的发现，直接决定依赖选型。

| 事实 | 证据 |
|---|---|
| gpui 自带 `HttpClient` trait 与 `cx.http_client()` | `gpui-pre-0.3.9/src/app.rs:1944`；`gpui.rs:188` re-export `http_client` |
| **macOS 上默认注入的是空实现**：`Application::with_platform` 传 `NullHttpClient` | `gpui-pre-0.3.9/src/app.rs:180-185` |
| 只有 wasm 分支调用了 `with_http_client(...)`，桌面分支没调 | `gpui-pre-platform-0.3.9/src/gpui_platform.rs:17-22`（`application()`）、`:46`、`:54`（wasm 专用） |
| gpui-kit 0.8 没有暴露任何 http feature，也没替我们注入 | `gpui-kit-0.8.0/Cargo.toml` 的 `[features]` 全表无 http 项 |

结论：**`cx.http_client()` 现在发请求必然失败**，想用它就得自己 `impl HttpClient` 再 `cx.set_http_client()`，
而 trait 的 `send()` 返回 `BoxFuture`，用阻塞客户端实现等于把同步 IO 包进 future，两头别扭。
因此选更直接的路线：在 `core/ai/` 里直接用阻塞式 HTTP，跑在后台线程，按项目已有的
「后台线程 + flume 通道 → UI」模式回传（同 `core/watcher.rs` 的做法）。

代价：拿不到「框架统一的 http 层」（代理、UA 等）。这些对笔记应用影响很小，且可在 `core/ai/` 里自己读环境变量。

### 2.2 已具备、可直接复用的能力

| 能力 | 位置 | AI 功能怎么用 |
|---|---|---|
| 全局状态 `AppState`（`dock_area` / `vaults` / `notes`） | `src/app.rs:79-110` | 拿当前库、当前笔记、已打开标签 |
| 配置模型（每字段 `#[serde(default)]` + `sanitize()`） | `src/core/config.rs:345` | 新增 `AiConfig`，旧配置不失效 |
| 设置界面**元数据驱动**（加字段 = 加一行描述表） | `src/ui/settings.rs:26` 起 | 新增「AI」分类成本低 |
| 选中区替换：`EditorState::selected_range/selected_text/replace/insert` | `src/ui/editor.rs:2330-2350`（`surround`）、`:2364`（`block`） | AI 结果落回编辑区的现成入口 |
| 命令面板（`CommandState` + 自算模糊匹配） | `src/ui/command.rs:29-100` | AI 命令与 Skills 的统一入口 |
| 全文检索 `search_files` | `src/core/search.rs:34` | 「检索式上下文」的召回层 |
| 索引（标题 / 双链 / 标签 / 反链） | `src/core/index.rs` | 结构化上下文、相关笔记推荐 |
| 后台任务 + 不阻塞 UI | `src/ui/command.rs:88`（`background_spawn`） | 网络与解析都放后台 |
| 原子写盘 `atomic_write` | `src/core/note.rs` | AI 结果落盘 / 快照备份复用 |
| 应用数据目录 `~/Library/Application Support/xink` | `src/state/workspace.rs:18-34` | AI 配置、历史、全局 skills |

### 2.3 缺的东西

| 缺口 | 影响 | 处理 |
|---|---|---|
| 没有任何撤销栈（保存即落盘） | AI 误改无法回退 | 改前写快照 + diff 预览双保险（§8） |
| 编辑区没有「内嵌流式渲染」的位置 | 增量文本往哪显示 | P0 显示在独立结果面板，接受后一次性写入；**不做逐字写入编辑器** |
| 无 HTTP 依赖 | — | 新增 `ureq`（§5） |
| 无 YAML 解析依赖 | 解析 `SKILL.md` frontmatter | 已有 `noyalib`（数据文件编辑），直接复用，不加依赖 |

---

## 3. 能力地图（功能梳理的骨架）

按「**作用范围** × **操作类型**」展开，需求文档里的 A1–A9 由此派生：

|  | 生成 | 改写 | 加工 | 管理 |
|---|---|---|---|---|
| **选中文本** | 就地扩写 | 润色 / 精简 / 改语气 / 翻译 | 提取待办、转成表格 | — |
| **整篇笔记** | 续写、从大纲成文 | 全文重构、统一术语 | 生成摘要 / 标题 / 标签、提取双链 | 拆分 / 合并建议 |
| **多篇（用户勾选）** | 合并成综述 | 批量格式统一 | 批量打标签、生成 MOC 索引页 | 重复笔记检测 |
| **全库** | 周报 / 月度回顾 | — | 标签体系梳理（合并同义标签） | 孤儿笔记、断链、命名规范检查 |

「管理」类与现有索引能力高度重合：**断链、孤儿、标签统计这些用 `core/index.rs` 现算即可，不必问模型**；
模型只负责「需要语义判断」的部分（同义标签合并、命名建议、综述生成）。

---

## 4. Provider 方案对比

### 4.1 方案 A：OpenAI 兼容 HTTP（**推荐，P0**）

`POST {base_url}/chat/completions` + `stream: true`，SSE 增量读 `choices[*].delta.content`。

- 优点：协议稳定、能拿到逐块增量（UI 可以「边出边显示」）、可单测（mock 一段 SSE 字节流即可）、
  换供应商只改 `base_url` + `model` + key（DeepSeek / Kimi / 智谱 / 百炼 / Ollama / vLLM 都在这条线上）。
- 缺点：要用户自备 API key；只有文本能力，没有工具/文件操作能力。
- 协议差异（调研确认，实现时必须容错）：
  - 终帧 `data: [DONE]`；OpenAI 在 `include_usage` 时可能发 `choices: []` 的 usage 帧，**不能无脑取 `choices[0]`**。
  - DeepSeek 的 usage 附在最后一个带 `finish_reason` 的内容帧上，不发独立 usage 帧。
  - 推理模型会带 `reasoning_content`（思维链），正文仍是 `content`。
  - 智谱端点不是 `/v1/chat/completions` 而是 `/api/paas/v4/chat/completions`，base_url 必须可整体配置。
  - **本机实测（Ollama 0.40.11，`ureq` 3.5.0）**：`/v1/chat/completions` 的 SSE 正常（`content-type: text/event-stream`、
    终帧 `[DONE]`），但**忽略 `think:false`**——thinking 模型把预算全吐在 `delta.reasoning` 上、正文 0 字；
    `num_predict` 给小（64）时尤其致命。native `/api/chat` 的 `think` 生效（思考 0 字 / 正文 66 字），
    且带 `keep_alive` / `num_ctx` 与 `/api/tags` 发现。→ **Ollama 单列一个 provider，走 native**。

### 4.2 方案 B：CLI Agent 子进程（**推荐，P1**）

spawn `codex exec --json` 或 `claude -p --output-format stream-json`，解析 JSONL 事件。

- 优点：**不用 API key**（复用用户已有的订阅）；天然带工具能力（读写库文件、跑脚本）；
  Skills 生态可以直接借力。
- 缺点：事件格式随 CLI 版本漂移（`codex` 用 `item.completed` 里的 `agent_message`；
  `claude` 用 `result` / `stream_event` 的 `content_block_delta`）；启动慢（秒级）；
  权限与沙箱参数（`--sandbox workspace-write`、`--dangerously-skip-permissions`）语义敏感，
  交给 AI 写盘前必须由 xink 自己做确认层。
- 定位：作为**第二个 provider** 接在同一个抽象后面，事件解析用独立适配器隔离，绝不共用解析器。

### 4.3 方案 C：只做「粘贴式」半自动（被否决）

把组装好的 prompt 复制到剪贴板，用户自己去网页/客户端里问，再粘回来。

- 否决理由：体验断层，且**没有增量输出**，与我们想要的「选中 → 一键 → 看 diff」差距太大。
  但可作为 M0 的降级兜底保留一个「复制 prompt」命令，成本几乎为零。

### 4.4 方案 D：MCP 客户端（`rmcp`，**P2 非目标**）

- 否决理由：`rmcp` 是 Tokio async 栈，会引入与 gpui 执行器并存的第二个运行时；
  且它解决的是「给模型接外部工具」，而我们 P0–P1 的工具需求（读笔记、写笔记、检索）
  **xink 自己就是宿主**，直接函数调用比走 MCP 更短也更可控。
- 重新评估的触发条件：用户明确要接外部 MCP server（如公司知识库、Jira）。

### 4.5 方案 E：内置本地小模型（**非目标**）

体积、内存、质量都不适合作为通用写作助手；本地场景用 Ollama 走 **native `/api/chat`** 而非方案 A 的兼容端点（理由见下节与 `ollama-provider-design.md` §2.3）。

---

## 5. 依赖选型

| 候选 | 版本 / 许可 | 结论 |
|---|---|---|
| **`ureq`** | 3.5.0，MIT OR Apache-2.0 | **选它**。阻塞式，无 async runtime；`response.body_mut().as_reader()` 拿到 `Read` 后可逐块读 SSE |
| `reqwest`（含 gpui 树里的 `gpui-pre-reqwest` 0.12.15） | 0.12.x | 否决：绑 Tokio 运行时，与 gpui 自带的执行器并存；且 `gpui-pre-reqwest` 只是 `gpui-kit-assets` 在 wasm 分支的传递依赖（Cargo.lock:2217），macOS 根本没编译它 |
| `rmcp` | 3.7.0 | 见 §4.4，P2 |
| `fastembed` / `candle` | 7.5.0 / 0.11.0 | 见 §7.3，P2。且 `fastembed` 默认走 ONNX Runtime，不是纯 Rust 栈 |
| `noyalib`（已有） | 0.0.58 | 复用，解析 skill frontmatter，不加依赖 |

`ureq` 需要的 features：`rustls`（TLS）+ `json`（可选）。预计新增编译时间 < 1 分钟级，
不改变现有「只依赖 gpui-kit」的铁律（它是独立的基础库，不碰 gpui 版本）。

**线程模型的硬约束**：`ureq` 是阻塞 IO，**绝不能在主线程调用**（规则 02 §6）。
一律走 `cx.background_spawn(...)` + `flume` 通道把增量送回 UI 实体，与 `core/watcher.rs` 同一套写法。

---

## 6. Skills

### 6.1 采用 Agent Skills 的 `SKILL.md` 格式

规范出处：<https://agentskills.io/specification>。要点：

- 一个 skill = 一个目录 + 目录内 `SKILL.md`，文件以 YAML frontmatter 开头。
- 必填：`name`（1–64 字符，小写字母数字连字符，须与父目录名一致）、`description`（1–1024 字符，**同时说明做什么、何时用**）。
- 可选：`license`、`compatibility`、`metadata`（如 `author` / `version`）、`allowed-tools`（规范标为 experimental）。
- 渐进式披露三层：① 只有 `name` + `description` 进入模型上下文（约 50–100 tokens/个）；
  ② 命中后加载 `SKILL.md` 正文；③ 执行中按需读 `scripts/` `references/` `assets/`。
  建议正文 < 500 行 / 5k tokens。

### 6.2 xink 的存放位置（不依赖任何 CLI 的自动发现）

| 作用域 | 路径 |
|---|---|
| 库内（随库走，可同步/共享） | `<vault>/.xink/skills/<name>/SKILL.md` |
| 用户级（所有库通用） | `~/Library/Application Support/xink/skills/<name>/SKILL.md` |

**关键设计决定：无论底层 provider 是 HTTP 还是 CLI，skills 一律由 xink 自己发现、自己把正文注入 prompt。**
理由：Codex / Claude / Cursor 各自的发现路径不同（`.agents/skills/`、`.claude/skills/`、`.cursor/skills/`…），
把行为押在某家 CLI 的自动发现上，等于把功能可用性交给外部版本；自己注入才能保证
「同一个 skill 在两个 provider 下行为一致」，也才好在设置界面里列出、启停、排序。

注入方式：system prompt 里放 skill 清单（name + description），用户选中某个 skill 后，
把该 skill 的正文作为 system 追加段 + 用户指令一起发出。

---

## 7. 上下文装配与检索

### 7.1 三档上下文（P0）

1. **选中** → 只发选中文本（+ 前后各一段做语境，可配）。
2. **整篇** → 全文（超过 `max_context_chars` 则截断到光标附近，并在 prompt 里声明「这是节选」）。
3. **多篇 / 全库** → 先用 `core/search.rs::search_files` 与 `core/index.rs` 召回候选，
   再按字符预算拼装；每条附 `路径 + 标题`，让模型输出可引用的 `[[双链]]`。

### 7.2 Token 预算

不做精确 tokenizer。按字符粗估（中文按 1 字 ≈ 1 token、西文 4 字符 ≈ 1 token 保守折算），
上限由配置项 `ai.max_context_chars` 控制（建议默认 12_000 字符），超限时**先截断并提示**，
不静默丢内容。

### 7.3 为什么 P0 不做向量检索

- 成本：embedding 模型要下载权重（`fastembed` 默认 ONNX，非纯 Rust）、要建索引、要持久化，
  对一个「个人笔记库 + 关键词 + 双链图」的场景，收益不覆盖复杂度。
- 现有资产够用：全文检索 + 标签 + 双链 + 标题，已能组装出质量不错的上下文。
- 触发再评估的条件：库规模到数千篇、且用户反馈「找不到该带的笔记」。

---

## 8. 写入安全与可撤销

项目铁律是「磁盘是真值」（规则 04），AI 一旦会写盘就是最大的风险面，所以 P0 立三条：

1. **默认不写盘**：所有会产生修改的动作，先出「diff 预览面板」，用户显式「接受」才写入。
   写入复用 `core/note.rs` 的 `atomic_write`，不新开写路径。
2. **改前快照**：接受写入前，把原文写到
   `~/Library/Application Support/xink/ai-history/<yyyy-mm-dd-HHMMSS>-<文件名>`，
   面板上明确写出这个路径，用户可自行取回（不自建撤销栈，避免与自动保存/外部修改检测打架）。
3. **批量操作的二次确认**：库级动作先产出**只读的执行计划**（每篇：改什么、改成什么），
   用户逐条勾选后再执行，且同样走快照 + diff 落地。

网络侧还有一条：**AI 默认关闭**。没配 provider 时，命令面板里的 AI 命令显示为「未配置」并直达设置页，
绝不静默发请求。

---

## 9. 需拍板的问题（请逐项确认后再进设计文档）

| # | 问题 | 建议 | 影响 |
|---|---|---|---|
| Q1 | API key 存哪 | 默认 **macOS Keychain**（`security` 命令行或 `keychain` crate），`config.json` 只存「从哪取」的引用；提供「明文存入 config.json」的可选开关 | 决定 `AiConfig` 的数据模型 |
| Q2 | P0 是否包含 CLI Agent provider | 建议 **否**（放 P1）。先验证 HTTP 链路，避免同时啃两个不稳定的事件格式 | 决定 M1 的范围与工期 |
| Q3 | P0 是否要多轮会话面板 | 建议 **否**。P0 只做一次性动作（选中/整篇 → diff）；会话面板放 M4 | 是否需要会话存储与上下文管理 |
| Q4 | 默认内置哪些动作 | 建议 8 个：润色、扩写、精简、翻译、改标题、生成摘要、生成标签、续写 | 内置 prompt 集合的规模 |
| Q5 | 输出语言默认 | 建议 **跟随笔记正文**（中文笔记用中文答），可在设置里改成固定语言 | prompt 组装 |
| Q6 | 是否允许 AI 自动落盘（跳过 diff） | 建议 **不允许**，至少 M1–M3 不允许；后续可作为高级开关 | 写入安全模型 |
| Q7 | Skills 是否 P1 就做 | 建议 **做**（M2）。成本主要在发现与注入，不在 provider | 是否要 skill 解析与优先级规则 |

---

## 10. 建议分期（每期可独立验证）

| 期 | 内容 | 验收方式 |
|---|---|---|
| **M0 Spike** | ✅ **已完成（2026-10-26）**：`ureq` 3.5.0 + rustls 全新 release 编译 12.8s；Ollama native NDJSON 与 `/v1` SSE 双协议跑通；取消 26ms 内关闭连接；工具调用 3/4 模型通过（1 个声明支持却返回 500）。数据见 [`ollama-provider-design.md`](./ollama-provider-design.md) §2 | 独立 crate（`/tmp/xink-ai-spike`，未进仓库）；结论已回写本文档与 `ai-assistant-design.md` |
| **M1** | `core/ai/`（client / sse / prompt / diff）+ 设置页 AI 分类 + 8 个内置动作 + diff 预览 + 快照 | 单测覆盖 SSE 解析与 diff；人工跑通「选中 → 润色 → 接受」 |
| **M2** | Skills 发现与注入 + 命令面板 `AI:` 前缀 + 自定义动作 | 库内放 2 个样例 skill，确认被列出、能生效 |
| **M3** | 库级管理动作（批量标签、MOC 生成、断链/孤儿检查报告） | 计划面板逐条勾选后执行，快照可回退 |
| **M4** | 多轮会话面板 + CLI Agent provider（codex / claude） | 两个 provider 切换后同一 skill 行为一致 |
| **P2** | MCP 客户端、本地 embedding 检索 | 触发条件见 §4.4、§7.3 |

M0 是 Spike 性质（规则 05 §1 允许），M1 起必须等本文档与需求文档确认、且设计文档就位后才动 `src/`。

---

## 11. 调研来源

- Agent Skills 规范：<https://agentskills.io/specification>、客户端实施指南 <https://agentskills.io/client-implementation/adding-skills-support>
- Codex 非交互模式（`codex exec --json` 事件）：<https://learn.chatgpt.com/docs/non-interactive-mode>
- Claude Code headless（`claude -p --output-format stream-json`）：<https://code.claude.com/docs/en/headless>、CLI 参考 <https://code.claude.com/docs/en/cli-reference>
- OpenAI Chat Completions 流事件：<https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events>
- 兼容性差异：DeepSeek <https://api-docs.deepseek.com/api/create-chat-completion/>、Kimi <https://platform.kimi.com/docs/api/chat>、
  智谱 <https://docs.bigmodel.cn/api-reference/模型-api/对话补全>、百炼 <https://help.aliyun.com/en/model-studio/compatibility-of-openai-with-dashscope>、
  Ollama <https://docs.ollama.com/api/openai-compatibility>、vLLM <https://docs.vllm.ai/en/latest/serving/online_serving/openai_compatible_server/>
- 依赖：`ureq` <https://docs.rs/crate/ureq/latest>、`rmcp` <https://docs.rs/crate/rmcp/latest>、`fastembed` <https://docs.rs/crate/fastembed/latest>

> 说明：CLI 参数与各家响应细节来自公开文档，未在本机实测（`codex` / `claude` 未确认安装与版本）。
> 进入 M4 前必须固定版本采集一次真实 JSONL 样例，本文档不把这些当作兼容性承诺。
