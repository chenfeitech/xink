# 设计：M3 —— 库级任务（批量动作与长任务）

> 需求：`docs/ai/ai-assistant-requirements.md` A6.1 / A6.2（多篇上下文、检索召回候选）、
> A7.1–A7.4（必须经 diff 预览、改前快照、复用 `atomic_write`、可中止）。
> 分期：`docs/ai/ai-assistant-design.md` §3.1（`session.rs` 标 M3、§3.4 的 `propose_edit` / `create_note`）、
> §5.3（库级任务流程）、§4.3 落盘表（`task.json` / `progress.md`）、决策 Q9（分批停下报告）、风险 R2–R4。
> 前置已完成：M1（agent loop / 只读工具 / 快照 / 原子写 / 索引与 `IndexChanged`）、M2（Skills）。
>
> **状态：✅ 已确认**（2026-10-27），T1 已实施，T2 进行中。
> 已拍板：D1 规则判定、D2 dialog 浮层、D3 标签面板入口、D7 一并做加 / 删标签。
>
> 命名偏差：分期表里写的是 `core/ai/session.rs`，实际落地为 **`core/ai/tasks.rs`**——
> `core::session`（标签 / 会话恢复）已经存在，两套"session"挨着会一直混淆。

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| AI 只有**单篇**链路：结果作用于「当前笔记」，落法由 `pending` + 评审条决定 | `src/ui/ai.rs`（`PendingEdit` / `classify_and_record`）、`src/ui/editor.rs`（评审条） |
| 标签面板是**只读**的：能看到 `#tag` 与带它的笔记，没有批量维护入口 | `src/ui/tags.rs`（`notes_with_tag` + 点击打开） |
| 索引已有「按标签取笔记」的能力，但下游没人用 | `src/core/index.rs:213` `notes_with_tag` |
| 快照是**单文件**粒度，批量要每篇调一次 | `src/core/ai/snapshot.rs::write_snapshot(dir, file_name, text)`，上限 `MAX_SNAPSHOTS = 200` |
| 原子写 + 索引刷新都有现成入口，不必新造 | `src/core/note.rs::atomic_write`、`src/app.rs`（`reindex_file` + `emit(IndexChanged)`） |
| **任务模型是空白**：`task.json` / `cursor` / 中断恢复 / 分批报告全都没有 | 分期表 §9 M3 行 |
| 写工具 `propose_edit` / `create_note` 还没做 | `src/core/ai/tools.rs` 顶部注释写明「写工具属 M3」 |
| 库级动作没有入口：命令面板只有单篇动作与 skill | `src/ui/command.rs`（`AiTarget`） |

一句话：**M1/M2 把"改一篇"做扎实了，M3 要解决"改一批"**——它带来三件新东西：
任务状态、批量写入的可中止与可恢复、以及一批结果怎么给用户过目。

## 2. 目标 / 非目标与分期

**目标**

- G1：能对一个明确的候选集合（如「带 `#AI` 的全部笔记」）生成**只读执行计划**，逐条可取消勾选。
- G2：确认后批量执行：每篇改前快照、`atomic_write`、索引刷新，中途可停。
- G3：中断可恢复：`task.json` 记 `cursor` 与已完成项，下次从计划面板「继续」。
- G4：分批报告：默认每 20 篇停下报告一次（Q9），结束给「改了 N 篇，跳过 M 篇，快照在 …」。
- G5：全程不经过模型的部分**用规则判定**，不靠模型自评（R2）。

**非目标**

- 不做"模型自由发挥改写全库"（`propose_edit` 驱动），留 P2；
- 不做向量 / embedding 召回（N3，已否决）；
- 不做跨库批量（跨库搜索本就不做）；
- 不落 trace（R4：默认不落盘，需要时再开）。

**分期建议（推荐按 P1 → P4 推进）**

| 期 | 内容 | 为什么先做 |
|---|---|---|
| **P1** | **规则可判定的批量动作**：批量改标签 / 加标签 / 删标签；任务模型 + 计划面板 + 分批执行 + 中断恢复 | 结果由**字符串替换规则**决定，不需要模型 → 成本 0、可单测、可预期；同时把 M3 的骨架（任务 / 计划 / 批量写 / 可恢复）一次搭完 |
| P2 | 工具 `propose_edit` / `create_note`：模型出批量提议，进同一个计划面板 | 依赖 P1 的计划面板与执行器；模型质量不可控，放在骨架稳定之后 |
| P3 | 多篇上下文（A6.1 用户勾选 / A6.2 检索召回）+ 库级 skill | 依赖 P2 与现有的 `search_files` 召回 |
| P4 | trace 落盘、任务历史面板 | 属于可观测性，有了前几期才有内容可看 |

### 2.5 决策记录（本设计按以下默认推进，需确认）

