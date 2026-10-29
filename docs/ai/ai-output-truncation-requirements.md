# AI 结果"不全面"——生成预算可调与截断可见 — 需求规格

> 状态：**G1 已过**（2026-10-29 用户拍板，见 §6）
> 关联：[`multi-model-architecture-design.md`](./multi-model-architecture-design.md)、
> [`ai-m1-design.md`](./ai-m1-design.md)
> 落地位置（预计）：`src/core/ai/{ollama,openai,mod}.rs`、`src/core/config.rs`、
> `src/ui/settings.rs`、`src/ui/ai.rs`
> 分支：`feature/ai-output-truncation`

---

## 1. 背景与目标

### 1.1 现状（全部附证据）

用户报"每次 AI 处理的内容不全面"，实测两种现象**同时存在**，根因不同：

| # | 现象 | 根因 | 证据 |
|---|---|---|---|
| S1 | **输出到一半就停了**：改写 / 润色长文时回答断在半句 | 生成预算 `num_predict` 默认 **1024**，到顶即停 | `config.rs:798`（`AiModelParams::default`） |
| S2 | **后半篇没被处理**：AI 只动了笔记前半部分 | 上下文超预算时**只取开头前缀**，后半篇压根没发给模型 | `context.rs:96-106`（`window()`）、`ai.rs:510-517`（`context_budget`） |

两条都**静默发生**，界面上没有任何异常提示：

| # | 问题 | 证据 |
|---|---|---|
| P1 | `num_predict` **在设置界面里根本没有**，用户无法自己调大 | `ui/settings.rs` 的 AI 分组只有 `ollama.num_ctx`(`:958`) / `max_context_chars`(`:985`) / `max_steps`(`:997`) / `timeout_secs`(`:971`)；`all_fields()` 的 AI 项清单 `settings.rs:1145-1149` 同样没有 |
| P2 | Ollama 路径**不解析 `done_reason`**，预算耗尽（`done_reason:"length"`）与正常结束（`"stop"`）长得一模一样 | `ollama.rs:283-291` 只读 `done` / `prompt_eval_count` / `eval_count` / `eval_duration`，然后直接 `out.push(StreamEvent::Done)` |
| P3 | OpenAI 兼容路径**只判断 `finish_reason` 存在与否**，不判值 | `openai.rs:197-213`：`.as_str().is_some()` 就 `push(Done)`，`"length"` 与 `"stop"` 不区分 |
| P4 | `StreamEvent` 没有表达"因长度截断"的通道，`Done` 一律当成功 | `mod.rs:142-156`（`enum StreamEvent`）；`ui/ai.rs:960-963` `Done => self.finish(...)` |
| P5 | 上下文被截断时只有一句**灰字**提示，与"生成中 / 工具次数"同色，扫一眼就漏 | `ai.rs:1937-1939` `stat(Some(IconName::Info), notice.clone(), muted)`；同一行 `muted` 还用于耗时(`:1911`)与 tokens(`:1931`) |
| P6 | 请求实际用的是**逐模型**的 `params.num_predict`，而设置表里那个 `num_ctx` 是 `OllamaConfig` 全局的，两者不是一回事 | `ollama.rs:224-228`（`"num_predict": req.params.num_predict`）；`config.rs:614`（`OllamaConfig.num_ctx`）vs `config.rs:507`（`ModelEntry.params.num_ctx`） |

### 1.2 目标

| # | 目标 | 衡量 |
|---|---|---|
| G1 | 生成预算用户可以自己调 | 设置界面能改当前模型的 `num_predict`，改完立刻生效 |
| G2 | **输出被截断时看得见** | 因 `length` 停止时，状态行有醒目提示，不再当成一次正常成功 |
| G3 | **输入被截断时看得见** | 上下文只发了前半篇时，提示从灰字升为醒目提示，并说明去哪调 |

---

## 2. 非目标

