# AI 协助：是否需要自建 Agent Harness 与记忆机制

> 类型：架构调研（`eval`）
> 日期：2026-10-26
> 分支：`feat/ai-assist-research`
> 状态：**分析结论，待确认**（不改 `src/`，不改既有两份 AI 文档的结论，只扩展）
> 相关：[`ai-capabilities-eval.md`](./ai-capabilities-eval.md)、[`ai-assistant-requirements.md`](./ai-assistant-requirements.md)

---

## 1. 结论先行

**是，需要的不是「调模型」，而是一套属于自己的 harness + 记忆机制。** 但形态要克制：

| 判断 | 结论 |
|---|---|
| 要不要 harness | **要**。哪怕只是一个「润色选中文本」，底层也应是同一个 agent loop，否则后面加工具要重写 |
| 要不要多步工具调用 | **要**，但 P0 只开**只读工具**（读笔记 / 检索 / 标签 / 双链），写操作一律以 `propose_edit` 出 diff |
| 要不要长任务（跨上下文窗口自举） | **要，但只用于库级任务**（整理标签体系、生成 MOC）。单篇动作保持单轮 |
| 要不要记忆 | **要，但是「文件化 + 人在环」**，不是接一个向量数据库服务 |
| 要不要 autonomous 长跑 agent | **不要**。xink 的价值观是「磁盘是真值 + 显式确认」，无人值守改盘与它冲突 |

一句话定位：**xink 的 AI 不是聊天框，是一个「本地文档领域的专业 agent」**——它的专业性来自
三件别处拿不到的东西：库的结构化事实（索引/双链/标签）、受控的写权限、以及可审阅的记忆。

---

## 2. 为什么不是「调一次大模型」就够

| 理由 | 在 xink 里的具体表现 |
|---|---|
| 库级任务天然是多步的 | 「整理标签体系」= 枚举标签 → 聚类同义 → 决定规范名 → 逐篇改写 → 校验双链不断。一次 completion 做不完也做不稳 |
| 模型不知道库里有什么 | 必须给它**工具**去查（`search_files`、`index.tags()`、`backlinks`），而不是把全库塞进上下文 |
| 输出必须可验证 | 生成 `[[双链]]` 后要检查目标存在、生成 frontmatter 要检查可解析、改标签要检查不与既有体系冲突。**没有验证环的 agent 会静默产垃圾** |
| 上下文是有限资源 | 长会话会退化（context rot）；工具结果堆积会挤爆窗口。需要 compaction / tool-result clearing / just-in-time 加载 |
| 任务要能中断与恢复 | 用户随时关窗口、网络随时断。库级任务必须能从「已完成 87 篇」继续，而不是从头再来 |
| 行为要可审计 | 用户需要知道「它为什么改了这篇」。所以需要 trace：每步工具调用、读了什么、写了什么 |

---

## 3. Harness 的八个组件（对照 xink 现状）

| # | 组件 | 职责 | xink 现状 | 需要新建 |
|---|---|---|---|---|
| 1 | **Agent Loop** | 观察 → 计划 → 调工具 → 验证 → 呈现 → 用户裁决 | 无 | `core/ai/loop.rs` |
| 2 | **Tool Registry** | 声明可用工具、参数 schema、结果裁剪 | 无 | `core/ai/tools.rs` |
| 3 | **Context Policy** | 装配上下文、字符预算、compaction、tool-result clearing | 部分有（`core/search.rs`、`core/index.rs` 是召回层） | `core/ai/context.rs` |
| 4 | **Permission Model** | 只读默认；写走 diff；笔记内容视为不可信输入 | 部分有（原子写盘、`atomic_write`） | `core/ai/permission.rs` |
| 5 | **Verifier** | 结构校验：Markdown 可解析、双链可解析、标签合法、frontmatter 合法 | 有解析能力（`core/markdown.rs`、`core/datafile.rs`） | 薄封装 |
| 6 | **Persistence** | 会话 transcript、任务清单、进度文件落盘，可恢复可回放 | 有 `state/workspace.rs` 的落盘约定 | `state/ai_session.rs` |
| 7 | **Observability** | trace 每步（工具、token、耗时），面板可展开 | 无 | trace 结构 + 面板 |
| 8 | **Skills（程序性记忆）** | 见 `ai-capabilities-eval.md` §6 | 无 | `core/ai/skills.rs` |