| # | 决策 | 取值 | 理由 |
|---|---|---|---|
| D1 | 第一期做什么 | **只做规则可判定的批量（改 / 加 / 删标签），不过模型** | 与 R2 同构：验收用规则判定而非模型自评。模型改全库既贵又不可预期，先把骨架做对 |
| D2 | 计划面板形态 | **dialog 浮层**（`window.open_dialog`，与设置 / 命令面板同构） | 库级任务与「当前笔记」无关，不该占编辑区下方（那里已是 AI 对话条 + 评审条）；计划可能上百行，需要大空间与滚动 |
| D3 | 入口 | **标签面板**：展开某标签 → 「全库改名 / 批量删除」按钮 | 上下文最明确（候选集合天然确定）。命令面板 `AI: 批量改标签` 留 P2 补，避免一开始就要解决「候选从哪来」 |
| D4 | 单篇的落法 | **确认后直接改盘**（计划即 diff），不进单篇评审条 | 几十上百篇不可能逐块人工确认（A7.1 的"经 diff 预览"由**整个计划**承担：每条都能看到改前 → 改后，且可单独取消勾选） |
| D5 | 中断与继续 | `task.json` + `cursor`，每 20 篇停一次（Q9），已完成项不重跑 | A7.4「批量执行中可中止，已执行的保持已完成状态」 |
| D6 | trace | 默认**不落盘** | R4：trace / 快照体积膨胀；需要排查时再开 |
| D7 | 动作种类 | P1 一并做 **改 / 加 / 删** 三种标签动作 | 复用同一套计划与执行器，边际成本低；只做改名的话标签维护不完整 |

## 3. 方案（P1）

### 3.1 任务模型 `core/ai/tasks.rs`（纯逻辑，可单测）

```rust
/// 一批要做的改动属于哪种
pub enum TaskKind {
    /// 全库把 `#from` 改成 `#to`
    Retag,
    /// 给候选集合统一加一个标签
    AddTag,
    /// 从候选集合里删掉一个标签
    RemoveTag,
}

pub enum TaskStatus { Ready, Running, Paused, Done }

/// 一条的状态：失败与跳过都要**带原因**，静默丢失等于骗用户
pub enum ItemState {
    Pending,
    Done,
    Skipped(String),
    Failed(String),
}

pub struct TaskItem {
    pub path: PathBuf,
    /// 计划阶段算好的预览：改前 / 改后的那几行（面板里逐条展示）
    pub preview: Vec<(String, String)>,
    pub state: ItemState,
}