- **不改 `num_predict` 默认值**（仍 1024）——用户明确选择"先只暴露设置项"，默认值调整留待观察后再定
- **不改上下文截断策略**（仍从开头取前缀）——改成分段 / 摘要 / 滑动窗口是另一笔账
- **不改 `max_context_chars` 默认 12000**
- **不处理 `OllamaConfig.num_ctx` 与 `ModelEntry.params.num_ctx` 的双份关系**（P6）——既有问题，另立需求
- **不动 AI 业务逻辑**：agent loop / 工具调用 / prompt 拼装 / 模型路由 / 会话落盘格式（`StoredBlock`）一律不改
- **不新增依赖**

---

## 3. 功能模块

| # | 需求 | 说明 |
|---|---|---|
| R-1 | **生成预算可调** | 设置界面为**每个模型**暴露 `num_predict`（最大生成 token）：档位 **10240 起步**，上限 `limit::AI_NUM_PREDICT.1`（`config.rs:1123`，32768）；越界由既有 `sanitize()` 夹取 |
| R-2 | **输出截断可见** | 协议层识别预算耗尽：Ollama `done_reason == "length"`（`ollama.rs:283`）、OpenAI 兼容 `finish_reason == "length"`（`openai.rs:197`）；经 `StreamEvent` 传到 UI，状态行**红色**提示"生成到预算上限就停了：去设置里调大「最大生成」" |
| R-3 | **输入截断可见** | 上下文被 `window()` 截断时，那句 notice 从 `muted` 升为 `danger`，文案补上去哪调（"上下文上限"） |
| R-4 | **提示可操作** | 两类提示都点明对应的设置项名称，用户不用猜 |
| R-5 | **温度与上下文窗口一并开出口** | 同一个模型行上再暴露 `temperature`（`limit::AI_TEMPERATURE`，0.0–2.0，默认 0.7）与 `num_ctx`（单位 token，逐模型）；`num_ctx` 的档位按该模型 `context_limit` 过滤——超上限的档位不列出，避免"选了又被 `sanitize()` 夹回"（`config.rs:984-988`） |

---

## 4. 边界与约束

逐条过边界九项：

| # | 边界项 | 判定 | 处理 |
|---|---|---|---|
| 1 | 空输入 | 不适用 | 本次只动协议解析与设置项，不涉及输入为空的路径 |
| 2 | 超大输入 | **适用** | 正是本次要暴露的场景；`num_predict` 调到 32768 时 UI 与请求体都要正常（不超过 `limit::AI_NUM_PREDICT.1`） |
| 3 | 非法路径 | 不适用 | 无路径输入 |
| 4 | 并发写 | 不适用 | 不新增写盘时机（设置项走既有 `update_config`） |
| 5 | 外部程序改文件 | 不适用 | — |
| 6 | 崩溃后半截文件 | **适用** | 新增字段若落 `config.json`，半截时 `#[serde(default)]` 必须能兜住：`num_predict` 是既有 `AiModelParams` 字段，非新增，无迁移成本 |
| 7 | 多库切换 | 不适用 | 设置项是全局配置，不随库变 |
| 8 | 隐藏与忽略文件 | 不适用 | — |
| 9 | 非 UTF-8 | **适用** | `done_reason` 是协议字段，取值未知时一律按"未截断"处理，绝不 panic |

补充约束：

