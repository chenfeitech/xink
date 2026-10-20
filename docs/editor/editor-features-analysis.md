# 编辑器三项配置功能：可行性与实现方案分析

> 状态：**决策已定：立项「编辑器内核升级」并已实施**（2026-09-24）
> 第 1 项（Tab 缩进）、第 2 项（行号）已完成接线
> **2026-10-20 更新**：第 3 项（拼写检查）**已决定不做**——框架无拼写能力、自研需新依赖与词典，
> 且中文场景价值低。配置项 `editor.spell_check` 已从数据模型与设置界面移除（见 §4）
> 范围：① 使用 Tab 缩进（含缩进宽度） ② 显示行号 ③ 拼写检查
> 关联：[`config-wiring-design.md`](../config/config-wiring-design.md)、[`editor-core-migration-eval.md`](./editor-core-migration-eval.md)
> 所有结论均基于 `gpui-base-0.6.4` / `gpui-component-0.6.4` 源码核实，并标注了证据位置

---

## 1. 结论摘要

### 1.1 可行性列表

| # | 功能 | 可行性 | 是否需要更换编辑内核 | 实现代价 | 现状 |
|---|---|---|---|---|---|
| 1 | **Tab 缩进（含宽度）** | ✅ **完全可行** | 否（`Textarea` 即可） | 极低 | **已实现**，待人工验证 |
| 2 | **显示行号** | ✅ 可行 | **是**（需 `EditorState`） | 中 | **已接线**（内核升级完成，2026-09-24），待人工验证 |
| 3 | **拼写检查** | ⚠️ 部分可行 | **是**（诊断机制仅 `EditorState`） | 高 | **已决定不做**（2026-10-20）：引擎需自研、中文价值低，配置项已移除 |

### 1.2 关键洞察：第 2、3 项共享同一个前提

核实发现，**框架的诊断（波浪线/悬浮提示）与代码动作（右键更正建议）都只在 `EditorState` 上可用**：

```314:321:/Users/helightxu/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-base-0.6.4/src/input/base/mode.rs
    pub(super) fn diagnostics(&self) -> Option<&DiagnosticSet> {
        match self {
            LayoutMode::CodeEditor { diagnostics, .. } => Some(diagnostics),
            _ => None,
        }
    }
```

`CodeActionProvider`（`input/editor/lsp/code_actions.rs:8`）的签名也直接收 `Entity<EditorState>`。

**这意味着**：行号与拼写检查不是两个独立任务，而是**同一个前置项（编辑内核升级）的两个产出**。
若两项都要做，换内核的收益从 1 项变为 2 项（外加语法高亮、搜索、折叠等附带能力），
此前"仅为行号不换内核"的结论需要重新权衡——见 §6。

### 1.3 建议优先级

| 优先级 | 事项 | 理由 |
|---|---|---|
| **P1** | Tab 缩进：**完成人工验证并收尾** | 代码已落地，零新增成本 |
| **P2** | **决策：是否立项"编辑器内核升级"** | 它是行号与拼写检查的共同前置，先决策再动手 |
| **P3** | 行号（随内核升级落地） | 换内核后接线成本极低（一次 `set_line_number` 调用） |
| ~~**P4**~~ | ~~拼写检查（先做"仅标记"版）~~ | **已取消**（2026-10-20）：决定不做，配置项已从模型与界面移除 |

---

## 2. 功能一：Tab 缩进（含缩进宽度）

### 2.1 可行部分

| 能力 | API | 位置 |
|---|---|---|
| 缩进宽度 | `TabSize { tab_size: usize, hard_tabs: bool }` | `input/editor/indent.rs:13` |
| 运行时设置 | `set_tab_size(&mut self, tab: TabSize, cx)` | `input/editor/indent.rs:504` |
| 构造期设置 | `tab_size(mut self, tab: TabSize)` | `input/editor/indent.rs:495` |
| **对 `Textarea` 可用** | 二者都定义在 `impl<M: MultiLineMode> InputBaseState<M>`，且 `impl MultiLineMode for TextareaMode` 成立 | `indent.rs:492`、`input/base/kind.rs:65` |
| **Tab 键确实触发缩进** | 全局绑定 `KeyBinding::new("tab", IndentInline, Some("Input"))` | `input/base/state.rs:182`（在 `pub(crate) fn init` 中，对所有输入状态生效） |

