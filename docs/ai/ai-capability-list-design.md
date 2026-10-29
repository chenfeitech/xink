# 模型能力清单：获取、展示与说明 — 技术设计

> 状态：**G2 待用户确认**
> 需求：[`ai-capability-list-requirements.md`](./ai-capability-list-requirements.md)
> 分支：`feature/ai-capability-list`

---

## 1. 现状与问题（附证据）

### 1.1 能力元数据目前没有"元数据"

```rust
// config.rs:632-633
/// 来自 `/api/tags`：`completion` / `tools` / `thinking` / `vision`
pub capabilities: Vec<String>,
```

约定值只活在注释里。没有枚举、没有注册表、没有说明文案 —— 所以"展示含义与用途"
这件事不是加个 UI 就行，得先有**字典**。

### 1.2 UI 原样拼接英文原始串

```rust
// settings.rs:1545-1546
if !model.capabilities.is_empty() { parts.push(model.capabilities.join(" / ")); }
```

显示出来是 `completion / vision / tools / thinking`。除"工具调用"一项外，
其余连中文标签都没有，更别说含义。

### 1.3 OpenAI 兼容侧是零

模型只能手填（`settings.rs:2456-2461` 直接 `..Default::default()` → `capabilities` 为空），
且**没有 `/v1/models` 拉取**（`ai-m1-completion-design.md:238` 明确排除：
各家路径不一）。所以配 DeepSeek 时一行能力都看不到 —— 这是本次要补的核心缺口。

### 1.4 声明不可信，但运行时另有判据

`probe.rs:1-4` 记着实测：`qwen3.5:0.8b` 在 `/api/tags` 里声明支持 tools，
真发工具过去返回 500。所以运行时**只信** `tools_enabled`（`ui/ai.rs:861`），
`capabilities` 只是"厂商说它有"。

这条决定了静态表的定位：**它给的只是默认值，不是承诺**。

### 1.5 `think` 的真实处境（本次不改，但要如实标注）

- 只对 Ollama 生效：`ollama.rs:223` 请求体有 `"think"`；`openai.rs:218-235` 的
  `build_body` **根本没有这个字段**
- 界面没有思考开关（`settings.rs:1594-1602` 的 `ModelParam` 只有三项）
- `supports_thinking()`（`config.rs:920`）是 `#[allow(dead_code)]` 死代码

用户拍板 Q2 是"只展示说明，不改行为"，所以本次**不做开关**，
但说明文案里要写清"Ollama 侧才生效"，不能让用户以为选了就有用。

---

## 2. 目标 / 非目标

目标见需求 §1.2（G1 每类模型都能看到清单 / G2 中文标签 + 含义用途 / G3 未知可见）。

非目标：

- 不改运行时行为：不做 `think` 开关、不动 agent loop 判据
- 不扩展探针（thinking / vision 不实测）
- 不新增 `/v1/models` 拉取
- 不清理 `supports_tools()` / `supports_thinking()` 死代码
- 不做 vision 功能
- 不新增依赖

---

## 3. 方案概述

### 3.1 选定方案

| 环节 | 做法 |
|---|---|
| 字典 | 新模块 `src/core/ai/capability.rs`：`Capability` 常量 + `info(id)` →（中文标签 / 含义 / 用途）；未知 id 回落"未知能力" |
| 推断 | 同模块 `infer(kind, base_url, model_id) -> Vec<String>`，**纯函数**：按模型名特征词 → 域名 → 兜底 |
| 数据模型 | `ModelEntry` 加 `capabilities_pinned: bool`；`true` 时 `/api/tags` 与静态表都不再覆盖它 |
| 展示 | 模型行下能力 chip 一行：每项一个小按钮，**悬停看说明**；末尾「编辑」下拉可勾选增删 |

### 3.2 被否决的方案

| # | 方案 | 否决理由 |
|---|---|---|
| A | 把 `capabilities` 改成枚举 `enum Capability` | 厂商会造新词（如 `embedding`）；枚举会让未知值在反序列化时丢失，而"未知也要能看到"是需求 R-5。保持自由字符串 + 字典回落 |
| B | 新增 `/v1/models` 拉取 | OpenAI 的 `/v1/models` 只有 `id/created/owned_by`，**没有能力字段**；且各家路径不一（`ai-m1-completion-design.md:238` 当初就排除了）。拉了也拿不到能力 |
| C | 扩展探针实测 thinking / vision | 用户拍板 Q3 不动。且 thinking 判据各家不一（`reasoning` / `reasoning_content` / Ollama 的 `message.thinking`），误判代价高；vision 本项目非目标 |
| D | 说明文案直接写在 UI 渲染处 | 加一个能力要改两个地方（字典 + UI），必然漂移。需求 C6 要求同一处 |
| E | 用 `Option<Vec<String>>` 表示"用户改过" | 语义混淆：Ollama 的 `/api/tags` 也会填 `Some(...)`，但那不是用户改的。用独立 `bool` 更清楚（见 §4.2） |