| # | 约束 | 理由 |
|---|---|---|
| C1 | `StreamEvent` 新增变体后，**所有 match 点都要补分支** | 已知 4 处：`mod.rs:305`（重试判断）、`mod.rs:433`（结束判断）、`agent_loop.rs:127-136`、`ui/ai.rs:946-967`；`probe.rs:74-76` 也匹配过。漏一处就编译不过或语义错 |
| C2 | 新变体必须在 `Done` **之前**发出 | `ui/ai.rs:960` 收到 `Done` 就 `finish()`，之后再来的事件没有落点 |
| C3 | 不得把"长度截断"当成 `AiError` | `AiError::Truncated`（`mod.rs:175`）已表示**连接中断**，语义不同；且它是 retryable（`mod.rs:269`），长度截断重试没有意义 |
| C4 | `num_predict` 是**逐模型**字段（`ModelEntry.params`），不做全局项 | 请求体用的是 `req.params.num_predict`（`ollama.rs:226`）；改成全局要动数据模型并引入"全局默认 + 逐模型覆盖"两层语义，超出本次范围 |
| C5 | 不在 `tests/` 根新增文件 | `Cargo.toml:13` `autotests = false`，靠 `#[path]` 挂载 |
| C6 | `ollama_parse.rs` 的帧解析单测不需要本机 Ollama | 新增的 `done_reason` 用例放这里，属常跑单测 |

---

## 5. 验收标准

| # | 类型 | 标准 | 关联 |
|---|---|---|---|
| AC-1 | 自动 | Ollama 帧解析：`done_reason:"length"` 产出截断事件；`"stop"` 不产出；字段缺失不产出 | R-2 |
| AC-2 | 自动 | OpenAI 帧解析：`finish_reason:"length"` 产出截断事件；`"stop"` / `"tool_calls"` 不产出 | R-2 |
| AC-3 | 自动 | `num_predict` 越过上下限时被 `sanitize()` 夹回 `limit::AI_NUM_PREDICT` | R-1 |
| AC-4 | 自动 | `cargo build` / `clippy --all-targets` 0 warning、`fmt --check` 干净 | G4 |
| AC-5 | 自动 | 既有测试全绿（含 `tests/core/ai/ollama_parse.rs`、`openai.rs`、`context.rs`、`tests/ui/settings.rs`） | C1 / C6 |
| AC-6 | 人工 | 设置里能看到并改当前模型的「最大生成」，改成 4096 后一次长文改写不再断在半句 | R-1 |
| AC-7 | 人工 | 用一篇改写后**明显超过 10240 token** 的长笔记发一次，状态行出现**红色**提示，且提示里点明了去哪调（档位最低 10240，调不小；要造小规模截断只能手改 `config.json`） | R-2 |
| AC-8 | 人工 | 打开一篇超过预算的长笔记发一次，状态行那句上下文提示是**红色**且点明了「上下文上限」 | R-3 |
| AC-9 | 人工 | OpenAI 兼容模型（若有）同样能在预算耗尽时给出红色提示 | R-2 |
| AC-10 | 人工 | 模型行上「温度」能改且改完生效（同一句话两次生成风格有可感差异）；「上下文窗口」档位只列出不超过该模型上限的那些 | R-5 |

---

## 6. 决策记录

| # | 问题 | 结论 | 日期 |
|---|---|---|---|
| Q1 | "不全面"具体指哪种 | **两种都有**：输出断在半句 + 后半篇没被处理 | 2026-10-29 |
| Q2 | 修多深 | **只暴露设置项 + 截断可见化**；不动上下文截断策略。默认值先不动，后于 2026-10-29 追加拍板**改为 8192**（见 Q6） | 2026-10-29 |
| Q3 | `num_predict` 落点 | **逐模型**（模型行上），不由全局项承担——理由见 C4，方案对比见设计文档 | 2026-10-29（由 C4 数据模型决定） |
| Q4 | 「最大生成」档位起点 | **10240**（不是 512）。默认值 8192 低于最低档，因此界面显示实际值 8192，选档后跳到 ≥10240 | 2026-10-29 |
| Q5 | `temperature` / `num_ctx` 是否一并开出口 | **是**，同一个模型行上一并暴露（R-5） | 2026-10-29 |
| Q6 | `num_predict` 默认值 | **8192**（原 1024）。只影响**新建**的模型条目——已有 `config.json` 里的条目已经存了旧值，不会被默认值改写，需要用户在档位下拉里选一次 | 2026-10-29 |

待用户拍板清单：**已清空**。