配置项 `indent.use_tabs` ↔ `hard_tabs`、`indent.tab_size` ↔ `tab_size` **一一对应**，无缺口。

### 2.2 不可实现部分

无。

### 2.3 实现方案（已完成）

`src/ui/editor.rs`：

- `create()`：构造 `TextareaState` 后首次下发 `TabSize`；
- `render()`：比较 `applied_indent` 与当前配置，变化时增量下发 → **改设置立即生效（L1）**；
- 守卫字段避免每帧重复下发。

改动约 35 行，`cargo build` 通过、`cargo test` 85 passed。

### 2.4 代价

| 维度 | 评价 |
|---|---|
| 开发复杂度 | 极低（已实现） |
| 性能影响 | 无（仅在配置变化时下发一次） |
| 维护成本 | 极低；唯一约束：新增影响行高的改动需同步 `layout` 常量（设置窗口侧） |

### 2.5 待验证

人工验证两条（见 `editor-core-migration-eval.md` §9.6）：改宽度 4→8 后按 Tab 插入 8 空格；勾选"使用 Tab 字符"后插入 `\t`。

---

## 3. 功能二：显示行号

### 3.1 可行部分

| 能力 | API | 位置 |
|---|---|---|
| 构造期开关 | `line_number(mut self, bool)` | `input/base/state.rs:9322` |
| 运行时开关 | `set_line_number(&mut self, bool, &mut Window, &mut Context<Self>)` | `input/base/state.rs:9330` |

二者都在 `impl InputBaseState<EditorMode>` 块内（该块起始于 `state.rs:9219`）。

### 3.2 不可实现部分

**当前 `Textarea` 内核下完全不可实现**：`input/base/kind.rs:14` 明确写着
"`TextareaState` has no `masked` or `line_number`"，取错方法是**编译错误**而非运行时降级。

自绘行号（gutter）的替代路径也不可靠：开启 soft wrap 后一个逻辑行占多个视觉行，
行号对齐需要 `DisplayMap` 层信息，而 `Textarea` 层拿不到。

### 3.3 实现方案

**方案 A（唯一路径）：切换到 `EditorState` 内核**

1. `src/ui/editor.rs`：`TextareaState` → `EditorState`、`Textarea` → `Editor`（12 处）；
2. 构造后按配置调用 `set_line_number(cfg.editor.show_line_numbers, window, cx)`；
3. 按 §3.4 逐项关闭对 Markdown 写作不利的默认行为；
4. 过 §3.5 的验收清单。

> 该替换**已验证可编译、测试全绿**（`spike/editor-core` 分支，12 处替换 + 85 passed）。
> 并且证明了 `set_value` / `scroll_offset` / `selected_range` / `replace` / `insert` / `cursor` 等
> 12 个方法在 `EditorState` 上同名同签名存在——API 差异风险已排除在编译期。

### 3.4 换内核带来的行为差异（必须处理）

| 默认行为 | 来源 | 处理 |
|---|---|---|
| 行号默认开 | `state.rs:9222` | 由配置覆盖即可 |
| **tab size 默认 2（软制表）** | `state.rs:9222` | **已被现有接线抵消**：`create`/`render` 会显式下发配置值 |
| 自动缩进（smart indent） | `set_smart_indent` 存在（`state.rs:9366`） | 建议显式关闭或评估手感 |
| 括号/引号自动补全默认开 | `auto_close`（`state.rs:9343`） | 建议对 Markdown 关闭 |
| 缩进参考线默认开 | `indent_guides`（`indent.rs:170`） | 可选关闭 |
| 大文件上限声明约 50K 行 | `state.rs:9229` | 需烟测 |

### 3.5 代价

| 维度 | 评价 |
|---|---|
| 开发复杂度 | **中**：代码替换本身已验证（约 12 处 + 配置项接线），成本主要在回归验证 |
| 性能影响 | 需实测；`EditorState` 自带语法高亮管线与 `DisplayMap`，长文开销高于 `Textarea` |
| 维护成本 | **中升**：编辑区从"纯文本多行"变为"代码编辑器"语义，后续所有编辑区特性都在此模型上演进 |
| 风险 | 编辑/保存主链路（P0）回归、滚动同步回归、写作手感变化 |

