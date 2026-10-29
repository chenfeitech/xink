# AI 结果"不全面"——生成预算可调与截断可见 — 技术设计

> 状态：**G2 已过**（2026-10-29 用户拍板 Q4 档位起点 10240、Q5 温度与上下文窗口一并开出口）
> 需求：[`ai-output-truncation-requirements.md`](./ai-output-truncation-requirements.md)
> 分支：`feature/ai-output-truncation`（已 rebase 到 `62b9426`）

---

## 1. 现状与问题（附证据）

### 1.1 输出截断链路上，没有任何一个环节能察觉

```
ollama.rs:283-291   if done { push(Done) }           ← 不看 done_reason
openai.rs:197-213   if finish_reason.is_some() { push(Done) }  ← 不看值
mod.rs:142-156      enum StreamEvent { Delta/Thinking/ToolCall/Usage/Done/Failed }
ui/ai.rs:960-963    StreamEvent::Done => self.finish(...)      ← 一律当成功
```

预算耗尽（`done_reason:"length"` / `finish_reason:"length"`）与正常结束（`"stop"`）
在这条链上**完全同形**，界面显示成一次正常的成功回复。

预算本身：

```rust
// config.rs:793-803
impl Default for AiModelParams {
    fn default() -> Self {
        Self { num_ctx: 8192, num_predict: 1024, temperature: 0.7, .. }
    }
}
```

`num_predict: 1024` ≈ 中文 700–1000 字，改一篇长文必然不够。
而这个字段**在设置界面里没有出口**：

- `all_fields()` 的 AI 数值项只有 4 个：`ai.timeout_secs`(`:971`)、`ai.ollama.num_ctx`(`:958`)、`ai.max_context_chars`(`:985`)、`ai.max_steps`(`:997`)
- `render_model_row`（`:2463-2666`）只有 6 个操作按钮：设为默认 / 停用 / 协议 / 密钥 / 删除 / 测试，**没有参数编辑**

请求体实际用的是逐模型值：

```rust
// ollama.rs:224-228
"options": {
    "num_ctx": req.params.num_ctx,
    "num_predict": req.params.num_predict,   // ← ModelEntry.params，不是全局
    "temperature": req.params.temperature,
}
```

### 1.2 输入截断有提示，但提示是灰的

```rust
// ai.rs:593 / 653 / 732
self.notice = prepared.notice.clone();     // "内容超过预算，本次只发送第 1–X 行"
// ai.rs:1938-1939
if let Some(notice) = &self.notice {
    status = status.child(stat(Some(IconName::Info), notice.clone(), muted));
}
```

同一个 `muted` 灰色还用于耗时(`:1911`)、工具次数(`:1915-1919`)、tokens(`:1931`)，
一句警告混在四五个中性数字里，扫一眼就漏掉。

`PreparedContext` 其实**已经带了 `truncated: bool`**（`context.rs:24`），
但 `ai.rs` 三处调用点都只取了 `notice`，把 `truncated` 丢掉了。

### 1.3 `notice` 不能一刀切变红

`notice` 承载的信息不止警告（`ai.rs` 全部赋值点）：

| 行号 | 内容 | 性质 |
|---|---|---|
| `:593` / `:653` / `:732` | 「内容超过预算，本次只发送第 1–X 行」 | **警告** |
| `:1101` | 「模型和原文对不上，已按"插入"处理」 | 中性说明 |
| `:1138` | 「已在编辑区内按块评审…」 | 中性说明 |
| `:1292` | 「已写入改前快照」 | 中性说明 |

所以必须**分流**，不能把整个 `notice` 改成红色。

---

## 2. 目标 / 非目标

目标：见需求文档 §1.2（G1 生成预算可调 / G2 输出截断可见 / G3 输入截断可见）。

非目标：

- 不改 `num_predict` 默认值（仍 1024）——用户选了"先只暴露设置项"
- 不改上下文截断策略（仍从开头取前缀）
- 不改 `max_context_chars` 默认 12000
- 不顺手修 `OllamaConfig.num_ctx` 与 `ModelEntry.params.num_ctx` 的双份关系（P6）
- 不动 agent loop / 工具调用 / prompt 拼装 / 模型路由 / `StoredBlock` 落盘格式
- 不新增依赖

---

## 3. 方案概述

### 3.1 选定方案

