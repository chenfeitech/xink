# 设计：AI 修订评审（hunk 级接受 / 拒绝）

> 需求：AI 改动要像 `git diff` 一样按块展示原文与改后文，每块给「接受修订」「拒绝修订」；
> 接受只改这一段，拒绝保留原文。同时吸收上一轮的结论：不再只有"替换整篇 / 不采用"两个选项。
> 相关文档：`ai-m1-completion-design.md`（结构校验）、`ai-chat-blocks-design.md` §6（多轮与 diff 固化）。

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 落法在**发起时**就定死：无选中 → `ReplaceWhole`，有选中 → `ReplaceSelection`；模型返回什么不影响它 | `src/ui/ai.rs:615-619` |
| 只有**一个**应用按钮，标签取 `kind.label()`，旁边只剩"不采用" | `src/ui/ai.rs:1397-1404` |
| 应用动作是 `set_selected_range(0..len)` + `replace`——真正的整篇覆盖 | `src/ui/editor.rs:1583-1591` |
| diff 只有"整篇平铺的 `Vec<DiffLine>`"，**没有 hunk 概念**：不分组、没有上下文、没有原文行号 | `src/core/ai/diff.rs`（只有 `diff_lines` / `diff_stats`） |
| 续写 / 生成摘要 / 生成标签天生该"插入"，现在也被判成替换整篇 | `src/core/ai/prompt.rs:11-28` + `src/ui/ai.rs:615` |
| 唯一的防丢措施是应用前写快照，但 UI 上没提示、也没别的落法 | `src/ui/ai.rs:1026` |
| 编辑面板是 `flex_col`：工具栏 / 正文 / （AI 对话条）/ 状态行——中间可以插一层评审条 | `src/ui/editor.rs:2230-2266` |

## 2. 目标 / 非目标

**目标**

- G1：AI 产出后按 **hunk** 展示：`-` 原文行（删除色）、`+` 改后行（主色）、带 2 行上下文。
- G2：每个 hunk 两个入口：**接受修订**（只改这一段）/ **拒绝修订**（保留原文、丢弃这段改动）。
- G3：评审期间原文被改动要能发现——定位不到就不改，绝不猜。
- G4：兜底落法还在，但不再是唯一选项：`替换选中` / `插入到光标处` / `替换整篇` / `不采用`，破坏性最大的那个要二次确认。
- G5：每个内置动作有合理的默认落法（续写/摘要/标签 → 插入）。

**非目标**

- 不做**行内**装饰（gutter 色条、编辑器内红绿背景）：gpui 编辑器的装饰能力未调研，先用独立的评审条，风险低、可用性强。
- 不做词级 / 字符级 diff，不做三方合并。
- 不改模型与 provider，不改现有快照机制。

## 3. 方案

### 3.1 新增 `core/ai/patch.rs`（纯函数，可单测）

```rust
/// 一个改动块（对应 git diff 的一个 hunk）
pub struct Hunk {
    /// 原文起始行号（0-based，仅用于展示）
    pub start_line: usize,
    /// 该块覆盖的原文行（Same + Del 的原文侧）
    pub orig_lines: Vec<String>,
    /// 替换后的行（Same + Add 的新侧）
    pub new_lines: Vec<String>,
    /// 展示用序列（含上下文行）
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
}

pub fn hunks(before: &str, after: &str, context: usize) -> Vec<Hunk>;

/// 在笔记里定位这个 hunk 对应的字节区间
pub enum Located { Unique(Range<usize>), NotFound, Ambiguous(usize) }
pub fn locate_range(note: &str, hunk: &Hunk) -> Located;

pub fn apply_hunk(note: &str, hunk: &Hunk) -> Result<String, PatchError>;
```

- `hunks()`：在 `diff_lines` 之上按连续改动分组，相邻改动间隔 ≤ `2 * context` 行就并成一块，
  每块前后各带 `context`（默认 2）行上下文。
- `locate_range()`：**按原文行序列**在笔记里找唯一匹配（逐行比较、忽略行尾空白），
  返回 `Unique` / `NotFound` / `Ambiguous(n)`。**这是"局部修改不误伤"的关键**：
  行号会因前面的改动而失效，内容定位不会。