### 3.3 静态表是启发式，不是承诺

```rust
/// OpenAI 兼容端点没有能力发现接口，只能按域名 + 模型名**推断**一个默认值
///
/// 用户拍板 Q1：给出默认值 + 允许手动改。所以匹配不上就返回空（UI 显示
/// "未知"），匹配上了也不代表一定支持——运行时权威判据仍是探针实测的
/// `tools_enabled`（`ui/ai.rs:861`）。
pub fn infer(kind: AiProviderKind, base_url: &str, model_id: &str) -> Vec<String>
```

匹配顺序（先具体后宽泛）：

| 次序 | 判据 | 结果 |
|---|---|---|
| 1 | 模型名含 `reasoner` / `thinking` / `-r1` | `completion` + `thinking` |
| 2 | 模型名含 `vision` / `-vl` | 加上 `vision` |
| 3 | 域名 `api.deepseek.com` 或模型名含 `deepseek` | `completion` + `tools`（`deepseek-chat`） |
| 4 | 域名含 `bigmodel` / `moonshot` / `dashscope` / `openai` 或模型名含 `glm` / `qwen` / `gpt` | `completion` + `tools` |
| 5 | 其他 | **空** → UI 显示"未知" |

Ollama 侧 `infer` 不被调用（它有 `/api/tags` 的真实来源）。

### 3.4 字典内容

| id | 中文标签 | 含义 | 用途 |
|---|---|---|---|
| `completion` | 对话补全 | 模型最基本的能力：给一段文字，续写或回答 | 所有对话、快捷动作、改写都依赖它 |
| `tools` | 工具调用 | 能按约定格式请求调用外部函数（读库、搜索笔记） | 开启后 AI 可先查笔记再回答；**以「连接测试」实测结果为准** |
| `thinking` | 思考 / 推理 | 先输出一段内部推理再给答案 | 复杂问题更准，但更慢、且占用生成预算；**仅 Ollama 侧可关闭** |
| `vision` | 视觉理解 | 能接收图片输入 | 本项目当前未使用 |
| 未知 | 未知能力 | 这个能力项不在已知列表里（厂商自定义） | 可手动删除，保留也不影响运行 |

---

## 4. 数据模型与持久化

### 4.1 新增一个 bool

```rust
pub struct ModelEntry {
    ...
    pub capabilities: Vec<String>,
    /// 用户手动改过能力清单：`true` 时 `/api/tags` 与静态表都不再覆盖它
    #[serde(default)]
    pub capabilities_pinned: bool,
}
```

`#[serde(default)]`：老配置没有这个字段 → `false`，行为与今天一致（可被自动刷新）。

### 4.2 为什么是 bool 而不是 Option（§3.2-E）

`Option<Vec<String>>` 的 `Some` 无法区分三种来源：

| 来源 | 是否用户改过 |
|---|---|
| Ollama `/api/tags` 合并（`models.rs:111-126`） | 否 |
| 静态表推断 | 否 |
| 用户在界面勾选 | **是** |

`bool` 直接表达"用户锁定没锁定"，且与字段值是正交的两件事。

### 4.3 改动点

| 位置 | 改动 |
|---|---|
| `models.rs:111-126` | 合并时 `if !entry.capabilities_pinned` 才覆盖 `capabilities` |
| `config.rs` | 加字段 + `Default` 里 `false` |

`StoredBlock` / 会话落盘格式不受影响。

---

## 5. 交互流程

### 5.1 看能力

1. 设置 · AI · 模型行 → 能力 chip 一行：`对话补全` `工具调用` `思考`
2. 鼠标悬停某个 chip → 显示该项的含义与用途
3. Ollama 模型：chip 来自 `/api/tags`；OpenAI 兼容模型：来自静态表推断

### 5.2 手动改

1. 点能力行末尾的「编辑」
2. 下拉列出**所有已知能力**，已选的带勾选标记
3. 点一下切换（选中 / 取消）
4. 改完 `capabilities_pinned = true`，此后静态表与 `/api/tags` 都不再覆盖

### 5.3 未知

静态表没匹配上（或 `capabilities` 为空）→ 显示一枚 `未知` chip，
悬停说明提示"可点「编辑」手动指定"。不留白。