**工具集建议（xink 原生函数，不走 MCP）**：
`read_note` / `search_notes` / `list_tags` / `list_backlinks` / `outline` / `list_notes` /
`propose_edit`（出 diff，不写盘）/ `create_note` / `add_tag` / `read_skill_resource` / `write_memory`。
理由见 `ai-capabilities-eval.md` §4.4：宿主自己就是工具提供方，走 MCP 只是徒增一层。

---

## 4. 记忆：四层模型

xink 的特殊之处在于**库本身就是记忆**，所以要先划清「谁的记忆」：

| 层 | 内容 | 存放 | 谁写 | 生命周期 |
|---|---|---|---|---|
| **L0 工作记忆** | 当前任务的上下文窗口：目标、近期轮次、工具结果、计划草稿 | 内存（会话内） | harness 装配 | 会话结束即弃；超阈值触发 compaction |
| **L1 库记忆** | **笔记本身**，唯一真值 | `<vault>/*.md` | 用户为主，AI 受控写 | 永久 |
| **L2 Agent 记忆（库内派生）** | 机器维护的派生状态：`MEMORY.md` 索引 + 主题文件 + 任务清单 + 进度日志 + 会话摘要 | `<vault>/.xink/agent/` | agent（经确认） | 可随时删除重建 |
| **L3 用户记忆（跨库）** | 写作风格偏好、术语表、常用动作、纠正过的错误 | `~/Library/Application Support/xink/agent/memory/` | agent（经确认） | 可编辑可清空 |

三条硬规则：

1. **L2/L3 必须是纯 Markdown / JSON、人类可读、可 diff、可整目录删除**。不引入数据库，不引入不透明的向量库——
   这既符合本地优先，也让用户能审计「它到底记住了什么」。
2. **L2/L3 与 L1 物理隔离**（独立目录 + 索引排除）。AI 的记忆绝不能混进用户的笔记流，
   也要防止 `SKILL.md`、记忆文件被当成笔记进索引（这一点在需求文档 §6 已标注为实现要点）。
3. **记忆写入是独立的一次小调用 + 结构化输出 + 用户可见**，不要在主对话里顺手写。
   原因：笔记正文是**不可信输入**（用户库里可能有从网上复制的内容），
   若让 agent 在读笔记时直接改持久记忆，就是一条 prompt injection → 记忆投毒的通路。

写入策略建议：**harness 后台抽取式**（会话结束时一次抽取 + 与已有记忆合并，出 diff 给用户确认），
而不是 Letta 那种「模型在循环里随时自己改」。前者可控、可审计、成本低；后者自治但难预测。

---

## 5. 可参考的框架与项目

### 5.1 协议层

| 项目 | 是什么 | 对 xink 的价值 |
|---|---|---|
| **ACP**（Agent Client Protocol，Zed 发起、Zed+JetBrains 共治，Apache-2.0） | 编辑器 ↔ agent 的 JSON-RPC over stdio 协议；session/turn 模型、diff 展示、权限请求、读未保存 buffer。<已有多家客户端与 25+ agent 支持，Obsidian 有插件> | **最值得借鉴的交互模型**。「编辑器是 client、agent 是 server」的划分，以及「agent 请求改文件、宿主决定」正是我们要的权限模型。将来 xink 既可以作为 client 接外部 agent，也可以作为 agent 被别的客户端接 |
| **MCP**（Model Context Protocol，捐给 Linux Foundation 下的 Agentic AI Foundation） | agent ↔ 工具/数据 | 我们不需要自己当 MCP client（§3）；但**可以把 xink 暴露为 MCP server**，让外部 agent 读库——这是 P2 的可选方向 |

### 5.2 Harness 实现（看它们怎么组织 loop 与状态）

