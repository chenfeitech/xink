# AI 交互界面全面优化 — 技术设计

> 状态：**待用户确认（G2 未过）**，确认前不动 `src/`
> 需求：[`ai-ui-overhaul-requirements.md`](./ai-ui-overhaul-requirements.md)（R-1…R-8、AC-1…AC-13）
> 状态文件：[`docs/exec-plans/ai-ui-overhaul.md`](../exec-plans/ai-ui-overhaul.md)
> 分支：`feature/ai-ui-overhaul`

---

## 1. 现状与问题（附证据）

| # | 现状 | 证据 |
|---|---|---|
| S1 | 对话条 `AiChat` 是 `EditorPanel` 内嵌块，非 Dock、非浮层；每个标签一份实例 | `editor.rs:2993-2998`、`main.rs:21-38`（面板注册表无 AI） |
| S2 | 五段结构：顶栏 / 对话区 / 快捷动作 / 输入行 / 状态行，根容器无边框无圆角 | `ai.rs:1373/1445/1637/1665/1760`、`ai.rs:1809-1821` |
| S3 | **AI 区是全项目唯一零 hover 态的交互区** | `.hover(` 全项目 7 处（outline/search/backlink/tags×2/rail/settings），`ai.rs`、`plan.rs` 各 0 处 |
| S4 | **AI 区是全项目唯一手搓颜色的地方**：`bubble_bg()` 用 `hsla()` | `ai.rs:64/66/74-81` |
| S5 | 尺寸写死：`HISTORY_MAX_H 260`、`BODY_MAX_H 220`、`BUBBLE_MAX_W 0.78`、`BUBBLE_RADIUS 12` | `ai.rs:52/57/59/61` |
| S6 | AI 回复气泡无背景 / 无边框 / 无角色标识，只靠对齐区分 | `ai.rs:1512-1514` |
| S7 | `AiMode`（Chat/Action/Skill）无切换控件 | `ai.rs:190-200`，渲染里无出口 |
| S8 | 输入是**单行** `Input`，不是 `Textarea` | `ai.rs:1669`、`ai.rs:473` |
| S9 | 状态行是纯文本堆叠（耗时 / tokens / 工具次数 / 错误），无图标无分隔 | `ai.rs:1760-1807` |
| S10 | 命令面板条目无图标、无键位；全项目无 `kbd` 实现 | `command.rs:259-278`；键位只有 `tooltip_with_action`（`editor.rs:2627-2631`） |
| S11 | 对话条不可调高 | `ai.rs:1448` `max_h(px(260))` |
| S12 | 评审卡片有边框无 hover，圆角写死 `px(6.)` | `editor.rs:2197-2206`、`editor.rs:2204` |
| S13 | `plan.rs` 条目 `rounded(radius)` 但**无底色**，看起来不是卡片 | `plan.rs:505` |
| S14 | AI 按钮没有"对话条已打开"的选中态 | `editor.rs:2623-2634` |
| S15 | 开关 AI 条的 `eprintln!` 调试残留 | `editor.rs:1676-1679` |

---

## 2. 目标 / 非目标

**目标**（对应 R-1…R-8）

| # | 目标 |
|---|---|
| D1 | 建立全局视觉规范（间距 / 圆角 / 字号 / 语义色 / 图标 / hover 口径），消除魔法数字与手搓颜色（R-1、R-7） |
| D2 | 对话区保持内嵌条形态，但层级与分隔清晰、可拖拽调高（R-2） |
| D3 | 气泡重做：角色标识 + 双方容器背景 + 圆角跟主题 + hover + 流式指示（R-3） |
| D4 | 输入区重做：多行输入 + 模式切换 UI + 快捷动作当前模式高亮（R-4） |
| D5 | 状态行分区（图标 + 分隔 + 错误用 `danger`）（R-5） |
| D6 | 交互细节：hover/active/focus 三态、命令面板图标与 `Kbd` 键位、AI 按钮选中态（R-6） |
| D7 | 评审条 + 库任务浮层同款视觉（R-8） |

