# AI · M1 收口 — 技术设计

> 状态：**待用户确认**（规则 05：确认后才动 `src/`）
> 日期：2026-10-26
> 上游：[`ai-assistant-design.md`](./ai-assistant-design.md)（总设计，本文只写它的 M1 缺口）、[`ai-assistant-requirements.md`](./ai-assistant-requirements.md)（A1–A9）、[`ollama-provider-design.md`](./ollama-provider-design.md)（probe / 模型管理）

---

## 1. 现状与问题

M1 的"通信 + 上下文 + diff + 快照 + 对话条"已落地，但设计 §3.1 点名的模块里**缺三块**，另有六处收尾项未做。逐条附证据：

| # | 现状 | 证据 |
|---|---|---|
| S1 | `core/ai/` 没有 `loop.rs` / `tools.rs` / `verify.rs` | `src/core/ai/mod.rs:14-23` 的 `pub mod` 列表只有 context/diff/keychain/models/ndjson/ollama/openai/prompt/snapshot/sse |
| S2 | 每次请求都是**一轮 completion**，模型拿不到库里其它笔记 | `src/ui/ai.rs:320` `start_request` 直接 `provider.stream(...)`，无回灌 |
| S3 | 设计 §10 的硬约束"M1 必须先做 `loop.rs`，否则 M3 要重写"未满足 | `ai-assistant-design.md:374-375` |
| S4 | `.xink/skills/**/SKILL.md` 会被当笔记收录（M2 一旦放 skills 就炸） | 三处枚举点都没有排除判据：`src/core/index.rs:136` `collect_markdown`、`:157` `reindex_file`（只判 `is_note`）、`src/ui/command.rs:317` `collect_candidates` |
| S5 | 有"从 Ollama 拉取"，**没有单模型连接测试** | `src/ui/settings.rs:1317`；`ModelEntry.tools_enabled` / `last_checked` / `capabilities` 三个字段已存在但无人写入（`src/core/config.rs:600`） |
| S6 | 命令面板没有 AI 入口（A9.1） | `src/ui/command.rs` 无 AI 相关代码 |
| S7 | 快照只写不清、无轮转上限（R4） | `src/core/ai/snapshot.rs` 只有 `write_snapshot` / `SNAPSHOT_DIR` |
| S8 | `StreamEvent::Usage` 收了但状态行不显示（A8.2） | 收集在 `src/ui/ai.rs:403`，状态行 `:731-741` 只有耗时与输入粗估 |
| S9 | 设计 §5.1 仍写"底部 dock 面板"，与决策 D1（编辑器下方对话条）矛盾 | `ai-assistant-design.md:239` vs `:64` |

---

## 2. 目标 / 非目标

**目标**

1. 补上 `loop.rs` / `tools.rs` / `verify.rs`，让"单篇动作"与将来的"库级多步"共用同一条路径。
2. `.xink/skills/**` 从索引 / 命令面板 / 搜索候选里彻底排除（判据收敛到一个函数）。
3. 补齐可观测性与运维件：单模型连接测试、快照轮转与清空、真实 usage、命令面板 `AI:` 入口。
4. 同步文档，让设计与代码一致。

**非目标**

| # | 非目标 | 理由 |
|---|---|---|
| N1 | 写工具 `propose_edit` / `create_note` | M3（设计 §3.4） |
| N2 | Skills 发现与注入 | M2 |
| N3 | 库级计划面板 / 长任务恢复 | M3 |
| N4 | 记忆 L2/L3、CLI provider | M4 |
| N5 | 工具调用过程的折叠交互 | 先用"一行一条"平铺，折叠交互留 M2 |

---

## 3. 方案概述

### 3.1 分层（沿用规则 02：`core` 不碰 UI）

```
ui/ai.rs            接线：判断走 loop 还是单轮；展示工具调用与校验结果
   ↓ 只读数据 + 回调
core/ai/loop.rs     gather → act → verify 的推进器（阻塞式，跑后台线程）
core/ai/tools.rs    工具定义（JSON schema）+ 执行 + 结果裁剪
core/ai/verify.rs   结构校验（双链 / frontmatter / 标签）
```

工具要读索引与磁盘，但 `core` 不能碰 `Entity<NoteIndex>`。解法与 `SecretStore` 同构：**抽 trait，UI 侧实现，测试用内存实现**。