| 项目 | 借鉴什么 | 不借鉴什么 |
|---|---|---|
| **Claude Agent SDK / Claude Code** | agent loop 的标准形态：工具即主要动作、subagent 隔离上下文、自动 compaction、`CLAUDE.md` 作为每请求重新注入的持久规则、`PreCompact` hook、session resume/fork、把「验证」做成一等公民 | 它的沙箱与运行时；我们不内嵌 SDK |
| **Anthropic《Effective harnesses for long-running agents》**（2025-11） | 跨上下文窗口的落地做法：initializer 建环境 → 每会话增量推进 → 留 `progress` 文件 + `feature_list.json`（每项 pass/fail）+ git 历史，让下一个「失忆」的会话能接上。**并点出两个失败模式：context anxiety（快到上限就提前宣布完成）、self-evaluation blindness（自我评分过高）** | 无人值守跑几天；xink 保留人在环 |
| **Codex CLI** | `exec --json` 事件流、sandbox 分级（read-only / workspace-write）、skills 发现 | 把写权限交给外部 CLI |
| **goose**（Block，Rust 核心 + CLI，Apache-2.0，已转基金会治理） | 「Rust 写的严肃 agent 长什么样」：单二进制、15+ provider、70+ MCP 扩展 | 直接依赖（体积与运行时冲突） |
| **Aider** | repo-map（用图/摘要表示大代码库给模型看）→ 对应我们的「库地图」（标签树 + 标题索引 + 双链图） | Python 实现 |
| **Cline / Roo Code** | 人在环的权限 UI 粒度（每次工具调用都要不要问、自动批准清单） | — |

### 5.3 上下文工程（直接可用的方法）

- Anthropic《Effective context engineering for AI agents》：compaction、tool-result clearing、
  **structured note-taking（agentic memory）**、subagent 隔离、just-in-time 检索（只存路径/查询，用时再加载）。
- Anthropic《Building agents with the Claude Agent SDK》：先 gather context → 再 take action → 最后 **verify**；
  三种验证手段（规则校验、代码化校验、视觉反馈）中，我们能用的是**规则/解析校验**。
- Anthropic《Agent Harness Design》(2026-04)：prompt 缓存视角下「静态在前、动态在后」的请求排布；
  让模型自己决定持久化什么（memory folder）比外挂检索更有效。
- Manus《Context Engineering》：把文件系统当作无限上下文、可压缩但可恢复、**保留失败尝试**、用复述 todo 把目标压在近期注意力里。

### 5.4 记忆框架（看数据模型，不用它们的实现）

| 项目 | 模型 | 借什么 | 判断 |
|---|---|---|---|
| **Letta（MemGPT）** | OS 式分层：core（在窗口内）/ recall / archival；**agent 自己用工具读写**；近期把 skills 做成一等「程序性记忆」 | 「程序性记忆 = skills」这个归类，正好解释我们在做什么 | 模型自治太强、每次查找都要一次推理；不照抄控制方式 |
| **Mem0** | LLM 抽取事实 + 向量/图双存；user / session / agent 三级 scope | **scope 划分**（用户级 / 会话级 / 库级）直接可用 | Python/TS，依赖向量库，不适合本地优先单二进制 |
| **Zep / Graphiti** | 双时序知识图（valid time / transaction time），事实变更不覆盖而是失效 | 时序建模的思路：笔记会变，「旧结论」应可追溯 | 需要图数据库，过重 |
| **Cognee / LangMem / Loomem** | 图+向量混合 / LangGraph 原生 / **Rust 单二进制 + RocksDB + Tantivy + 本地 ONNX，BM25+向量+实体图混合检索** | **Loomem 是形态参考**：本地优先、无外部服务、可离线、MCP 原生 | Loomem 是单用户记忆层，不是 agent 框架；可作 L2/L3 的检索形态参考 |

### 5.5 Rust 生态（现在不建议直接依赖，但 trait 设计可参考）

| crate | 现状 | 判断 |
|---|---|---|
| `rig-core`（0.42，MIT） | Rust 里事实上的 agent 框架：20+ provider、工具调用、memory、向量存储、OTel  tracing | **暂不引**：它是 Tokio async 栈，会与 gpui 执行器并存（同 `rmcp` 的理由）。若将来接受「AI 跑在独立线程 + 自带 tokio runtime」，它是第一候选 |
| `swiftide`（0.32，MIT） | agent harness + **编译期类型检查的 task graph** + 流式索引；内建 turn limit、stop tool、`ApprovalRequired` 人在环 | 值得读源码学 harness 结构；不直接依赖 |
| `adk-rust` / `autoagents` / `genai` | 各自活跃度一般 | 观察 |
| `rmcp` | 官方 MCP Rust SDK | 见 §5.1，P2 |

