# AI 交互界面全面优化 执行计划

| 属性 | 内容 |
|---|---|
| slug | ai-ui-overhaul |
| 创建日期 | 2026-10-29 |
| 需求文档 | [docs/ai/ai-ui-overhaul-requirements.md](../ai/ai-ui-overhaul-requirements.md) |
| 设计文档 | [docs/ai/ai-ui-overhaul-design.md](../ai/ai-ui-overhaul-design.md) |
| 分支 | `feature/ai-ui-overhaul`（已合并并删除） |
| 关联 commit | `24dd3c0`(T-1) `923fb57`(T-2) `6e95ec2`(T-3) `07ca69f`(T-4) `e5f2ed8`(T-5) `1990b96`(T-6)；合入 main `f02b331` |

## 状态机

需求 ✅ → 设计 ✅ → 计划 ✅ → 实施 ✅ → 测试 ✅ → 质量 ✅

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 四节齐全；R-1…R-8 编号唯一；现状均附 `文件:行号`；§6 拍板清单已清空（2026-10-29 用户拍板：内嵌条 / 全部 / 全局统一 / 按笔记归属） |
| G2 设计 | ✅ | 十章齐全、含 5 个被否决方案；用户确认于 2026-10-29 |
| G3 计划 | ✅ | T-1…T-6 每任务含依赖、涉及文件、验收命令、关联 R-n，每期可独立编译验证 |
| G4 实施 | ✅ | T-1…T-6 每期四条命令全绿，输出摘要见下方 |
| G5 测试 | ✅ | `cargo test` 624 passed / 0 failed / 3 ignored；边界九项逐条标注见需求 §4「验证方式」列 |
| G6 质量 | ✅ | 人工验收项已登记 [ACCEPTANCE.md §19](../ACCEPTANCE.md)；用户于 2026-10-29 确认合并（`f02b331`），视为验收通过。合并后重跑 G4 四连：build / clippy 0 warning、fmt clean、test 624 passed |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | 新建 `src/ui/tokens.rs`（间距 / 圆角 / 字号 / 语义色 / hover 口径）；AI 四块 + 大纲/搜索/反链/标签统一引用；删 `bubble_bg` 的 `hsla()` | — | P0 | 新增 `src/ui/tokens.rs`、`src/ui/ai.rs`、`src/ui/{outline,search,backlink,tags,rail}.rs`、`tests/ui/ai.rs` | T-1 / T-2 / T-5 测试 + G4 四连 | R-1 / R-7 | ✅（24dd3c0） |
| T-2 | 气泡重做：角色图标 + 双方底色 + `theme.radius` + hover + 折叠 affordance | T-1 | P0 | `src/ui/ai.rs`、`src/ui/tokens.rs` | G4 四连 + 人工 AC-5 / AC-6 | R-3 | ✅ |
| T-3 | 输入区：`Textarea` 替换 `Input`；模式指示与「回到对话」；快捷动作当前项高亮 | T-2 | P1 | `src/ui/ai.rs`、`src/ui/tokens.rs` | G4 四连 + 人工 AC-7 | R-4 | ✅ |
| T-4 | 状态行分区（图标 + 错误用 `danger`）；空态容器；损坏会话兜底测试 | T-3 | P1 | `src/ui/ai.rs`、`src/ui/tokens.rs`、`tests/state/ai_sessions.rs` | 新增加 1 条测试 + G4 四连 + 人工 AC-11 | R-5 | ✅ |
| T-5 | 评审条 + `plan.rs` 卡片化；命令面板 `Icon` + 底部 `Kbd` 键位提示；AI 按钮选中态；删 `eprintln!` | T-4 | P2 | `src/ui/editor.rs`、`src/ui/ai/plan.rs`、`src/ui/command.rs`、`src/ui/tokens.rs` | G4 四连 + 人工 AC-8 / AC-10 | R-6 / R-8 | ✅ |
| T-6 | 可拖拽调高：`v_resizable` + `ResizableState`，范围 160–600 | T-4 | P2 | `src/ui/editor.rs`、`src/ui/ai.rs`、`src/ui/tokens.rs` | G4 四连 + 人工 AC-9 / AC-12 | R-2 | ✅ |

（计划阶段填充）

## 待用户拍板

- [x] Q1 对话区形态 → 保持编辑器内嵌条（不注册 Dock 面板）
- [x] Q2 范围 → 全部（对话条 + 评审条 + 库任务浮层 + 命令面板）
- [x] Q3 视觉规范 → 全局统一
- [x] Q4 会话归属 → 按笔记归属

## 门禁命令输出摘要

### T-1 提交前（2026-10-29）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 623 passed, 0 failed, 3 ignored
git diff --stat            : 11 files（新增 src/ui/tokens.rs），+158-94
```

### T-2 提交前（2026-10-29）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 623 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/ai.rs +62-12、src/ui/tokens.rs +10（与计划一致）
```

### T-3 提交前（2026-10-29）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 623 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/ai.rs +94-21、src/ui/tokens.rs +2（与计划一致）
```

多行输入的关键：多行模式下 Enter 默认是**换行**（`state.rs:2179`），
必须开 `submit_on_enter(true)`，否则会"按回车既换行又发送"。
现在 Enter 发送、Shift+Enter 换行。

### T-4 提交前（2026-10-29）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 624 passed, 0 failed, 3 ignored（新增 corrupt_session_file_falls_back_to_empty）
git diff --stat            : src/ui/ai.rs +62-18、src/ui/tokens.rs +2、tests/state/ai_sessions.rs +25
```

边界 E4（模型输出非 UTF-8）判定为**不适用**：Rust `String` 保证合法 UTF-8，
唯一的字节→字符串转换在 `core/ai/keychain.rs:33/53`，已用 `from_utf8_lossy`。

### T-5 提交前（2026-10-29）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 624 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/{command,editor,ai/plan,tokens}.rs +106-28
```

键位提示的落点调整：命令面板的候选是文件与 AI 动作，**都不是 gpui 的
`Action`**，查不到各自绑定（`Kbd::binding_for_action` 用不上）。
改为在面板底部提示面板自己的操作键（↑↓ 移动 / ⏎ 打开 / esc 关闭），
用 `Kbd` 组件画（键位来自 `Keystroke::parse`，解析不出就不画该键）。

`Size::XSmall` 用不了（`gpui_kit::*` 里 `Size` 是另一个泛型类型，
Icon::size 要的是 `impl Into<Length>`），沿用项目既有写法
`Icon::with_size(Size::Size(px(14.)))`。

### T-6 提交前（2026-10-29）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff（已 cargo fmt）
cargo test                 : 624 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/{ai,editor,tokens}.rs +43-15
```

两个面板始终挂在 `v_resizable` 上，关掉的那个用 `visible(false)` 塌成 0 高。
理由：`ResizableState::sync_panels_count` 按**实际渲染**的子面板数增删槽位，
条件挂/摘会让 `sizes` 被 truncate，拖过的高度每开关一次就丢一次。
AI 面板走 `.fixed()`：窗口缩放时正文吸收差值，对话条保持用户拖出来的高度
（不 fixed 会按比例跟着缩放）。

`AI_HISTORY_MAX_H` 退役：对话区高度改由 `flex_1` 吃面板剩余空间，
对话条的高度改为 `AI_PANEL_DEFAULT_H / MIN_H / MAX_H` 三个常量。

## 偏差记录

（暂无）
