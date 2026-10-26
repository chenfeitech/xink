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

## 6. 多轮：为什么"只看到一轮"（2026-10-26 追加）

排查结论（用户报告"其余轮次没显示"）：**数据没丢，是看不见 + 会话归属**。

| # | 原因 | 处置 |
|---|---|---|
| A | 对话区钉死 180px 且**没有自动滚到底**，旧轮次在视口外 | ① 高度 180 → 260；② 内容长度变化时 `ScrollHandle::scroll_to_bottom()`（只在变化时调，用户手动上滚不会被拽回） |
| B | `AiChat` 挂在 `EditorPanel`，而 `OpenTabs.panels` 是 **按笔记路径** 一个面板：切笔记 = 另一个（空的）会话；关标签 = 会话销毁；重启 = 全丢 | 会话**按笔记归属并落盘**（见 §6.1） |
| C | `finish()` 先压 assistant 后压 user → 发给模型的顺序是 `[a1,q1,a2,q2]`，跨轮错序 | `send()` 开始时压 user、`finish()` 只压 assistant；动作模式（一次性改写）不进历史 |
| D | `pending: Option<PendingEdit>` 是单值，下一轮直接覆盖 → **上一轮的 diff 消失** | 每轮 `finish` 时把该轮 diff 固化成 `Block::Diff { lines, collapsed, applied }`；`pending` 只服务"当前待应用" |
| E | diff 默认展开 + 工具结果长 → 占满可视区，把更早的对话挤出去 | `Block::Diff` 默认折叠，标题保留 `+A -B` |
| F | **展开后内容滚不动**：① 外层用的是组件库 `overflow_y_scrollbar()`，它把元素包进一层 `size_full()` 的滚动壳、并**覆盖调用方的 id 与 `track_scroll`**（滚动句柄由它按 id 自管），我们自己那个 `ScrollHandle` 因此完全无效；② 展开的长正文把对话区撑高，外层那 260px 再怎么滚也看不全 | ① 对话区改回 gpui 原生 `overflow_y_scroll()` + `track_scroll(&self.history_scroll)`，滚谁由自己说了算；② **每个展开块自带滚动盒**：`max_h(220) + min_h_0 + overflow_y_scroll`，块内垂直滚动，与外层互不影响 |

### 6.1 会话持久化

- 归属：**按笔记**（面板即笔记、系统提示里带的是当前笔记正文，语义上会话就是"关于这篇的对话"）。
- 位置：`state/ai_sessions.rs`，单文件 `app_data_dir()/ai_sessions.json`，
  `HashMap<PathBuf, Vec<StoredBlock>>`（`StoredBlock` 是 `Block` 的可序列化镜像，
  折叠状态不落盘，加载后按 §3.1 的默认值重建）。
- 上限：30 个会话 × 每个 200 块，超出丢最旧的——文件不能无限长。
- 清空对话条 = 删除该笔记的会话；写盘失败只记 `error`，不影响对话本身。
- 面板被复用去打开另一篇笔记时（`capture` 拿到不同路径）重新载入对应会话。

## 7. 测试与验收

- 自动化：`tests/core/ai/agent_loop.rs` 已有用例断言 `ToolResult` 事件出现（用了 `..`，
  加字段不影响）；补一条断言 `content` 非空。`tests/state/ai_sessions.rs` 覆盖裁剪与读写。
- 人工验收：
  - [ ] 拖选 AI 回复的一段文字 → `Cmd-C` 能粘出来（含代码块）
  - [ ] 点 AI 回复里的代码块右上角 Copy → 粘出的是代码原文
  - [ ] 工具行默认只显示 `⚙ … → N 行 / M 字符`，点一下展开看到完整结果，再点收起
  - [ ] 校验失败块默认收起，展开能看到具体问题
  - [ ] 用户消息与 AI 正文可分别折叠
  - [ ] 流式过程中正文正常滚动、不被折叠
