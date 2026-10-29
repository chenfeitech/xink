# AI 交互界面全面优化 执行计划

| 属性 | 内容 |
|---|---|
| slug | ai-ui-overhaul |
| 创建日期 | 2026-10-29 |
| 需求文档 | [docs/ai/ai-ui-overhaul-requirements.md](../ai/ai-ui-overhaul-requirements.md) |
| 设计文档 | [docs/ai/ai-ui-overhaul-design.md](../ai/ai-ui-overhaul-design.md)（待产出） |
| 分支 | `feature/ai-ui-overhaul` |
| 关联 commit | — |

## 状态机

需求 ✅ → 设计 ⬜ → 计划 ⬜ → 实施 ⬜ → 测试 ⬜ → 质量 ⬜

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 四节齐全；R-1…R-8 编号唯一；现状均附 `文件:行号`；§6 拍板清单已清空（2026-10-29 用户拍板：内嵌条 / 全部 / 全局统一 / 按笔记归属） |
| G2 设计 | ✅ | 十章齐全、含 5 个被否决方案；用户确认于 2026-10-29 |
| G3 计划 | ✅ | T-1…T-6 每任务含依赖、涉及文件、验收命令、关联 R-n，每期可独立编译验证 |
| G4 实施 | ⬜ | 见下方命令输出摘要 |
| G5 测试 | ⬜ | |
| G6 质量 | ⬜ | 人工验收项待登记 ACCEPTANCE.md |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | 新建 `src/ui/tokens.rs`（间距 / 圆角 / 字号 / 语义色 / hover 口径）；AI 四块 + 大纲/搜索/反链/标签统一引用；删 `bubble_bg` 的 `hsla()` | — | P0 | 新增 `src/ui/tokens.rs`、`src/ui/ai.rs`、`src/ui/{outline,search,backlink,tags,rail}.rs`、`tests/ui/ai.rs` | T-1 / T-2 / T-5 测试 + G4 四连 | R-1 / R-7 | ⬜ |
| T-2 | 气泡重做：角色图标 + 双方底色 + `theme.radius` + hover + 折叠 affordance | T-1 | P0 | `src/ui/ai.rs` | G4 四连 + 人工 AC-5 / AC-6 | R-3 | ⬜ |
| T-3 | 输入区：`Textarea` 替换 `Input`；模式切换分段控件；快捷动作当前项高亮 | T-2 | P1 | `src/ui/ai.rs` | G4 四连 + 人工 AC-7 | R-4 | ⬜ |
| T-4 | 状态行分区（图标 + 分隔 + `danger`）；空态容器；非 UTF-8 与损坏会话兜底 | T-3 | P1 | `src/ui/ai.rs`、`src/state/ai_sessions.rs` | T-3 / T-4 测试 + G4 四连 + 人工 AC-11 | R-5 | ⬜ |
| T-5 | 评审条 + `plan.rs` 卡片化；命令面板 `Icon` + `Kbd`；AI 按钮选中态；删 `eprintln!` | T-1 | P2 | `src/ui/editor.rs`、`src/ui/ai/plan.rs`、`src/ui/command.rs` | G4 四连 + 人工 AC-8 / AC-10 | R-6 / R-8 | ⬜ |
| T-6 | 可拖拽调高：`v_resizable` + `ResizableState`，范围 160–600 | T-4 | P2 | `src/ui/editor.rs`、`src/ui/ai.rs` | G4 四连 + 人工 AC-9 / AC-12 | R-2 | ⬜ |

（计划阶段填充）

## 待用户拍板

- [x] Q1 对话区形态 → 保持编辑器内嵌条（不注册 Dock 面板）
- [x] Q2 范围 → 全部（对话条 + 评审条 + 库任务浮层 + 命令面板）
- [x] Q3 视觉规范 → 全局统一
- [x] Q4 会话归属 → 按笔记归属

## 门禁命令输出摘要

（实施阶段逐任务填充）

## 偏差记录

（暂无）