**非目标**

- 不动 AI 业务逻辑：agent loop / 工具 / prompt / 模型路由 / 会话落盘格式（`StoredBlock`）
- 不注册 Dock 面板、不改 `MAIN_DOCK_VERSION`（`app.rs:492`）——用户既有布局不动
- 不新增依赖
- 不做多会话管理 UI（Q4 保持按笔记归属）
- 不改预览区 / 文件树 / 大纲的**交互**（只统一间距与 hover 口径）

---

## 3. 方案概述

### 3.1 选定方案 A：新建全局 `ui/tokens.rs`，四块 UI 分两期重做

| # | 做法 | 落在哪 |
|---|---|---|
| A1 | **新建 `src/ui/tokens.rs`**：间距 / 圆角 / 字号 / 语义色 / hover 口径的常量与辅助函数，AI 四块 + 大纲/搜索/反链/标签统一引用 | 新文件 |
| A2 | 气泡：角色图标（`CircleUser` / `Bot`）+ 双方容器背景 + `theme.radius` + hover + 折叠 affordance | `ai.rs:1464-1526` |
| A3 | 输入区：`Textarea` 替换 `Input`；模式切换用分段控件暴露 Chat / Action / Skill；快捷动作当前项高亮 | `ai.rs:1665-1757`、`ai.rs:1637-1662` |
| A4 | 状态行：分成 4 个带图标的小格（状态 / 耗时 / 工具 / tokens），错误用 `danger` 色 + `CircleAlert` | `ai.rs:1760-1807` |
| A5 | 可拖拽调高：AI 条与正文之间加 `v_resizable` + `ResizableState`，高度存 `EditorPanel` | `editor.rs:2993-2998`、`ai.rs:1448` |
| A6 | 命令面板条目：左侧 `Icon`（默认按类别给图标）+ 右侧 `Kbd`（`Kbd::binding_for_action`） | `command.rs:259-278` |
| A7 | 评审条 / `plan.rs`：卡片补底色 + hover，圆角走 `theme.radius` | `editor.rs:2197-2206`、`plan.rs:498-555` |
| A8 | 删掉 `bubble_bg()` 的 `hsla()`，改走语义色；同步改 `tests/ui/ai.rs:97-119` | `ai.rs:64-81` |

### 3.2 被否决方案

| # | 方案 | 否决理由 |
|---|---|---|
| **B** | AI 对话区改成右侧 Dock 面板（可停靠 / 与预览并列） | 用户已选"保持内嵌条"（需求 §6-Q1）；且注册面板要递增 `MAIN_DOCK_VERSION`（`app.rs:492`），会重置用户既有布局 |
| **C** | 只改对话条，评审条与 `plan.rs` 不动 | 用户已选"全部"（§6-Q2）；留一半会割裂 |
| **D** | 视觉规范只建在 AI 区，其它面板后续再说 | 用户已选"全局统一"（§6-Q3）；且现有 7 处 hover 已是同一口径 `accent.opacity(0.12)`，统一成本极低 |
| **E** | 拖拽调高自己画把手（监听 mouse down/move） | 框架已有 `v_resizable` + `resizable_panel` + `ResizableState`（`gpui-base/src/resizable/panel.rs:38-108`），dock 内部就是用它；自绘既重复又与 dock 手感不一致 |
| **F** | 键位提示继续用 `tooltip_with_action` 文案 | 用户明确要"快捷键提示"；框架有 `Kbd` 组件（`gpui-component/src/kbd.rs:11`），`Kbd::binding_for_action(action, context)` 能直接取绑定，不必自绘 |

---

## 4. 视觉规范（`src/ui/tokens.rs`）

### 4.1 间距 scale

沿用 base `SpacingTokens`（`gpui-base/src/theme_tokens.rs:132-154`）：