```rust
/// 工具能看到的"库"。UI 侧用 `AppState.notes` 实现，测试用内存实现。
pub trait ToolHost {
    fn read_note(&self, path: &Path) -> Option<String>;
    fn search(&self, query: &str, limit: usize) -> Vec<Hit>;      // core/search.rs:30
    fn tags(&self) -> Vec<(String, usize)>;                        // core/index.rs:202
    fn backlinks(&self, path: &Path) -> Vec<LinkRef>;              // core/index.rs:179
    fn outline(&self, path: &Path) -> Vec<String>;                 // 由 parse_facts 取 headings
    fn list_notes(&self, limit: usize) -> Vec<(PathBuf, String)>;
}
```

### 3.2 Agent Loop

```rust
pub struct LoopRequest<'a> {
    pub messages: Vec<ChatMessage>,
    pub model: &'a ModelEntry,
    pub tools_enabled: bool,
    pub max_steps: u8,          // 单篇默认 8（AiConfig::max_steps）
    pub host: &'a dyn ToolHost,
}

pub enum LoopEvent {
    Delta(String),                  // 最终文本增量
    ToolCall { name: String, args_summary: String },
    ToolResult { name: String, lines: usize, chars: usize },
    VerifyFailed(Vec<VerifyIssue>), // 已回灌重试
    Done,
}

pub fn run(
    req: &LoopRequest,
    provider: &dyn Provider,
    cancel: Arc<AtomicBool>,
    on: &mut dyn FnMut(LoopEvent),
) -> Result<LoopOutcome, AiError>;
```

推进规则：

1. `tools_enabled == false`（探针未通过 / 用户关闭）→ **退化为当前的一轮 completion**，不进 loop（行为与今天完全一致，零回归面）；
2. 否则最多 `max_steps` 步：模型要工具 → `tools::execute` → 结果裁到 `max_tool_result_chars`（默认 4000）→ 回灌 → 再问；
3. 模型给最终文本 → `verify::check` → 有问题就回灌，最多重试 2 次；
4. 每一步前查 `cancel`，置位即停。

**是否进 loop 的判定只用 `ModelEntry.tools_enabled`**（由 P5 的连接测试写入），不猜模型能力。

### 3.3 工具集（M1 只读 6 个）

`read_note` / `search_notes` / `list_tags` / `list_backlinks` / `outline` / `list_notes`（设计 §3.4 的 `read_skill_resource` 随 M2 一起）。

协议差异按设计 §3.3 实测结论各自解析：**Ollama 的 `arguments` 是对象、OpenAI 兼容是字符串**；Ollama 思维链字段 `message.thinking`。

### 3.4 校验（verify.rs）

| 校验 | 判据 | 数据来源 |
|---|---|---|
| 双链可解析 | `index.resolve_wikilink(target)` 为 `Missing` | `core/index.rs:225` |
| frontmatter 合法 | 存在 `---` 块时能被 `core/datafile.rs` 解析 | 复用既有解析 |
| 标签合法 | 非空、不含空白与 `#` | `parse_facts` 的 `Tag` |

产出 `Vec<VerifyIssue>`，UI 侧在 diff 上方标红显示；重试 2 次仍失败也照样呈现（不阻塞用户）。

### 3.5 排除判据（一处定义，三处接入）

```rust
/// core/media.rs（与 is_editable_text / editor_language 同处，已是"什么文件算什么"的收敛点）
pub fn ai_excluded(path: &Path) -> bool;   // 位于 .xink/ 下即为 true
```

接入：`core/index.rs:136` `collect_markdown`、`ui/command.rs:317` `collect_candidates`、`ui/search.rs` 的候选来源（搜索的 `files` 由调用方传入，故接入点在调用方）。

### 3.6 被否决的方案

| 方案 | 否决理由 |
|---|---|
| 让 `core/ai/tools.rs` 直接持有 `Entity<NoteIndex>` | 违反规则 02（core 不碰 UI 类型），且领域层无法单测 |
| 用文本协议（让模型输出 ```tool 块）代替原生 function calling | 各家已有原生 tools，M0 spike 实测 3/4 模型通过；自造协议要自己处理转义与截断，收益为负 |
| 工具不支持时报错 | 与设计"探针失败 → 降级为单轮"冲突（设计 §7 Ollama probe 用例） |
| 把 `ai_excluded` 放 `core/ai/` | 会让 `core/index.rs` 依赖 `core/ai/`，方向别扭；`media.rs` 本就是共享判据处 |

---

## 4. 数据模型与持久化

| 项 | 变更 | 兼容 |
|---|---|---|
| `AiConfig` | 新增 `agentic: bool`（默认 `true`，总开关：关了就永远单轮）、`max_tool_result_chars`（默认 4000，夹 `[500, 20000]`） | 全字段 `#[serde(default)]`，旧 `config.json` 无此段照常启动 |
| `ModelEntry` | **不改结构**：P5 只开始写入已有的 `tools_enabled` / `last_checked` / `capabilities` | 已有字段，无迁移 |
| 快照目录 | `~/Library/Application Support/xink/ai-history/` 不变；新增轮转（保留最近 200 个） | 只删最旧，不改命名 |