验收清单见 `editor-core-migration-eval.md` §6（7 条）。

---

## 4. 功能三：拼写检查（已决定不做）

> **2026-10-20 决策：不做。**
> - 原因：框架无拼写引擎（§4.2）；自研需新增依赖 + 词典 + 编辑器装饰接入，属**独立特性**
>   而非"配置项"；中文"拼写检查"意义有限（应做错别字/用词检查，属另一个需求）。
> - 已执行：`editor.spell_check` 从 `core/config.rs`（字段 + `Default`）与设置界面
>   `all_fields()` 中移除；旧 `config.json` 里的该字段被 serde 忽略，**无迁移成本**。
> - 下面 §4.1–§4.3 **保留作知识资产**，避免将来重复调研。若日后要重新做，
>   加回字段的成本约 5 行。

### 4.1 可行部分（展示与交互层，框架已具备）

| 能力 | API | 位置 |
|---|---|---|
| 诊断条目 | `Diagnostic::new(range, message)`、`with_severity` | `input/editor/diagnostics.rs:98/106` |
| 诊断集合 | `DiagnosticSet::push / extend / clear / iter` | `diagnostics.rs:204/218/232` |
| 严重级别 | `Hint / Error / Warning / Info` | `diagnostics.rs:77` |
| 修正建议入口 | `trait CodeActionProvider`（`code_actions` / `perform_code_action`） | `input/editor/lsp/code_actions.rs:8-28` |
| 波浪下划线 | `UnderlineStyle`（含 wavy） | `gpui-pre-0.3.5/src/style.rs:835` |

→ **不需要自绘任何装饰**，只要产出 `Diagnostic` 列表交给框架即可。

### 4.2 不可实现部分（框架没有的）

| 缺口 | 说明 |
|---|---|
| **拼写引擎** | 全量搜索 `gpui-*` 依赖：无词典、无 `NSSpellChecker` 绑定、无任何拼写 API。`diagnostics.rs:280` 的 "Spelling mistake" 只是 LSP 诊断的**测试 fixture 字符串** |
| **中文支持** | 即使接入引擎，中文"拼写检查"意义有限（应做错别字/用词检查而非拼写） |
| 在 `Textarea` 上使用 | 诊断与代码动作均限 `EditorState` |

### 4.3 实现方案

**前提**：先完成 §3.3 的内核切换。

**引擎选型（需决策）**

| 方案 | 优点 | 缺点 |
|---|---|---|
| A. `objc` 调 macOS `NSSpellChecker` | 复用系统词典与多语言、无需打包词库、体积小、建议质量高 | 新增 `objc` 依赖、**仅 macOS**（当前项目本就是 macOS 优先）、需 unsafe 桥接 |
| B. Rust crate（`hunspell` / `typo`） | 跨平台、纯 Rust | 需打包词典（体积 + 许可）、中文能力弱、需调校 |

**实现步骤（"仅标记"最小版）**

1. 在编辑面板对文本做**防抖**检查（复用现有 `auto_save_delay_ms` 量级的防抖思路，另设更长间隔）；
2. 分词时**排除**：代码块、行内代码、URL、内联 HTML、`[[wikilink]]`、 front-matter；
3. 对每个疑似错词生成 `Diagnostic::new(range, message).with_severity(Warning)`，`push` 进 `DiagnosticSet`；
4. 文本变更时 `clear()` 后重算（诊断集合随文本 reset，`state.rs:1184/3551` 已有 reset 逻辑）；
5. 进阶：实现 `CodeActionProvider` 提供"替换为…"建议列表。

### 4.4 代价

| 维度 | "仅标记"版 | 含"更正建议"版 |
|---|---|---|
| 开发复杂度 | 中（引擎适配 + 分词 + 防抖 + 排除规则） | 中高（再叠加 `CodeActionProvider` 与交互） |
| 性能影响 | 中：每次防抖全量扫描；长文需改增量（仅检查可视区域/变更行） | 同左 + 建议计算开销 |
| 维护成本 | 中高：排除规则随 Markdown 语法用法演进；误报调优是长期工作 | 高 |