| 名 | 值 | 用途 |
|---|---|---|
| `SPACE_XS` | 4 | 图标与文字的间隙 |
| `SPACE_SM` | 8 | 行内元素间隙（`gap_2`） |
| `SPACE_MD` | 12 | 分区内边距（`px_3`） |
| `SPACE_LG` | 16 | 分区之间 |
| `SPACE_XL` | 24 | 空态、浮层内边距 |

### 4.2 圆角

统一 `cx.theme().radius`（`gpui-component/src/theme/mod.rs:185`）。
**禁止再写 `px(12.)`（`ai.rs:1504`）/ `px(6.)`（`editor.rs:2204`）/ `px(3.)`（`editor.rs:2603`）**。

### 4.3 字号

| 场景 | 字号 |
|---|---|
| 气泡正文 | `text_sm`（14） |
| 状态行 / 辅助说明 / 键位 | `text_xs`（12） |
| 分区标题（对话条顶栏的动作名） | `text_sm` + `font_weight(600)` |

### 4.4 语义色映射（**这是本次最核心的收敛**）

| 用途 | token | 替换掉 |
|---|---|---|
| 用户气泡底 | `accent.opacity(0.12)` | `bubble_bg(is_dark)` 手搓 HSL（`ai.rs:64-81`） |
| AI 气泡底 | `muted`（或 `group_box`） | 现状：无底色（`ai.rs:1512-1514`） |
| hover | `accent.opacity(0.12)` | 无（与既有 7 处口径一致） |
| 选中 / 当前模式 | `accent.opacity(0.18)` + 左侧 2px `primary` 竖条 | 无 |
| 分隔 | `border` | 现状只有顶栏一条（`ai.rs:1379`） |
| 错误 | `danger` 文字 + `CircleAlert` 图标 | 现状灰字（`ai.rs:1760-1807`） |
| 生成中 | `muted_foreground` + `Loader` 图标 | 现状单行文本"生成中…" |
| 卡片底（评审 / plan 条目） | `group_box` | 现状无底色（`plan.rs:505`） |

### 4.5 图标

| 用途 | 图标 | 依据 |
|---|---|---|
| 用户角色 | `IconName::CircleUser` | `gpui-kit-assets-0.8.0/assets/icons/circle-user.svg` |
| AI 角色 | `IconName::Bot` | 项目已用（`editor.rs:2621`） |
| 生成中 | `IconName::Loader` | `.../loader.svg` |
| 停止 | `IconName::CircleStop` | `.../circle-stop.svg` |
| 发送 | `IconName::ArrowRight` | 项目已用 |
| 错误 | `IconName::CircleAlert` | `.../circle-alert.svg` |

### 4.6 对话条线框（目标形态）

```
┌─ EditorPanel ────────────────────────────────────┐
│ 工具栏                                            │
│ [评审条：卡片，hover 有反馈]                       │
│ ───────── v_resizable 分隔（可拖）────────────── │
│ 正文编辑区                                        │
│ ───────── v_resizable 分隔（可拖）────────────── │
│ ┌─ AI 对话条 ─────────────────────────────────┐ │
│ │ ⊙ deepseek-chat   [对话|润色|Skill]  清空 ✕ │ │ 顶栏：模型 + 模式分段 + 操作
│ │ ┌──────────────────────────────────────┐   │ │
│ │ │ 👤 帮我润色这段                        │   │ │ 用户气泡（accent 底，右对齐）
│ │ │        🤖 好的，以下是…                │   │ │ AI 气泡（muted 底，左对齐）
│ │ │        ▸ 工具调用 ×2（可折叠）         │   │ │
│ │ └──────────────────────────────────────┘   │ │ 滚动区（可拖高）
│ │ [润色][精简][翻译]…（当前项高亮）            │ │ 快捷动作
│ │ ┌────────────────────────────┐ [发送]      │ │ 多行输入
│ │ └────────────────────────────┘             │ │
│ │ ⏱ 1.2s · 🔧 2 · 8.4k tokens                 │ │ 状态行（分区 + 图标）
│ └─────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────┘
```

---

## 5. 数据模型与持久化