命令面板候选是运行时构造，不落盘。

---

## 5. 交互流程

1. **单篇动作（有工具）**：选动作 → 对话区出现 `⚙ search_notes("标签体系") → 3 条` 这样的一行一行 → 结果增量出现 → 校验不通过时上方标红 → diff → 接受 / 丢弃。
2. **单篇动作（无工具）**：与今天完全一致，一轮出结果。
3. **连接测试**：设置 · AI · 模型管理，每个模型一行加「测试」→ 行内显示 `✓ 320ms · 支持工具` / 具体错误；结果写回该模型。
4. **清空快照**：设置 · AI 底部「清空 AI 历史快照（N 个）」→ 二次确认 → 删除并刷新计数。
5. **命令面板**：输入 `AI:` 或动作名 → 列出 8 个内置动作 → 回车 = 打开对话条并执行该动作。
6. **取消**：停止按钮 / Esc 依旧置 `AtomicBool`，loop 每一步自查。

---

## 6. 异常与边界

| 场景 | 处理 | 用户看到 |
|---|---|---|
| 模型不支持工具 | 探针置 `tools_enabled=false`，走单轮 | 无（与今天一致） |
| 工具返回空 | 回灌"没有命中"，让模型继续 | 工具行显示 `0 条` |
| 工具名 / 参数非法 | 回灌错误文本，不中断 loop | 工具行显示 `参数错误` |
| 步数到上限 | 停下，用已产出的文本（没有则报 `EmptyResult`） | 「已到步数上限（8），以下是当前结果」 |
| 校验失败且重试 2 次仍失败 | 照常呈现 | diff 上方标红列出未通过项 |
| 连接测试失败（401/404/超时） | 归类到既有 `AiError`，写 `last_checked` | 行内显示对应中文文案 |
| 快照目录不存在 / 已空 | 计数 0，按钮禁用 | 「暂无快照」 |
| 库未打开 | 工具全部返回空 | 「没有打开库」 |
| loop 期间取消 | 逐步自查后立即返回 `Canceled` | 回到空闲态 |

---

## 7. 测试计划

**可自动化（`tests/core/ai/`，`#[path]` 挂载）**

| 模块 | 用例 |
|---|---|
| `tools.rs` | 6 个工具各自命中；空结果；参数非法；**只读性**：临时库执行前后 mtime 与内容不变；结果裁到 `max_tool_result_chars` |
| `verify.rs` | 双链目标不存在 / 歧义 / 命中；frontmatter 非法；标签含空格为空 |
| `loop.rs` | fake provider：step 上限生效；工具结果回灌；`cancel` 后不再回调；`tools_enabled=false` 时只跑一轮 |
| `media.rs` | `ai_excluded`：`.xink/skills/x/SKILL.md` 为 true、普通笔记为 false |
| `index.rs` | 放了 SKILL.md 的临时库，`notes()` 里不含它 |
| `snapshot.rs` | 轮转后只保留最近 200 个；清空删净 |

**需人工验收**

工具调用在对话区的可读性、连接测试的真实耗时与错误文案、命令面板 `AI:` 与文件名搜索共存、取消后的响应性、小模型（无工具）仍能用。

---

## 8. 人工验收清单

