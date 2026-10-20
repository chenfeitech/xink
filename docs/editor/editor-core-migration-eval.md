# 编辑器内核切换评估：`show_line_numbers` 与 `indent.*` 的实现路径

> 状态：**Spike 已执行完毕；后续决策：采纳内核升级并已实施**（2026-09-24）
> - Spike 结论：缩进无需换内核即可接线（已落地）
> - **后续决策：采纳内核升级**（`Textarea` → `EditorState`）。原因是拼写检查所需的诊断能力
>   同样只在 `EditorState`（见 [`editor-features-analysis.md`](./editor-features-analysis.md) §1.2），
>   一次升级可同时解锁行号与诊断，比分别推进更划算
> - **注（2026-10-20）**：拼写检查已决定**不做**（见 `editor-features-analysis.md` §4），
>   "解锁诊断"不再作为推进理由；但内核升级本身的收益依然成立——行号、语法高亮、
>   缩进、搜索、代码动作等都只在 `EditorState` 上可用
> - 现状：已实施，`cargo build` 通过、`cargo test` 85 passed，待 GUI 人工验收
> Spike 结果见 [§9](#9-spike-执行结果2026-09-24)，实施结果见 [§10](#10-实施结果2026-09-24)
> 关联：[`config-wiring-design.md`](../config/config-wiring-design.md) §4 #9（行号）、#8（缩进）、§9 Q1/Q3
> 涉及：`src/ui/editor.rs`（编辑面板全部逻辑）、`src/core/config.rs`、`src/ui/preview.rs`（滚动同步）

---

## 1. 结论（先给答案）

| 问题 | 结论 |
|---|---|
| 是否单独立项？ | **是**（已立项并执行）。它不是"配置项接线"，而是**编辑器内核替换**，触及 P0 的编辑与保存主链路，且影响 P1（滚动同步）、P3（行号/缩进）、P4（编辑模式） |
| 直接推进替换，还是先做原型？ | **先做原型 —— 事后证明这个选择是对的**：原型发现"缩进根本不需要换内核"，从而避免了一次不必要的高风险替换 |
| 最终建议 | Spike 当时的结论是"不为行号**单独**换内核"；**2026-09-24 决策升级为采纳内核升级**——拼写检查的诊断能力同样只在 `EditorState`，一次升级同时解锁行号与诊断（拼写检查的展示层）。现已实施，见 §10 |

---

## 2. 框架核实结果（决定了这次评估的结论）

对 `gpui-base-0.6.4` / `gpui-component-0.6.4` 源码核实后，有三个**改变判断**的事实：

### 事实 1：行号只在 `EditorState` 上暴露，Textarea 拿不到（编译期）

```9218:9235:/Users/helightxu/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.6.4/src/input/base/state.rs
/// Methods that only a source-code editor offers.
impl InputBaseState<crate::input::EditorMode> {
    /// Create a source-code editor state.
    ///
    /// Default options: line numbers on, tab size 2 with soft tabs, indent
    /// guides on, multi-line, and search enabled. Set the language for syntax
    /// highlighting with [`Self::language`]; without one the text is shown
    /// unhighlighted.
    ///
    /// The editor aims at simple code editing or display, not at being a
    /// full-featured code editor. It offers syntax highlighting, auto indent,
    /// line numbers, and handles large text up to about 50K lines.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
```

- `line_number()` / `set_line_number()` 定义在 `state.rs:9322` / `:9330`，**属于上面这个 `impl InputBaseState<EditorMode>` 块**；
- `gpui-base-0.6.4/src/input/base/kind.rs:14` 的注释明确："`TextareaState` has no `masked` or `line_number`"，且"Reaching for the wrong one is a **compile error**"。

→ 结论：**不换内核就拿不到行号**，原"方案 B 自研 gutter"是唯一的非替换路径。

### 事实 2：`Editor` 与 `Textarea` 是同构封装，UI 侧替换可能只有一行

`gpui-component-0.6.4/src/input/editor.rs` 与 `textarea.rs` 的公开 API **几乎逐条对应**
（`new / h / appearance / bordered / disabled / readonly / tab_index / role / aria_label / context_menu / on_paste`）。

这与 `kind.rs` 的架构说明一致：三者是同一引擎 `InputBaseState<Mode>` 的不同 mode 标记。

→ UI 侧大概率只需 `Textarea::new(&s)` → `Editor::new(&s)`；**真正的成本在状态类型与行为差异，不在组件写法**。

### 事实 3：Editor 自带缩进能力，但公開 setter 未确认存在

- EditorState 默认 "**tab size 2 with soft tabs**"（`state.rs:9222`）；
- 内部确有 tab 逻辑（`state.rs:1555/1566/1955` 使用 `self.mode.tab_size()`）；
- 但在 `base/state.rs` 与 `input/*.rs` 中**未搜到公开的 `set_tab_size` / `set_indent`**。

→ 换内核**可能**顺带解锁 `indent.*` 两项配置（收益从 1 项变 3 项），但**不能在评估阶段假定**，必须由原型确认。

---

## 3. 方案对比

| 方案 | 内容 | 成本 | 主要风险 | 依赖 |
|---|---|---|---|---|
| **A. 静态评估后直接全量替换** | 读框架代码写出评估报告 → 直接改 `editor.rs` | 中 | **评估结论不可验证**：同构 API 只是"看起来一样"，事件/选区/滚动/性能差异必须跑起来才知道；失败时编辑保存主链路已被动过，回退成本高 | 工具链 1.98.1 |
| **B. 时间盒原型（推荐）** | 新建分支做最小替换，跑 7 条验收清单 → 据事实决策 | 低（约 1 天） | 原型"跑通但没测到关键点"（用验收清单规避）；原型与生产代码可能漂移（原型结论有效期有限） | 工具链 1.98.1 |
| **C. 不换内核，自研 gutter** | 在 Textarea 左侧叠加行号列，用现有 `scroll_offset()` 对齐 | 中高 | **软换行致命**：`Textarea` 开启 soft wrap 后，一个逻辑行占多个视觉行，行号对齐需 display_map 信息，该层拿不到 → 错位；关闭 wrap 则编辑体验变差 | 需先验证 soft wrap 行为 |
| **D. 降级：移除配置项** | 从模型与界面移除 `show_line_numbers` | 极低 | 功能缺失（写长文/改 markdown 时行号有用）；若日后仍需，加回成本 5 行 | 无 |

---

## 4. 推荐"先原型"的决策依据

1. **影响面在核心链路**：`editor.rs` 中内核相关调用点至少 9 处
   （`TextareaState::new`:50、`on_change` 订阅:55、`set_value`:176/213/384、`scroll_offset`:247、`set_scroll_offset`:306、`Textarea::new`:665，以及 4 个工具栏函数签名 `:863/:887/:898/:913` 的 `&mut TextareaState`）。
   静态评估无法可靠估计这些点在新内核下的语义差异。
2. **收益不确定**：是否顺带解决 `indent.*`（事实 3）直接决定 P3 的优先级排序，必须实测。
3. **风险不对称**：原型失败代价 = 1 天 + 一个废弃分支；全量替换失败代价 = P0 编辑保存功能回退。
4. **决策门槛可客观化**：原型通过与否由 §6 的 7 条清单判定，不是主观判断。

---

## 5. 风险与依赖

### 5.1 风险清单

| # | 风险 | 说明 | 缓解 |
|---|---|---|---|
| R1 | 编辑手感改变 | Editor 默认 **自动缩进 + 括号自动补全（auto_close 默认 true）+ 缩进参考线 + tab size 2**，对 Markdown 写作可能是干扰 | 原型中逐项关闭并评估；必要时构造时显式配置 |
| R2 | API 语义差异 | `set_value` / `scroll_offset` / `set_scroll_offset` / `InputEvent` 在 `EditorState` 上是否等价存在 | 原型第 1–5 条验收覆盖 |
| R3 | 保存链路回归 | 防抖保存（`editor.rs:320-347`）与外部变更重载（`:384`）依赖 change 事件语义；若 Editor 的 change 事件触发时机不同，可能出现"误判为用户编辑"或"漏保存" | 原型第 2、3 条验收覆盖 |
| R4 | 滚动同步回归 | 预览双向同步基于 `scroll_offset` 帧轮询；换内核后需重验 | 原型第 5 条验收覆盖 |
| R5 | 大文件性能 | Editor 文档自述"handles large text up to about **50K lines**"，超大笔记表现未知 | 原型第 7 条烟测 |
| R6 | 配置默认值冲突 | Editor 默认 tab size 2、软制表；当前配置默认 `use_tabs=false, tab_size=4`（`config.rs:220-224`）。若接线，默认值需重新定义并同步 `sanitize` | 在决策门阶段一并处理 |
| R7 | 语法高亮双份开销 | 若给 Editor 设 `language("markdown")`，则编辑区（tree-sitter）+ 预览区（syntect）两套高亮并存 | 原型可不设 language（不高亮），作为后续可选 |
| R8 | **工具链阻塞** | 当前 `rustc 1.87 < 项目要求 1.98`，**原型与一切代码改动都跑不了** | 前置：修 `~/.rustup` 属主 → 装 1.98.1 |

### 5.2 依赖关系

```
[硬前置] 工具链 1.98.1（rustup 修复）
        │
        ▼
   Spike：编辑器内核原型（本评估，1 天）
        │
        ├─ 通过 ──► P3：接线 show_line_numbers +（可能）indent.*
        │              └─► 同步修订 config 默认值与 sanitize（R6）
        │
        ├─ 不通过 ─► 行号降级：D（移除配置）或 C（自研 gutter，需先验证 soft wrap）
        │
        └─ 无论成败 ─► 产出「编辑器内核能力矩阵」
                           ├─► 编辑形态演进（若将来做所见即所得，必然基于 Editor 非 Textarea；
                           │   原 default_mode 配置已于 2026-09-24 移除）
                           └─► 未来大纲 / 双链等编辑区特性
```

**并行性**：Spike 不阻塞 P1（`font.weight`、`code_theme`、`link_open`、`sync_scroll` 编辑器侧判断）与 P2（i18n、keymap），可并行推进。

---

## 6. 原型（Spike）验收清单

范围：新建分支，把 `src/ui/editor.rs` 的 `TextareaState` → `EditorState`、`Textarea` → `Editor`，不改其它模块。

| # | 验收项 | 通过标准 |
|---|---|---|
| 1 | 打开笔记 | 内容正确显示，光标位置正常 |
| 2 | 编辑 + 防抖保存 | 输入后按 `auto_save_delay_ms` 写盘；磁盘文件内容与编辑区一致 |
| 3 | 外部变更重载 | 外部改文件：未改动则自动重载；有未保存改动则提示（现有逻辑不被破坏） |
| 4 | 工具栏 13 个按钮 | 全部可用（4 个函数签名改为 `&mut EditorState` 后编译通过且行为一致） |
| 5 | 滚动同步 | 编辑 ↔ 预览双向同步仍工作（`sync_scroll` 开关有效） |
| 6 | **行号** | `set_line_number(true)` 显示、`false` 隐藏，且由 `config.editor.show_line_numbers` 驱动 |
| 7 | 大文件烟测 | ≥ 5000 行笔记：打开、滚动、输入无明显卡顿 |

附加记录（不阻断，但写入结论）：`set_tab_size` 类 API 是否存在 → 决定 `indent.*` 能否一并接线。

---

## 7. 决策门

| 原型结果 | 处置 |
|---|---|
| 7 条全通过 | 采纳替换：`show_line_numbers` 在 P3 接线；若 `indent.*` setter 存在则一并接线；按 R6 修订配置默认值 |
| 1–5、7 通过，6 失败 | 行号仍不可用 → 走 D（移除配置），编辑内核是否保留取决于手感评估 |
| 核心项（2/3/4/5）任一失败 | **不替换**，行号降级为 D；把发现的问题记入能力矩阵，供 P4 参考 |
| 性能（7）失败 | 不替换（笔记应用不能接受大文件卡顿） |

---

## 8. 与既有文档的关系

- 更新 [`config-wiring-design.md`](../config/config-wiring-design.md) §4 #9：`show_line_numbers` 的处置**前置于本 Spike**，不再在 P3 内直接决定；
- 本 Spike 的结论同时是"编辑形态演进"的技术前提——若将来做所见即所得，必然基于 `Editor` 而非 `Textarea`
  （原 `default_mode` 配置项已于 2026-09-24 移除）；
- 行号/缩进两项配置的最终去向（接线 or 移除）在决策门后回填到 `config-wiring-design.md` 的接线总表。

---

## 9. Spike 执行结果（2026-09-24）

### 9.1 验证环境

| 项 | 值 |
|---|---|
| 工具链 | `rustc/cargo 1.98.1`（rustup default 1.98.1） |
| 基线 | `cargo build` 通过；`cargo test` **85 passed / 0 failed** |
| 分支 | `spike/editor-core` |

### 9.2 两方案的验证结果

| 验证项 | 方案 A：换 `EditorState` | 方案 B：保留 `Textarea` 接线缩进 |
|---|---|---|
| 编译 | ✅ 通过（12 处替换，API 全部同名可用） | ✅ 通过 |
| 单元测试 | ✅ 85 passed | ✅ 85 passed |
| 行号 `set_line_number` | ✅ API 可用（`state.rs:9330`） | ❌ 编译期不可达（与预期一致） |
| 缩进 `set_tab_size` | ✅ 可用 | ✅ **可用 —— 无需换内核** |
| GUI 行为 | ⏳ 需人工 | ⏳ 需人工 |

> 方案 A 的编译通过还顺带证明了：`set_value` / `scroll_offset` / `set_scroll_offset` /
> `selected_range` / `selected_text` / `replace` / `insert` / `cursor` / `cursor_position` /
> `set_cursor_position` / `text()` 在 `EditorState` 上**同名同签名存在**，
> 即风险 R2（API 差异）的编译层面已被排除——这条对将来若要真换内核很有价值。

### 9.3 关键发现：缩进不需要换内核

`set_tab_size` 与 `tab_size` 定义在

```492:504:/Users/helightxu/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.6.4/src/input/editor/indent.rs
impl<M: crate::input::MultiLineMode> InputBaseState<M> {
    /// Set the tab size for the input.
    #[doc(hidden)]
    pub fn tab_size(mut self, tab: TabSize) -> Self {
```

而 `gpui-base-0.6.4/src/input/base/kind.rs:65` 有 `impl MultiLineMode for TextareaMode {}`，
因此 **`TextareaState` 同样可以设置 `TabSize { tab_size, hard_tabs }`**，
正好一一对应配置里的 `indent.tab_size` 与 `indent.use_tabs`。

**这改变了决策**：换内核的收益从"行号 + 缩进 2 项"缩回"仅行号 1 项"，
而成本（P0 主链路替换 + 手感风险 R1 + 大文件风险 R5）不变 → 收益/成本比显著恶化。

### 9.4 建议结论

1. ~~本次不为行号更换编辑内核~~ → **2026-09-24 决策采纳内核升级并已实施**（见 §10）；
2. **缩进两项现已可接线**，且已落地（含改动即时生效）；
3. **行号**：随内核升级**已接线**（构造期按配置设置 + `render` 增量下发），待人工验收；
4. 若产品上坚持"现在就要行号"，再走方案 A，但须先完成 §6 的 GUI 验收（尤其 R1 手感与 R5 大文件）。

### 9.5 分支上已落地的改动（`spike/editor-core`）

`src/ui/editor.rs`：

- 新增字段 `applied_indent: IndentConfig`（记录已下发的缩进，用于变更检测）；
- `create()`：构造后首次下发 `TabSize`；
- `render()`：比较配置与 `applied_indent`，变化时增量下发 `set_tab_size` → **缩进改动 L1 立即生效**，
  不需要重开标签，也不会每帧重复下发（守卫字段）；
- `cargo build` 通过、`cargo test` 85 passed。

### 9.6 仍需人工（GUI）验证

| # | 验证项 | 预期 |
|---|---|---|
| 1 | 设置里改"缩进宽度 4 → 8"，回到编辑器按 Tab | 插入 8 个空格，**且改完即生效，无需重开标签** |
| 2 | 勾选"使用 Tab 字符"后按 Tab | 插入 `\t` |
| 3 | 打开笔记 / 编辑 / 自动保存 / 外部变更重载 | 无回归 |
| 4 | 编辑 ↔ 预览滚动同步 | 无回归 |
| 5 | ≥ 5000 行笔记打开与滚动 | 无明显卡顿 |
| 6 | 行号开关：设置里打开 → 编辑区立即显示行号；关闭 → 隐藏 | 即时生效，无需重开标签 |
| 7 | Markdown 写作手感 | 自动补全/自动缩进已显式关闭；输入引号与括号不应被自动补全 |
| 8 | 打开/编辑/保存/滚动同步/大文件（≥5000 行）无回归 | 与切换前一致 |

---

## 10. 实施结果（2026-09-24）

分支 `feature/editor-core`，改动集中在 `src/ui/editor.rs`：

| 改动 | 说明 |
|---|---|
| 内核切换 | `TextareaState` → `EditorState`、`Textarea` → `Editor`；字段 `textarea` 改名为 `editor`（共约 16 处，批量替换） |
| 行号接线 | 构造期 `.line_number(cfg.editor.show_line_numbers)`；`render` 中用 `applied_line_numbers` 守卫检测变更 → `set_line_number(...)` 增量下发，**开关即时生效** |
| 缩进接线（保留） | `create` 首次下发 + `render` 增量下发（原已实现，继续有效） |
| 关闭干扰项 | 显式 `.auto_close(false)` / `.smart_indent(false)` / `.folding(false)` / `.indent_guides(false)`：Markdown 写作不需要括号自动补全与回车自动缩进；折叠与缩进参考线的交互尚未验证，先关闭 |

验证：`cargo build` 通过、`cargo test` **85 passed / 0 failed**、`cargo clippy` 无 error。

**仍需人工验收**：§9.6 第 6–8 条（行号开关、写作手感、保存与滚动同步无回归、大文件）。
~~这批验证通过后，可继续推进拼写检查~~——拼写检查已于 2026-10-20 **决定不做**
（见 `editor-features-analysis.md` §4），"诊断能力已解锁"不再作为推进理由。