- **`Block` / `StoredBlock` 序列化格式一字不改**（`ai.rs:300-364`）——`tests/state/ai_sessions.rs`、`tests/ui/ai.rs:56-91` 锁死
- **会话归属不变**：`note_path`（`ai.rs:440`），切标签即切会话
- **新增的唯一持久化**：AI 条高度。存哪？两个选择：
  - （选定）**不进 `config.json`**，用 `EditorPanel` 字段 + `ResizableState`（框架自己管），窗口内记住即可，重启回到默认 260
  - 若要跨会话记住，得在 `AppConfig` 加 `ai_panel_height`（会动持久化格式，本次不做，见 §9 开放问题 O1）
- 删除 `bubble_bg()` 后，`tests/ui/ai.rs:97-119` 的断言改为"气泡底色来自 `accent.opacity(0.12)`"

---

## 6. 异常与边界

| # | 场景 | 处理 |
|---|---|---|
| E1 | 空对话 | 空态容器：居中图标 + 一句提示（现状是裸 `text_xs muted`，`ai.rs:1456-1462`） |
| E2 | 空提示词点发送 | 不发起请求，输入框给一次轻提示 |
| E3 | 超长消息 / 超长输出 | 气泡超过阈值自动折叠（沿用现有折叠机制），滚动区 `overflow_y_scroll` + `min_h_0`；不做虚拟化 |
| E4 | 模型输出含非法 UTF-8 | `TextView::markdown` 前做 `String::from_utf8_lossy`，绝不 panic |
| E5 | 会话文件半截 / 损坏 | `ai_sessions` 加载失败 → 降级为空会话 + 状态行提示，不 panic（现状无覆盖，本次补一条测试） |
| E6 | 切库 / 切标签 | 会话按 `note_path` 归位；评审条清空；AI 条开合状态按标签各自维护（现状已是每标签一份实例） |
| E7 | 切主题 | 全部走 `cx.theme()`，无一处手搓颜色；切完立即生效（`app.rs:267` 的 `refresh_windows`） |
| E8 | 可拖拽边界 | 高度范围 `[160, 600]` px，拖到极限停住，不出现 0 高度或盖住正文 |

---

## 7. 测试计划

### 7.1 能自动化

| # | 测试 | 断言 |
|---|---|---|
| T-1 | 既有 AI 相关 | `tests/ui/ai.rs`（9 条）、`tests/ui/command.rs`、`tests/ui/editor.rs`、`tests/state/ai_sessions.rs` **保持全绿** |
| T-2 | **改** `bubble_bg` 断言 | 气泡底色来自主题 token，不再有手写 HSL |
| T-3 | **新增** 损坏会话降级 | `ai_sessions` 解析失败 → 空会话不 panic |
| T-4 | **新增** 非 UTF-8 兜底 | 非法字节不 panic，输出含 `U+FFFD` 或等价处理 |
| T-5 | **新增** token 表自洽 | 间距/圆角常量与 `theme.radius` 的引用关系（防止回潮写死值；至少保证常量被引用） |
| T-6 | G4 四连 | build / clippy 0 warning、fmt clean、cargo test 全绿 |

### 7.2 要人工（视觉 / 手感，登记进 `docs/ACCEPTANCE.md` 新的一节）

hover 三态、明暗两套、模式切换、多行输入、命令面板图标与 `Kbd`、拖拽调高、评审卡片、plan 卡片、状态行分区、切库切标签切主题、信息密度。逐条对应 AC-5…AC-13。

---

## 8. 人工验收清单

**唯一登记处：`docs/ACCEPTANCE.md`**（实施时新增一节）。编号映射：

| AC | 一句话 |
|---|---|
| AC-5 | hover / active / focus 三态 |
| AC-6 | 明暗两套都不脱 token |
| AC-7 | 多行输入 + 模式切换可见可点 |
| AC-8 | 命令面板 AI 条目有图标与 `Kbd` |
| AC-9 | 对话条可拖高，边界 160–600 |
| AC-10 | 内嵌条与编辑器、评审条的层级与分隔清楚 |
| AC-11 | 状态行分区，错误是红色 |
| AC-12 | 切库 / 切标签 / 切主题后状态正确 |
| AC-13 | 信息密度合理，接近 Obsidian |