1. 未配置 → 提示「未配置」，无网络请求。
2. Ollama 拉取模型 → 点「测试」→ 显示耗时与是否支持工具；错地址 / 未启动给出可读错误。
3. 选中中文 →「润色」→ 文字逐步出现 → diff 正确 → 接受后落盘。
4. 快照出现在 `ai-history/`，等于改前原文。
5. 丢弃 → 文件与编辑器均不变。
6. 中途停止 → 无残留回调（**含 loop 场景**），可立刻发起下一次。
7. 断网 / 错 key / 超时 5s → 三类错误文案正确。
8. 整篇「生成标签」→ 优先复用库内标签。
9. **新**：支持工具的模型上，问"库里还有哪些笔记提到 X" → 对话区出现工具行 → 答案引用了真实路径。
10. **新**：不支持工具的模型 → 行为与今天一致（一轮出结果，不报错）。
11. **新**：`Cmd+P` 输入 `AI:` → 列出 8 个动作 → 回车打开对话条并执行。
12. **新**：设置 · AI → 清空快照 → `ai-history/` 被清空。
13. **新**：`.xink/skills/x/SKILL.md` 不出现在文件树、索引、搜索、命令面板。
14. 关闭 AI 总开关 → 所有入口不可用，其余功能不受影响。

---

## 9. 风险与开放问题

| # | 风险 | 对策 |
|---|---|---|
| R1 | 各家 tools 协议漂移 | 解析各自独立（设计 §3.3）；新端点接入前留档一次真实请求 |
| R2 | 小模型工具调用质量差，反而更慢更差 | 只在探针通过时启用；`AiConfig.agentic` 可整体关掉 |
| R3 | loop 让一次动作变成多次请求，耗时与 token 上升 | `max_steps` 硬上限（单篇 8）；对话区显示步数与工具次数 |
| R4 | 校验误报（库内确实没有该笔记） | 校验只"标红提示"，不阻断应用 |
| R5 | 主线程被阻塞 | loop 与 provider 同样只在 `background_spawn` 调用（规则 02 §6） |

**需拍板（本文按推荐取值，可推翻）**

| # | 问题 | 推荐 |
|---|---|---|
| Q1 | 工具调用默认开吗 | **只在连接测试探针通过时开**，另给 `ai.agentic` 总开关 |
| Q2 | 连接测试用什么探针 | Ollama：`/api/tags` + 1 token chat；OpenAI 兼容：一次 `max_tokens=1` 的 chat（不依赖 `/models`，各家路径不一） |
| Q3 | 命令面板 `AI:` 入口 M1 就做吗 | **做**（与 skills 解耦，M2 只是往同一分组加 skill） |
| Q4 | 工具调用过程要不要折叠交互 | M1 平铺一行一条，折叠留 M2 |
| Q5 | 校验失败是否阻断应用 | **不阻断**，只标红 |

---

## 10. 实施步骤（每期独立可验证）

| 期 | 内容 | 涉及文件 | 独立验证 |
|---|---|---|---|
| **P1 地基与小件** | `ai_excluded` + 三处接入；快照轮转与清空入口；状态行显示真实 usage；同步 `ai-assistant-design.md` §5.1 与 `INDEX.md` | `src/core/media.rs`、`src/core/index.rs`、`src/ui/command.rs`、`src/core/ai/snapshot.rs`、`src/ui/settings.rs`、`src/ui/ai.rs`、文档 | 清单 12、13；单测 |
| **P2 工具层** | `core/ai/tools.rs` + `ToolHost` + JSON schema + 6 个工具 + 结果裁剪 | `src/core/ai/tools.rs`、`src/core/ai/mod.rs`、`tests/core/ai/tools.rs` | 单测（含只读性） |
| **P3 校验层** | `core/ai/verify.rs` + 三类校验 | `src/core/ai/verify.rs`、`tests/core/ai/verify.rs` | 单测 |
| **P4 Loop** | `core/ai/loop.rs` + 接线到对话条（工具行、步数、取消、降级单轮） | `src/core/ai/loop.rs`、`src/ui/ai.rs`、`src/core/config.rs`（`agentic`） | 清单 6、9、10 |
| **P5 可观测与入口** | 单模型连接测试（写回 `tools_enabled` / `capabilities` / `last_checked`）；命令面板 `AI:` 入口 | `src/ui/settings.rs`、`src/ui/command.rs`、`src/ui/editor.rs`（转发） | 清单 2、11 |

顺序硬约束：**P2 → P3 → P4**（loop 依赖工具与校验）；P1 / P5 可并行或提前。

---

## 11. 参考

- 总设计与分期：`ai-assistant-design.md` §3.2（loop）、§3.4（工具）、§7（测试）、§10（M0–M5）
- 需求：A1.4（连接测试）、A3.4（双链校验）、A5.x（skills，M2）、A8.2/A8.4、A9.1
- Provider 与探针细节：`ollama-provider-design.md`