| 环节 | 做法 |
|---|---|
| 协议层 | `StreamEvent` 新增 **`Truncated`** 变体（无载荷）；Ollama 侧 `done_reason == "length"` 时、OpenAI 侧 `finish_reason == "length"` 时，在 `Done` **之前**发出 |
| UI 状态 | `AiChat` 新增两个独立 `bool`：`out_truncated`（生成预算耗尽）、`ctx_truncated`（上下文被截断）；每次发起请求前重置 |
| UI 呈现 | 状态行末尾两条**红色**（`danger` + `CircleAlert`）提示，各带去哪调；`notice` 其余来源保持灰字不变 |
| 设置项 | `render_model_row` 增一行**三个参数的档位下拉**（逐模型，闭包捕获模型 id）：最大生成 / 上下文窗口 / 温度 |

关键取舍：**新增事件变体而不是扩充 `Usage`**，理由见 §3.2-B。

### 3.2 被否决的方案

| # | 方案 | 否决理由 |
|---|---|---|
| A | 全局设置项 `ai.num_predict` | `num_predict` 在 `ModelEntry.params` 上（`config.rs:507`），请求体读的是它（`ollama.rs:226`）。做成全局要新增配置项、并引入"全局默认 + 逐模型覆盖"两层语义，超出本次范围（需求 C4） |
| B | 给 `Usage` 加 `truncated: bool` 字段 | OpenAI 路径的 `usage` 只在响应**带 `usage` 字段**时才更新（`openai.rs:160-163`），而 `build_body`（`:222-228`）**没有请求 `stream_options: {include_usage: true}`**，实测多半拿不到 → 截断信号会静默丢失。要修就得顺带改请求体，把改动面从"加一个字段"扩大到"改协议请求" |
| C | 复用 `AiError::Truncated` | 语义不同：它表示**连接中断**（`mod.rs:175`、文案 `mod.rs:198`），且被判为 retryable（`mod.rs:269`）。长度截断重试一次只是再烧一遍 token，毫无意义 |
| D | 把 `notice` 整条改成红色 | `notice` 还承载 3 处中性说明（`ai.rs:1101/1138/1292`），一刀切会让"已写入改前快照"也变成报错（§1.3） |
| E | 模型行加「参数」按钮，点开一个展开区编辑 `num_predict` / `temperature` / `num_ctx` | 要新增展开态字段、多组输入框与本地编辑状态，改动面远大于"暴露一个字段"。本次只需要 `num_predict` |
| F | 用既有 `number_control`（`settings.rs:1254`）画「最大生成」 | 它的 `set: fn(&mut AppConfig, f32)` 是**裸函数指针，不能捕获模型 id**；输入框状态存在 `inputs: HashMap<&'static str, _>`（`:1376`），key 是 `'static str`，逐模型的运行时 id 放不进去 |

### 3.3 为什么是档位下拉而不是步进器

`number_control` 用不了（§3.2-F），得自己画。两种自画形态：

- **步进器**（− / 数值 +）：点很多次才能到目标值，且要管输入框状态
- **档位下拉**：一步点到位，且这三个参数（生成预算 / 上下文 / 温度）本来就是粗粒度选择，不需要精确到个位数

选档位下拉。三个参数各一组，同一行横排，标签定宽对齐：

| 参数 | 字段 | 档位 | 说明 |
|---|---|---|---|
| 最大生成 | `params.num_predict` | **10240 / 20480 / 32768** | 起点 10240（用户拍板 Q4），上限 `AI_NUM_PREDICT.1`（`config.rs:1123`） |
| 上下文窗口 | `params.num_ctx` | 4096 / 8192 / 16384 / 32768 / 65536 … **再按该模型 `context_limit` 过滤** | 单位 token；不过滤会出现"选了被 `sanitize()` 夹回"（`config.rs:984-988`） |
| 温度 | `params.temperature` | 0.0 / 0.3 / 0.7 / 1.0 / 1.5 / 2.0 | 默认 0.7 在档位内；`AI_TEMPERATURE`（`config.rs:1124`） |

当前值不在档位里（用户手改过 `config.json`）时，标签显示实际数值，选任一档位后归入档位。

**注意**：`num_predict` 默认值 1024 **低于**最低档 10240（Q2 决定不改默认值）。
因此首次打开时标签显示 `1024`，一旦选档就跳到 ≥10240 —— 这是预期行为，
等于把"默认值偏小"这件事显式摆到用户面前，而不是悄悄改掉默认值。

---

## 4. 数据模型与持久化

### 4.1 不新增持久化字段