---

## 9. 风险与开放问题

| # | 风险 | 处置 |
|---|---|---|
| K1 | `v_resizable` 改 `EditorPanel` 结构，可能影响正文滚动/焦点 | 分期做（放在最后一期），出问题可回退为固定高度 |
| K2 | `muted` / `group_box` 在深色下对 AI 气泡的对比度 | 人工验 AC-6；不合适就换 `surface` 语义色（`theme.tokens.colors.surface`） |
| K3 | 全局统一 hover 口径会动 7 处已有面板 | 每处只改数值不改逻辑，逐个人工验 |
| K4 | `Kbd::binding_for_action` 需要 `Action` 与 context | 若取不到绑定（自定义动作），降级为不显示键位，不显示空标签 |
| K5 | 删除 `bubble_bg` 后旧测试断言失效 | 同步改 `tests/ui/ai.rs:97-119` |

**开放问题（默认做法，不反对即按此执行）**

| # | 问题 | 默认 |
|---|---|---|
| O1 | AI 条高度要不要跨会话记住 | **不记**：窗口内有效，重启回默认 260（不为此新增配置字段） |
| O2 | 快捷动作行是否默认展开 | **展开**（现状即展开），只在输入框聚焦时收紧为一行 |
| O3 | 命令面板 AI 条目图标用什么 | 动作与 Skill 统一用 `Bot`（避免为每个动作配图标） |

---

## 10. 实施步骤（分期，每期可独立编译验证）

| 期 | 内容 | 涉及文件 | 关联 R | 验收 |
|---|---|---|---|---|
| **P1** | 新建 `src/ui/tokens.rs`（间距/圆角/字号/语义色/hover 口径）；AI 四块 + 大纲/搜索/反链/标签统一引用；删 `bubble_bg` 的 `hsla()` | 新增 `src/ui/tokens.rs`、`ai.rs`、`outline.rs`、`search.rs`、`backlink.rs`、`tags.rs` | R-1 / R-7 | T-1 / T-2 / T-5 + G4 四连 |
| **P2** | 气泡重做（角色图标 + 双方底色 + `theme.radius` + hover + 折叠 affordance + 流式指示） | `src/ui/ai.rs` | R-3 | G4 四连 + 人工 AC-5 / AC-6 |
| **P3** | 输入区：`Textarea` + 模式切换分段控件 + 快捷动作当前项高亮 | `src/ui/ai.rs` | R-4 | G4 四连 + 人工 AC-7 |
| **P4** | 状态行分区（图标 + 分隔 + `danger`）；空态容器；非 UTF-8 与损坏会话兜底 | `src/ui/ai.rs`、`src/state/ai_sessions.rs` | R-5 / 边界 E4-E5 | T-3 / T-4 + G4 四连 + 人工 AC-11 |
| **P5** | 评审条 + `plan.rs` 卡片化；命令面板 `Icon` + `Kbd`；AI 按钮选中态；删 `eprintln!` | `src/ui/editor.rs`、`src/ui/ai/plan.rs`、`src/ui/command.rs` | R-6 / R-8 | G4 四连 + 人工 AC-8 / AC-10 |
| **P6** | 可拖拽调高（`v_resizable` + `ResizableState`，范围 160–600） | `src/ui/editor.rs`、`src/ui/ai.rs` | R-2 | G4 四连 + 人工 AC-9 / AC-12 |

每期结束跑 G4 四连，输出摘要贴进 `docs/exec-plans/ai-ui-overhaul.md`；全部完成后登记 `docs/ACCEPTANCE.md` 与 `docs/INDEX.md`。

**分支**：实施在 `feature/ai-ui-overhaul` 上进行，未得明确指示不合并 main（2026-10-28 约定）。