---

## 6. 对分期的影响（修订）

原 `ai-capabilities-eval.md` §10 的分期不变，但**每期的内容要升级**：

| 期 | 原计划 | 修订 |
|---|---|---|
| M1 | 8 个一次性动作 | **最小 agent loop + 只读工具集**（`read_note` / `search_notes` / `outline`）+ 一个写工具 `propose_edit`（出 diff）。UI 上仍然是「选中 → 润色 → 接受」，但底层已经是 loop |
| M2 | Skills | 不变，并明确 Skills = **L3 程序性记忆** |
| M3 | 库级管理 | 引入长任务形态：`task.json`（每篇一个条目，pass/fail）+ `progress.md` + 快照目录；支持中断恢复与续跑 |
| M4 | 会话面板 + CLI provider | 加 **L2/L3 记忆层**（文件化、可审阅、可清空）与会话摘要/compaction |
| M5（新增，可选） | — | **ACP**：xink 作为 client 接外部 ACP agent；或把库暴露为 MCP server |

---

## 7. 新增的开放问题

| # | 问题 | 建议 |
|---|---|---|
| Q8 | 记忆写入是否需要用户逐条确认 | 建议：L3（用户偏好）**必须确认**；L2（库内派生）可批量确认一次 |
| Q9 | 库级任务是否允许「跑着跑着停下等你」 | 建议允许，且这是默认：每 N 篇或每遇冲突停下 |
| Q10 | 是否引入 embedding 做记忆检索 | 建议 **L2/L3 初期用关键词 + 路径索引**（体量小、人类可读）；到数千条再评估 `fastembed` |
| Q11 | 是否要做 ACP 兼容 | 建议 **M5 再评估**；先把自家 loop 做扎实，协议只是外层适配 |

## 8. 新增风险

1. **记忆投毒 / 提示注入**：笔记正文不可信。记忆写入必须独立调用、结构化输出、用户可见。
2. **自我评估过高**（self-evaluation blindness）：不能让 agent 自己判定「整理完成」，
   验收要用规则（双链是否可解析、标签是否收敛、frontmatter 是否合法）。
3. **成本与时长失控**：库级任务要有硬上限（最大步数、最大 token、最大文件数），到顶就停下并报告进度。
4. **trace 体积**：会话 transcript 全量落盘会很大，需要轮转与上限。

---

## 9. 来源

- ACP：<https://agentclientprotocol.com/>、<https://zed.dev/acp>、ACP vs MCP <https://circleci.com/blog/acp-vs-mcp-what-s-the-difference-for-agentic-coding>
- Anthropic：《Effective harnesses for long-running agents》<https://www.anthropic.com/research/effective-harnesses-for-long-running-agents>、
  《Effective context engineering for AI agents》<https://anthropic.com/engineering/effective-context-engineering-for-ai-agents>、
  《Building agents with the Claude Agent SDK》<https://anthropic.com/engineering/building-agents-with-the-claude-agent-sdk>、
  《Agent Harness Design》<https://claude.com/blog/harnessing-claudes-intelligence>、
  Agent loop 文档 <https://platform.claude.com/docs/en/agent-sdk/agent-loop>
- 记忆：<https://mem0.ai/blog/open-source-ai-agents-with-built-in-memory>、
  Mem0/Letta/Zep 对比 <https://thedatapraxis.com/blog/knowledge-graphs-for-ai-agents>、Loomem 对比页 <https://loomem.ai/compare.html>
- Rust 生态：<https://arewelearningyet.com/agents>、<https://rig.rs/>、<https://decebaldobrica.com/blog/2026-06-18-rust-ai-crate-radar-agents-inference>
- 上下文工程实践：<https://manus.im/blog/Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus>

> 说明：以上均为公开资料核对（2026-10），未在本机安装或运行任何上述框架；
> 版本与 star 数随时间变化，引入依赖前需重新核对许可与维护状态。