`num_predict` 是 `AiModelParams` 的**既有字段**（`config.rs:509`），
`#[serde(default)]`（`config.rs:504`），`sanitize()` 已做 clamp：

```rust
// config.rs:976-979
model.params.num_predict = model
    .params
    .num_predict
    .clamp(limit::AI_NUM_PREDICT.0, limit::AI_NUM_PREDICT.1);
```

因此：

- **无迁移逻辑**：历史 `config.json` 里 `models[].params` 已整体序列化，读回来照旧
- **无兼容问题**：UI 只是给既有字段开一个出口，值的语义与落盘格式都不变
- 越界值由 `sanitize()` 夹回，UI 档位下拉的取值范围与之一致

### 4.2 新增的运行时状态（不落盘）

| 字段 | 位置 | 类型 | 落盘 |
|---|---|---|---|
| `StreamEvent::Truncated` | `mod.rs:142` 枚举 | 无载荷变体 | — |
| `out_truncated` | `ui/ai.rs` `AiChat` | `bool` | 否（请求级） |
| `ctx_truncated` | `ui/ai.rs` `AiChat` | `bool` | 否（请求级） |

会话落盘格式（`StoredBlock`）不受影响。

---

## 5. 交互流程

### 5.1 正常路径：调大「最大生成」

1. 设置 · AI · 模型 → 找到当前默认模型那一行
2. 该行参数区三个下拉：最大生成 / 上下文窗口 / 温度
3. 「最大生成」当前显示 `1024`（默认值，低于最低档），选 `20480`
4. `update_config` 写 `ModelEntry.params.num_predict`
5. 下一次请求 `ollama.rs:226` 带上 20480，长文不再断在半句
6. 「上下文窗口」只列出不超过该模型 `context_limit` 的档位，选了不会被夹回
7. 「温度」选档即写 `params.temperature`，下一次生成风格随之变化

### 5.2 输出被截断

1. 模型生成到预算上限，最后一帧 `done_reason: "length"`（或 `finish_reason: "length"`）
2. provider 依次发出 `StreamEvent::Truncated` → `StreamEvent::Done`
3. `ui/ai.rs` 收到 `Truncated` → `out_truncated = true`；收到 `Done` → `finish()`
4. 状态行末尾出现红色一行：
   > ⚠ 生成到预算上限就停了，结果不完整（设置 · AI 里调大「最大生成」）

### 5.3 输入被截断

1. `assemble()` 返回 `truncated: true` + `notice`
2. `ui/ai.rs` 置 `ctx_truncated = true`（**不**再写 `notice`，避免同一句话说两遍）
3. 状态行出现红色一行：
   > ⚠ 内容超过预算，本次只发送第 1–X 行（设置 · AI 里调大「上下文上限」）

两种截断可同时出现，各占一格，互不覆盖。

---

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| `done_reason` 字段缺失（老版本 Ollama / 非标端点） | 按"未截断"处理，行为与今天一致 |
| `done_reason` 是未知值（如 `"unload"`） | 只认 `"length"`，其余一律未截断 |
| `finish_reason: null`（OpenAI 非末帧恒为 null） | `.as_str()` 取不到 → 不触发，与今天一致 |
| `finish_reason: "tool_calls"` | 不是 `"length"`，不触发（工具轮次不该报截断） |
| OpenAI 端点不返回 `usage` | 不影响：`Truncated` 是独立事件，与 `Usage` 无关（§3.2-B） |
| 一次请求里两种截断同时发生 | 两格都显示，输出的排在后面（更严重） |
| 用户把「最大生成」调到 32768 | `sanitize()` 夹到 `AI_NUM_PREDICT.1`；请求体照发，慢由用户承担 |
| 模型行在下拉菜单打开期间被删除 | 参照 `render_key_panel`（`settings.rs:2685-2689`）的既有做法：找不到模型就不渲染该控件 |
| agent loop 多步执行 | 每步都会发 `Done`；`out_truncated` 用"任一为真"语义（`\|=`），不被后续步骤清掉 |

---

## 7. 测试计划

### 7.1 能自动化