---

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| `/api/tags` 返回未知能力词 | 字典查不到 → 回落"未知能力" chip，值原样保留在配置里，不丢弃 |
| 空字符串能力项 | 同"未知"，不 panic |
| DeepSeek 新出模型、表里没有 | 按 §3.3 第 5 档 → 空 → 显示"未知"，用户可手动加 |
| 用户手动删掉所有能力 | `capabilities` 为空但 `pinned = true` → 显示"未知"，且**不会被自动填回** |
| Ollama 重新拉取（用户已 pinned） | `models.rs` 跳过覆盖，用户的清单保留 |
| 能力项很多 | chip 行 `flex_wrap` 换行，不撑爆模型行 |
| 给 DeepSeek 标了 tools 但没跑连接测试 | 文案仍是"工具调用待实测"，agent loop **不会**启用工具阶段（判据是 `tools_enabled`） |

---

## 7. 测试计划

### 7.1 能自动化

| # | 用例 | 位置 |
|---|---|---|
| T-1 | `infer()`：`deepseek-reasoner` 含 `thinking`；`deepseek-chat` 含 `tools`；未知域名 + 未知模型名 → 空；Ollama 侧不被调用 | `tests/core/ai/capability.rs`（新，需 `#[path]` 挂载） |
| T-2 | `info()`：已知 id 给中文标签与说明；未知 id 与空串回落"未知能力"且不 panic | 同上 |
| T-3 | `capabilities_pinned = true` 时 `/api/tags` 合并**不覆盖**；`false` 时覆盖 | `tests/core/ai/models.rs` |
| T-4 | `capability_summary()` 中文摘要（上下文 / 工具实测结论 / 停用），既有断言按新文案更新 | `tests/ui/settings.rs` |
| T-5 | 既有测试全绿 | 全仓 |

### 7.2 要人工（登记进 `docs/ACCEPTANCE.md`）

AC-6 / AC-7 / AC-8 / AC-9 / AC-10 / AC-11（见需求 §5）——涉及真实端点与肉眼确认。

---

## 8. 人工验收清单

1. 手填一个 DeepSeek 模型（`deepseek-chat`）→ 模型行能看到能力清单，不再是空白
2. 悬停某个能力 chip → 看到含义与用途说明
3. Ollama 模型的能力显示为中文，不再出现 `completion / vision / tools` 原始串
4. 点「编辑」能增删能力，改完重启仍在
5. 改过之后重新「从 Ollama 拉取」，手动清单不被覆盖
6. 手填一个冷门模型名 → 显示"未知"，不是留白
7. **行为未变**：给 DeepSeek 标了 tools 但没跑连接测试，不会启用工具阶段

---

## 9. 风险与开放问题

| # | 风险 | 处置 |
|---|---|---|
| R1 | 静态表是猜的，可能给错 | 它只是默认值，用户可改（Q1）；运行时判据仍是探针实测。说明文案里写明"以连接测试为准" |
| R2 | 静态表要持续维护，容易腐化 | 表里只放主流厂商；匹配不上就"未知"，不做强行猜测。表本身集中在一个函数里 |
| R3 | `capability_line()` 的既有断言会挂 | 设计上把它拆成 `capability_summary()`（可测的中文摘要）与 chip 渲染，测试改测前者 |
| R4 | chip 行在窄面板下可能挤 | `flex_wrap` 换行；能力项本来也不多（≤6） |

开放问题：

- **O1**：静态表要不要覆盖更多厂商？当前只列 DeepSeek / 智谱 / Moonshot / 通义 / OpenAI 五家 + 名字特征词。其余走"未知"
- **O2**：`vision` 本项目非目标，要不要在字典里保留？——保留但注明"当前未使用"，避免厂商报了这个值却显示"未知"

---

## 10. 实施步骤

| 期 | 内容 | 涉及文件 | 关联 | 验收 |
|---|---|---|---|---|
| **P1** | 新模块 `core/ai/capability.rs`：能力字典 + `infer()` 纯函数 | `src/core/ai/capability.rs`（新）、`src/core/ai/mod.rs`（挂 mod） | R-1 / R-3 | T-1 / T-2 / T-5 + G4 四连 |
| **P2** | `ModelEntry` 加 `capabilities_pinned`；`models.rs` 合并时尊重它 | `src/core/config.rs`、`src/core/ai/models.rs` | R-4 | T-3 / T-5 + G4 四连 |
| **P3** | UI：能力 chip 行（悬停看说明）+ 「编辑」勾选菜单；`capability_line` 拆出可测的 `capability_summary` | `src/ui/settings.rs` | R-2 / R-5 | T-4 / T-5 + G4 四连 + 人工 AC-6…AC-11 |
| **P4** | 状态回写 + 验收登记 | `docs/` | — | G6 |

每期可独立编译验证，不存在"全做完才能编"的任务。