### 4.5 建议

- 先落地"**仅标记、不修正**"，用 `Warning`/`Info` 级别，把代价压到最低；
- 引擎优先选 A（`NSSpellChecker`），与项目 macOS 定位一致且免词库；
- 明确**排除代码块与 URL**，否则误报会淹没真实错误；
- 中文场景建议改用"错别字/术语检查"另行评估，不纳入拼写检查。

---

## 5. 依赖条件

| # | 依赖 | 状态 | 影响 |
|---|---|---|---|
| D1 | Rust 1.98.1 | ✅ 已就绪 | 阻塞一切编译验证 |
| D2 | 第 1 项无额外依赖 | ✅ | — |
| D3 | 第 2、3 项 → 编辑内核升级（`EditorState`） | ⏳ 待决策 | 二者共享，一次升级同时解锁 |
| D4 | 拼写引擎选型（A/B） | ⏳ 待决策 | 决定第 3 项的实现路径与体积 |
| D5 | 第 2、3 项 → GUI 人工验收 | ⏳ 无法自动化 | 编辑手感、滚动同步、大文件必须人工确认 |
| D6 | 内核升级 → 配置默认值对齐 | ⚠️ 注意 | `EditorState` 默认 tab size 2，但现有接线会显式下发配置值，已抵消；`auto_close`/`smart_indent` 需显式设置 |

---

## 6. 潜在限制与风险

| # | 限制 / 风险 | 影响 | 缓解 |
|---|---|---|---|
| L1 | 换内核后写作手感变化（自动补全、自动缩进） | Markdown 写作体验 | 构造时显式关闭，并人工试写验证 |
| L2 | `EditorState` 声明支持到约 50K 行 | 超大笔记 | 烟测 ≥5000 行；必要时限制检查范围 |
| L3 | Tab 与焦点系统的潜在冲突 | Tab 键可能被焦点陷阱截获 | 框架已在 `"Input"` 上下文绑定 `tab → IndentInline`（`state.rs:182`），需人工确认优先级 |
| L4 | 拼写误报（代码块、URL、专有名词） | 用户信任度 | 严格排除规则 + 先只标记不打扰（Hint 级别） |
| L5 | 拼写性能 | 长文卡顿 | 防抖 + 增量（仅可视区/变更行） |
| L6 | 中文拼写无实际能力 | 功能对中文用户价值低 | 明确范围："英文拼写检查"；中文另立需求 |
| L7 | 内核升级是**不可逆的风格选择** | 后续编辑区特性都建立在此模型 | 先做 Spike（已完成编译验证）+ 7 条验收 |

---

## 7. 建议实施路径

```
第 1 步（立即）：人工验证 Tab 缩进两条 → 收尾第 1 项
        │
第 2 步（决策）：是否立项"编辑器内核升级"？
        │
        ├─ 是 ──► 分支实施：Textarea → Editor
        │          ├─ 接线行号（第 2 项完成）
        │          ├─ 过 7 条验收（手感 / 保存 / 同步 / 大文件）
        │          └─ 再评估拼写检查：先"仅标记"（第 3 项）
        │
        └─ 否 ──► 行号与拼写检查均维持 NotWired：
                  行号 = 需内核；拼写 = 需内核 + 引擎
                  （保持现状，不做半成品）
```

**2026-09-24 决策**：采纳内核升级，已实施（分支 `feature/editor-core`，行号已接线、编译与测试通过）。
**2026-10-20 决策**：第 3 项（拼写检查）**不做**，配置项已移除（见 §4）。

**判断依据**：第 1 项零成本已完成；第 2、3 项的共同前置是内核升级，
先做一次决策比分别推进更划算——避免"为行号换一次内核、为拼写再改一次"。

---

## 8. 与既有文档的关系

- `config-wiring-design.md`：第 1 项标记为已接线；第 2、3 项的"待接线"原因在此文档中给出依据；
- `editor-core-migration-eval.md`：内核替换的 Spike 结果与 7 条验收清单，是第 2、3 项的实施基础；
- 决策落地后，需回填 `config-wiring-design.md` 的接线总表（#9 行号、新增拼写检查行）。