| # | 用例 | 位置 |
|---|---|---|
| T-1 | Ollama 帧：`done_reason:"length"` → 产出 `Truncated` + `Done`；`"stop"` → 只有 `Done`；字段缺失 → 只有 `Done` | `tests/core/ai/ollama_parse.rs` |
| T-2 | OpenAI 帧：`finish_reason:"length"` → 产出 `Truncated`；`"stop"` / `"tool_calls"` → 不产出 | `tests/core/ai/openai.rs` |
| T-3 | `num_predict` 越界被 `sanitize()` 夹回 `AI_NUM_PREDICT` | `tests/core/config.rs`（若已有 `sanitize` 测试则并入） |
| T-4 | 既有测试全绿（尤其 `tests/core/ai/agent_loop.rs`、`tests/ui/ai.rs`、`tests/ui/settings.rs`） | 全仓 |
| T-5 | `ctx_options(context_limit)`：`None` 时给全档；`Some(4096)` 时只给 ≤4096 的档；上限低于最小档时至少给一档 | `tests/ui/settings.rs` |

### 7.2 要人工（登记进 `docs/ACCEPTANCE.md`）

AC-6 / AC-7 / AC-8 / AC-9（见需求 §5）——涉及真实模型、真实长文，自动化跑不了。

---

## 8. 人工验收清单

1. 设置 · AI · 模型行参数区能看到三个下拉：最大生成 / 上下文窗口 / 温度
2. 「最大生成」档位是 10240 / 20480 / 32768，当前显示默认 1024
3. 选 20480，重开一次长文改写 → 输出完整，不再断在半句
4. 用一篇改写后明显超过 10240 token 的长笔记发一次 → 状态行出现**红色**提示，文案点明「最大生成」
5. 打开一篇超过 12000 字的笔记，发一次 → 状态行出现**红色**提示，文案点明「上下文上限」
6. 「上下文窗口」只列出不超过该模型上限的档位
7. 改「温度」后同一句话两次生成，风格有可感差异
8. 「已写入改前快照」这类普通提示**仍是灰色**，没被误染成红
9. `cargo test` 全绿

---

## 9. 风险与开放问题

| # | 风险 | 处置 |
|---|---|---|
| R1 | 新增 `StreamEvent` 变体后 4 处 match 漏补分支 | 靠编译期穷举匹配兜住（`agent_loop.rs:127-136` 是穷举的）；实施时按需求 C1 逐个过 |
| R2 | 档位下拉与用户手改 `config.json` 的值不一致 | 非档位值照常显示（§3.3），选档后归入档位；不校验、不纠正 |
| R3 | Ollama 之外的端点未必遵守 `finish_reason:"length"` | 只认字面量，不猜；拿不到就不提示，行为退回今天 |
| R4 | 默认 1024 仍然偏小，多数用户可能根本不会去设置里找 | **已知缺口**，本次按用户决策不改默认值；观察一轮后再定 |

开放问题（需用户拍板）：

- ~~**O1**：档位起点定 512 是否够低？~~ → **已拍板（Q4）：10240 起步**
- ~~**O2**：是否顺带把 `temperature` / `num_ctx` 也开出口？~~ → **已拍板（Q5）：一并开出口**（R-5）

仍挂着的一项（不阻塞本次，等用户定方向）：

- **O3**：全局 `ai.ollama.num_ctx` 与 `ai.ollama.keep_alive` 是**死配置项**（改了不生效，请求用的是逐模型 `params.*`）。
  逐模型出口到位后，全局这两项就成了纯误导。方向二选一：**让它生效**（作为逐模型默认值下发）或**从设置里摘掉**。
  本次不动——摘设置项属于独立改动，且两个方向相反，不敢替用户选。

---

## 10. 实施步骤

| 期 | 内容 | 涉及文件 | 关联 | 验收 |
|---|---|---|---|---|
| **P1** | `StreamEvent::Truncated` 变体；Ollama `done_reason`、OpenAI `finish_reason=="length"` 解析；补全部 match 分支 | `src/core/ai/{mod,ollama,openai,agent_loop}.rs` | R-2 | T-1 / T-2 / T-4 + G4 四连 |
| **P2** | `AiChat` 加 `out_truncated` / `ctx_truncated`；状态行两条红字；`assemble` 的 `truncated` 不再丢 | `src/ui/ai.rs` | R-2 / R-3 / R-4 | T-4 + G4 四连 + 人工 AC-7 / AC-8 |
| **P3** | 模型行三个参数的档位下拉（最大生成 / 上下文窗口 / 温度）；`num_ctx` 档位按 `context_limit` 过滤（抽成纯函数便于单测） | `src/ui/settings.rs` | R-1 / R-5 | T-3 / T-5 / T-4 + G4 四连 + 人工 AC-6 / AC-10 |

每期可独立编译验证，不存在"全做完才能编"的任务。