- `apply_hunk()`：只在 `Unique` 时替换该区间；`Ambiguous` / `NotFound` 返回错误，不猜。
- 上限 `MAX_HUNKS = 50`：超了只保留前 50 并在评审条上提示"改动过多，已截断"。

### 3.2 评审状态（挂在 `EditorPanel`）

```rust
struct AiReview {
    path: PathBuf,
    hunks: Vec<HunkEntry>,
}
struct HunkEntry {
    hunk: Hunk,
    state: HunkState,
}
enum HunkState {
    Pending,
    Accepted,
    Rejected,
    /// 原文被改过，定位不上了：按钮禁用，提示重新生成
    Stale(String),
}
```

- 与 `ai_chat` 同级挂在 `EditorPanel`（`src/ui/editor.rs:59` 附近），随面板走——切标签回来评审还在。
- 笔记路径不匹配（`switch_note` 换了笔记）→ 整个评审作废。
- 进入评审时**写一次快照**（沿用 `write_snapshot`），后续接受任意 hunk 不再重复写。

### 3.3 触发时机

`AiChat::finish()` 里由**结果**决定，而不是发起时的选中状态（§3.6）：

1. 算出 `hunks(before, after, 2)`；
2. hunks 非空 → `panel.start_review(hunks)`，评审条出现在编辑区下方；
3. hunks 为空（纯插入、或输出与原文无对应关系）→ 落到 `InsertAtCursor`；
4. 同时保留"替换整篇"作为兜底按钮（ghost + 二次确认）。

### 3.4 渲染：编辑区下方的"修订评审条"

位置：夹在正文与 AI 对话条之间（`src/ui/editor.rs:2259` 之后），`max_h(240) + min_h_0 + overflow_y_scroll`
（滚动写法沿用 `ui/ai.rs` 里已验证的那一套）。

```
┌ AI 修订（3 处待处理 · +8 -5）        全部接受  全部拒绝 ┐
│ 第 12 行起 · +2 -1            接受修订   拒绝修订 │
│   上下文行                                        │
│ - 原文这一行                                      │
│ + 改后的这一行                                    │
│   上下文行                                        │
│ 第 40 行起 · +1 -0            接受修订   拒绝修订 │
└──────────────────────────────────────────────────┘
```

- 已处理的块整块置灰、按钮换成一个标签：`已接受` / `已拒绝` / `原文已改动，需重新生成`。
- 全部处理完 → 评审条收起，状态行提示"已应用 N 处修订（快照已保存）"。

### 3.5 接受 / 拒绝

- **接受**：`locate_range(当前笔记原文, hunk)` → `Unique` 才 `apply_hunk` → 写回笔记 → 状态 `Accepted`。
  每次都**按内容重新定位**，因此接受顺序无所谓（不像行号叠加会错位）。
- **拒绝**：只把状态置 `Rejected`，不动原文。
- **定位失败**：状态置 `Stale`，按钮禁用；提示"原文已改动，这段修订已失效，请重新生成"。

### 3.6 落法判定（把上一轮的 P0 合并进来）

`finish()` 时按输出判定：

| 判定 | 落法 |
|---|---|
| 输出与原文相似度 > 0.7 且行数接近 | hunks 可能覆盖全篇 → 仍进评审（逐块接受），兜底按钮才允许"替换整篇" |
| 输出是片段（明显更短 / 能定位到原文块） | hunks 少 → 评审（默认路径） |
| 与原文无对应关系（纯新增） | `InsertAtCursor` |

内置动作的默认落法：

| 动作 | 默认落法 |
|---|---|
| 润色 / 扩写 / 精简 / 翻译 | 有选中 → 替换选中；无选中 → **评审** |
| 改写标题 | 评审（通常只有一个 hunk，只动首行） |
| 生成摘要 / 生成标签 / 续写 | **插入到光标处**（不进评审） |

按钮组（按可用性显示）：`接受全部` `拒绝全部`、`替换选中`、`插入到光标处`、`替换整篇`（ghost，删除行 > 30% 时二次确认）、`不采用`。

## 4. 异常与边界

