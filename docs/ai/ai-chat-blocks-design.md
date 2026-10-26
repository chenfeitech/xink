# 设计：AI 对话条 —— 可选中复制 + 交互过程分块折叠

> 需求（用户 2026-10-26）：① AI 回复的文字要能选中复制；② 完整的交互过程按逻辑分块，
> 每块可折叠 / 展开。

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 对话正文全部是 `div().child(String)`，gpui 里 `div` 里的文本**不可选中** | `src/ui/ai.rs:871-880`（`format!("AI：{}", turn.text)`） |
| 工具行只留了摘要 `⚙ name(args) → N 行 / M 字符`，**结果正文没传给 UI** | `src/core/ai/agent_loop.rs:60-64`（`LoopEvent::ToolResult` 只有 `lines` / `chars`）；`src/ui/ai.rs:618-631` |
| 一轮对话 = 两条 `Turn`（我 / AI），工具行也塞在同一个 `Vec` 里，靠 `tool: Option<String>` 区分 | `src/ui/ai.rs:149-155` |
| diff 直接在 `render` 里逐行铺开，**没有折叠状态**（每帧重算，也没有可收起的地方） | `src/ui/ai.rs:882-896` |
| 框架已提供可选中 + 可复制的文本元素：`TextView`（`selectable` 默认开、`cmd-c` 由 `gpui_base::root` 绑定、代码块自带 Copy 按钮） | `gpui-component-0.8.0/src/text/compat.rs:68/95`；`gpui-base-0.8.0/src/root.rs:42`、`text/text_view.rs:735/1809` |
| `TextView` 只有 Markdown / HTML 两种格式，没有纯文本 | `gpui-base-0.8.0/src/text/state.rs:63-68` |

## 2. 目标 / 非目标

**目标**

- G1：AI 回复（以及用户消息、工具结果）能用鼠标选中、`Cmd-C` 复制。
- G2：一轮交互拆成逻辑块（我 / AI 正文 / 工具调用 / 校验 / diff），每块一行标题 + 可折叠正文。
- G3：过程性信息（工具结果、校验细节）默认收起，只留一行摘要；要看细节再展开。

**非目标**

- 不做右键菜单与"复制整轮"按钮（框架自带的选择 + `Cmd-C` 已够用）。
- 不改 `pending` / 应用 / 快照这条链路，也不改模型与 provider。
- 不做会话持久化（对话仍是内存态）。

## 3. 方案

### 3.1 数据模型：`Turn` → `Block`

```rust
enum Block {
    /// 用户说的话
    User { text: String, collapsed: bool },
    /// AI 正文（流式追加）
    Assistant { text: String, collapsed: bool },
    /// 一次工具调用：标题是摘要，正文是完整结果
    Tool { title: String, output: String, collapsed: bool },
    /// 结构校验没过，已回灌让模型重写
    Verify { detail: String, collapsed: bool },
}
```

- `collapsed` 落在块上，`Vec<Block>` 仍然是一条时间线（顺序 = 发生顺序）。
- 默认折叠：`Tool` / `Verify` 为 `true`（过程噪音），`User` / `Assistant` 为 `false`。
- diff 不进 `Block`：它每帧由 `pending` 重算，折叠状态放 `AiChat::diff_collapsed`。

### 3.2 复制：正文改用 `TextView`

- AI 正文 / 用户消息：`TextView::markdown(id, text).selectable(true)`。
- 工具结果：包进 ``` 围栏按**代码块**渲染——保留换行与缩进，复制得到的是原文，
  且框架自带代码块 Copy 按钮。围栏长度按内容里最长的反引号串 +1，避免被内容截断。
- 标题行（"我" / "AI" / "⚙ …"）仍是 `div`，短、不需要复制。

**取舍**：`TextView` 只有 Markdown / HTML 两种格式（§1），所以用户输入与工具结果也走
Markdown 解析。用户消息里的 `#`、`*` 会被当成 Markdown 渲染——对口语输入几乎无影响，
换来的是"整段都能选中复制"。工具结果走代码块，不受影响。

### 3.3 core 改动：`LoopEvent::ToolResult` 带上正文

```rust
ToolResult { name: String, lines: usize, chars: usize, content: String }
```

没有 `content` 就无处"展开看细节"——工具层已经按 `max_tool_chars` 截断过，
UI 只负责显示，不再二次截断。

### 3.4 交互

- 点标题行 → 切换该块的 `collapsed`（标题左侧 ▸ / ▾ 指示）。
- 流式期间正文块自动展开（否则看不到正在生成的内容）。
- "清空"清掉所有块。

## 4. 异常与边界

| 场景 | 处理 |
|---|---|
| 工具结果为空 / 工具执行失败（工具层返回错误文本） | 照常显示，`output` 是错误文本，展开能看到原因 |
| 结果超长 | 工具层已按 `max_tool_chars` 截断；UI 不二次截断，靠折叠 + 对话区滚动 |
| 工具结果里有 ``` | 围栏长度按内容自适应（§3.2） |
| 折叠状态下又收到同一块的流式增量 | 只在"新的正文块"上追加；流式块不会被折叠 |
| `TextView` 解析失败 | 框架内部有 `parsed_error`，不会 panic |

## 5. 测试与验收

- 自动化：`tests/core/ai/agent_loop.rs` 已有用例断言 `ToolResult` 事件出现（用了 `..`，
  加字段不影响）；补一条断言 `content` 非空。
- 人工验收：
  - [ ] 拖选 AI 回复的一段文字 → `Cmd-C` 能粘出来（含代码块）
  - [ ] 点 AI 回复里的代码块右上角 Copy → 粘出的是代码原文
  - [ ] 工具行默认只显示 `⚙ … → N 行 / M 字符`，点一下展开看到完整结果，再点收起
  - [ ] 校验失败块默认收起，展开能看到具体问题
  - [ ] 用户消息与 AI 正文可分别折叠
  - [ ] 流式过程中正文正常滚动、不被折叠