pub struct Task {
    pub id: String,
    pub kind: TaskKind,
    /// 要改 / 加 / 删的标签（不带 `#`）
    pub target: String,
    /// 改后的标签（仅 Retag 用，其余为空）
    pub new_tag: String,
    pub status: TaskStatus,
    pub items: Vec<TaskItem>,
    /// 下一个要处理的下标：中断恢复靠它
    pub cursor: usize,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Task {
    /// 还有多少条待处理（面板标题用）
    pub fn pending(&self) -> usize;
    /// 统计：done / skipped / failed（结束报告用）
    pub fn stats(&self) -> (usize, usize, usize);
    /// 从 `cursor` 往后取一批，不带已完成项
    pub fn next_batch(&self, size: usize) -> Vec<usize>;
}

pub fn task_dir(vault_root: &Path, id: &str) -> PathBuf;   // <vault>/.xink/agent/tasks/<id>
pub fn save(vault_root: &Path, task: &Task) -> Result<(), String>;
pub fn load(vault_root: &Path, id: &str) -> Option<Task>;
pub fn list(vault_root: &Path) -> Vec<Task>;               // 「继续上次」用
```

### 3.2 计划生成：纯函数 + 一次读盘

```rust
/// 单篇的改写结果
pub enum RetagOutcome {
    Changed(String),   // 改后的整篇文本
    Unchanged,         // 没这个标签
    Skipped(String),   // 已有目标标签（避免重复）
}

/// 只替换**完整标签 token**：`#AI` 命中，`#AI助手` 不命中
///
/// 两处来源都要处理：正文里的 `#tag`，与 front-matter 的 `tags:` 列表
/// （`tags: [AI, 笔记]` 与 `- AI` 两种写法）。
pub fn retag_text(text: &str, from: &str, to: &str) -> RetagOutcome;

/// 生成计划：候选来自索引，正文读盘，产出可直接展示的预览
pub fn plan_retag(
    root: &Path,
    candidates: &[PathBuf],
    from: &str,
    to: &str,
) -> Vec<TaskItem>;
```

- 候选集合由调用方给（P1 是 `notes_with_tag`）→ 计划函数不依赖 `Entity<NoteIndex>`（规则 02：`core` 不碰 UI 类型）。
- 预览只取**含标签的那几行**（改前 → 改后），不是整篇 diff：计划面板要能一屏看几十条。
- 读盘在后台线程做（`cx.background_spawn`），与命令面板扫描同一套路。

### 3.3 执行器：后台线程 + flume 回传

与 `ui/ai.rs` 的既有结构完全同构（`Entity` 进不了后台线程，规则 02 §6）：

```rust
pub enum BatchEvent {
    /// 处理完一条：面板逐条打勾
    Item { ix: usize, state: ItemState },
    /// 一批结束（到批上限 / 全部完成 / 被取消）
    BatchDone { cursor: usize, status: TaskStatus },
    Failed(String),
}
```

单条的处理顺序（**顺序不能换**）：

1. 读原文 → 2. `write_snapshot`（改前）→ 3. `retag_text` → 4. `atomic_write`
   → 5. 回传 `Item` 事件（UI 侧再做 `reindex_file` + `emit(IndexChanged)`）

第 5 步必须回到 UI 实体上做：索引是 `Entity<NoteIndex>`，后台线程拿不到。
取消沿用 `Arc<AtomicBool>`（同 `AiChat.cancel`）。

### 3.4 计划面板 `ui/ai/plan.rs`（dialog 浮层）

```
┌ 批量改标签：#AI → #人工智能        共 42 篇 · 待处理 42 ┐
│ ☑ notes/a.md        - #AI          → - #人工智能      │
│ ☑ notes/b.md        tags: [AI]     → tags: [人工智能] │
│ ☐ notes/c.md        已含目标标签（跳过）               │
│ …                                                     │
│ [取消勾选的 1 篇]  [执行]  [停止]  [关闭]              │
│ 已改 20 篇，跳过 1 篇 · 快照在 ~/Library/…/ai-history  │
└───────────────────────────────────────────────────────┘
```

- 每条一行：勾选框 + 相对路径 + 改前 → 改后；跳过的灰色显示原因且不可勾选。
- 底部状态栏：进度、`已改 N 篇 / 跳过 M 篇`、快照目录。
- 「执行」→ 后台跑一批；到批上限自动停并报告，用户可继续点「执行」。
- 关闭浮层 ≠ 放弃任务：`task.json` 已落盘，下次从标签面板「继续上次」进来。

### 3.5 入口（P1）

标签面板展开某个标签后，标题行右侧加一个「⋯」/「改名」按钮 →
弹一个小输入框（新标签名）→ 生成计划（后台）→ 打开计划面板。

> 不用右键菜单：现有面板一律左键交互（`docs/index-panels-design.md`），保持一致。

## 4. 数据模型与持久化

| 内容 | 位置 |
|---|---|
| 任务 | `<vault>/.xink/agent/tasks/<task-id>/task.json` |
| 进度（人可读） | 同目录 `progress.md` |
| 改前快照 | `~/Library/Application Support/xink/ai-history/<时间戳>-<文件名>`（已有，上限 200） |

- `.xink` 已进恒跳名单（文件树 / 索引 / 命令面板候选都排除），新增子目录不需要改守卫
  （`core/vault.rs::should_ignore` + `core/media.rs::ai_excluded`）。
- **不新增配置项**：批大小先写死 20（Q9 的默认值），确有必要再进 `config.json`。
- 任务不属于任何一篇笔记，因此**不进** `state/ai_sessions.rs`（那是按笔记落盘的会话）。

## 5. 交互流程

1. 标签面板 → 展开 `#AI` → 点「改名」→ 输入 `#人工智能`。
2. 后台生成计划（读候选笔记正文）→ 计划面板列出 42 条，逐条可取消勾选。
3. 点「执行」→ 逐条打勾；每 20 篇停下报告；可随时「停止」。
4. 停止 / 关闭后从标签面板「继续上次」进来 → 已完成的不重跑，从 `cursor` 继续。
5. 全部结束 → 报告「改了 41 篇，跳过 1 篇，快照在 …」。

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 候选为空 / 标签不存在 | 不出计划，提示「库里没有带 #X 的笔记」 |
| 某篇已经含目标标签 | `Skipped("已含目标标签")`，不可勾选（避免重复标签） |
| 某篇读到一半失败（非 UTF-8 / 权限） | `Failed(原因)`，跳过继续，不中断整批 |
| 执行中某篇被外部改了 | 快照已写改前版本；以读到的当前文本为准改标签，事后提示（与现有外部修改策略一致） |
| 快照轮转 | `MAX_SNAPSHOTS = 200`：批量超过 200 篇时旧快照会被挤掉，计划面板底部要**明示**这一点 |
| 中止 | 已完成的保持完成（A7.4），`cursor` 落盘，不产生半篇写入（`atomic_write`） |
| 库被切换 / 关闭 | 任务按库根落盘，换库自然看不到；不跨库 |
| 新标签名非法（空 / 含空格 / 含 `#`） | 输入框即时校验，禁用「确定」 |

## 7. 测试计划

`tests/core/ai/tasks.rs`（新增，纯函数）：

| 用例 | 断言 |
|---|---|
| `retag_replaces_inline_tag_only_as_a_whole_token` | `#AI` 命中，`#AI助手` 不受影响 |
| `retag_handles_front_matter_list_and_dash_forms` | `tags: [AI]` 与 `- AI` 都会被替换 |
| `retag_returns_unchanged_when_tag_absent` | 无标签 → `Unchanged`，不进计划 |
| `retag_skips_when_target_already_present` | 已有目标标签 → `Skipped` |
| `plan_lists_every_candidate_with_a_preview` | 每条都有非空预览行 |
| `task_json_round_trips` | 落盘 → 读回，字段与 `cursor` 一致 |
| `next_batch_skips_finished_items` | 已 Done / Skipped 的不重复返回 |
| `stats_count_done_skipped_failed` | 三种状态分别计数 |

`tests/ui/tags.rs`（若已有则扩）：入口按钮只在展开标签后出现。

## 8. 人工验收清单

- [ ] 库里有 3 篇带 `#AI` 的笔记 → 标签面板展开 → 「改名」→ 计划列出 3 条，预览正确
- [ ] 取消勾选其中 1 条 → 执行后那篇没被改
- [ ] 执行 → 3 篇（勾选的）正文与 front-matter 里的标签都改了，文件树 / 标签面板同步更新
- [ ] 快照目录里能看到改前的版本
- [ ] 造 25 篇 → 执行到 20 篇停下报告 → 点继续 → 剩下 5 篇完成
- [ ] 执行中途停止 → 关闭 → 再从标签面板「继续上次」→ 已完成的不重跑
- [ ] `#AI` 不会误伤 `#AI助手`
- [ ] 已含目标标签的那篇显示为「跳过」且不可勾选
- [ ] `.xink/agent/tasks/**` 不出现在文件树、搜索与命令面板候选里

## 9. 实施步骤（P1）

| 步 | 内容 | 可独立验证 |
|---|---|---|
| T1 | `core/ai/tasks.rs`：任务模型 + `retag_text` / `add_tag_text` / `remove_tag_text` + `plan` + `task.json` 读写 | ✅ 21 项单测全绿 |
| T2 | 标签面板入口：展开态的「改名 / 加 / 删」按钮 + 新标签输入框 | 人工验收 1 |
| T3 | 计划面板（dialog）：逐条勾选 + 预览 + 状态栏 | 人工验收 1–3 |
| T4 | 执行器：后台批量 + flume 回传 + 快照 / 原子写 / 索引刷新 | 人工验收 3–4 |
| T5 | 分批停下（20）与中断恢复（cursor + 「继续上次」） | 人工验收 5–6 |

## 10. 开放问题

| # | 内容 | 建议 |
|---|---|---|
| 1 | 批大小 20 要不要可配 | 先写死；真有上千篇的库再进 `config.json` |
| 2 | 批量改标签之外，P1 还要不要带「批量加 / 删标签」 | 建议带：`AddTag` / `RemoveTag` 复用同一套计划与执行器，成本很低 |
| 3 | 任务要不要自动清理（跑完 N 天后删 `task.json`） | 先不清理，留一个「清除任务」入口即可；无自动删除更安全 |
| 4 | P2 的 `propose_edit` 要不要限制单次影响篇数 | 要：硬上限（如 50 篇）+ 超出要求用户收紧候选，对应 R3 |

## 11. 实施记录

### T1（2026-10-27，`core/ai/tasks.rs` + `tests/core/ai/tasks.rs`，21 项单测）

标签改写规则最终敲定如下（都写进了单测，改规则前先看它们）：

| 规则 | 行为 |
|---|---|
| 命中粒度 | 整 token 匹配：`#AI` 命中，`#AI助手` / `## 标题` 不命中 |
| 代码块 | 围栏内（``` / ~~~）一律不动，避免 `#define AI` 被误伤 |
| front-matter | 支持 `tags: [a, b]` 与 `tags:` + `- a` 两种写法；没有 `tags:` 键时插在闭合 `---` 之前 |
| 加标签 | 有 front-matter 就进列表，没有就在**文末**追加一行 `#tag`（前面补一个空行） |
| 删标签 | 删到整行只剩空白就连这一行删掉，并收掉多出来的空行；`tags:` 删空则整行去掉 |
| 改名 | 目标标签已存在 → `Skipped("已含 #x")`，不产生重复标签 |
| 异常 | 读不到的文件记为 `Failed(原因)`，不中断整批；计划阶段绝不写盘 |

命名偏差见文档开头：模块落地为 `core/ai/tasks.rs`，不是分期表里的 `session.rs`。