| 场景 | 处理 |
|---|---|
| 评审期间用户手改了原文 | 接受时定位失败 → 该块 `Stale`，其余块不受影响 |
| 原文里有多个相同的段落（定位到 N 处） | `Ambiguous(N)` → 不应用，提示"这段原文出现 N 次，无法确定位置" |
| 笔记被关闭 / 切到别的笔记 | 评审随面板；路径不匹配即作废 |
| 改动块超过 50 | 截断到 50 并提示；建议用户缩小改动范围重跑 |
| 输出与原文完全不同（整篇重写） | hunks 会很大 → 正常进评审；"替换整篇"作为一键兜底 |
| 接受前笔记有未保存修改 | 照常应用（应用后是脏状态，用户自己保存）；快照已写 |
| 大笔记（>1500 行差异） | `diff.rs` 已退化为"整段替换"，此时 hunks 会很大 → 同上 |

## 5. 测试计划

`tests/core/ai/patch.rs`（新增，纯函数）：

| 用例 | 断言 |
|---|---|
| `hunks_groups_adjacent_changes` | 相邻改动合并成一块，间隔大的分成两块 |
| `hunks_include_context_lines` | 每块前后各 2 行上下文 |
| `hunk_carries_original_and_new_lines` | `orig_lines` / `new_lines` 与 diff 一致，可直接重建改后文本 |
| `locate_range_finds_unique_block` | 唯一 → `Unique(range)` |
| `locate_range_reports_ambiguous` | 原文重复两块 → `Ambiguous(2)` |
| `locate_range_reports_not_found` | 原文被改掉 → `NotFound` |
| `apply_hunk_only_touches_that_block` | 应用后只有该段变化，前后文逐字节不变 |
| `apply_hunks_in_any_order_is_stable` | 先接受后面的、再接受前面的，结果与顺序无关 |
| `hunks_are_capped` | 超过 `MAX_HUNKS` 被截断 |

评审状态机（`tests/ui/editor.rs`，新增）：pending → accepted / rejected / stale；全部处理完 → 列表清空。

## 6. 人工验收清单

- [ ] 对一篇长笔记执行"润色"（无选中）：编辑区下方出现评审条，按块显示原文 `-` 与改后 `+`
- [ ] 点某一块的"接受修订"：只有那一段变了，其余内容逐字不变
- [ ] 点另一块的"拒绝修订"：原文保持不动，该块置灰
- [ ] 接受前手动改几行原文，再接受另一块 → 该块提示"原文已改动"，不误改
- [ ] "全部接受" / "全部拒绝" 生效
- [ ] 执行"续写"：直接插入到光标处，不弹评审条
- [ ] 无选中时执行"润色"，仍能用兜底的"替换整篇"（带二次确认）
- [ ] 设置 → AI 快照里能找到这次改动前的快照

## 7. 实施步骤

| 步 | 内容 | 可独立验证 |
|---|---|---|
| S1 | 新增 `core/ai/patch.rs`（`Hunk` / `hunks` / `locate_range` / `apply_hunk`）+ `tests/core/ai/patch.rs` | 纯函数单测全绿 |
| S2 | `EditorPanel` 加 `ai_review` 状态与 `start_review` / `accept_hunk` / `reject_hunk` / `accept_all` / `reject_all`，状态机可测 | 编译通过；状态机单测 |
| S3 | 评审条 UI（编辑区下方，滚动、按块、两个按钮、置灰态） | 编译通过；人工可见 |
| S4 | `AiChat::finish()` 改为结果驱动：算 hunks → 进评审；落法判定 + 按钮组（含兜底与二次确认） | 人工验收清单 1–8 |
| S5 | 文档：INDEX.md 登记；`ai-chat-blocks-design.md` §6 补"落法已改为结果驱动" | 文档自洽 |

## 8. 已拍板的决策（2026-10-26）

| # | 内容 | 结论 |
|---|---|---|
| 1 | 评审条位置 | **编辑区下方常驻条**（改动小、不挡正文）；不做抽屉 / 覆盖层 |
| 2 | 行内装饰（编辑器里红绿高亮 + gutter 按钮） | **本期不做**（依赖 gpui 编辑器装饰能力，需先 Spike） |
| 3 | 接受修订后自动保存？ | **不自动保存**（与现有 AI 应用行为一致），只标脏状态；进入评审时写一份快照 |
