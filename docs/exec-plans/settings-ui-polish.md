# 设置界面优化（梳理 / 精简 / 美化）执行计划

| 属性 | 内容 |
|---|---|
| slug | settings-ui-polish |
| 创建日期 | 2026-10-28 |
| 需求文档 | [docs/config/settings-ui-polish-requirements.md](../config/settings-ui-polish-requirements.md) |
| 设计文档 | [docs/config/settings-ui-polish-design.md](../config/settings-ui-polish-design.md) |
| 分支 | `feature/settings-ui-polish` |
| 关联 commit | — |

## 状态机

需求 ✅ → 设计 ⬜ → 计划 ⬜ → 实施 ⬜ → 测试 ⬜ → 质量 ⬜

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 文档含目标/非目标/边界/验收四节；R-1…R-7 编号唯一；现状均附 `文件:行号`；§6 拍板清单已清空（2026-10-28 用户拍板：高级折叠 / 照单执行 / 移除 debug_scroll_sync / 快捷键独立分类 / 卡片分组+加宽 840 / 描述进 tooltip） |
| G2 设计 | ⬜ | 设计文档已产出（十章齐全、含 4 个被否决方案），**待用户确认**；确认前不动 `src/` |
| G3 计划 | ⬜ | |
| G4 实施 | ⬜ | 见下方命令输出摘要 |
| G5 测试 | ⬜ | |
| G6 质量 | ⬜ | 人工验收项待登记进 ACCEPTANCE.md |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | | — | P0 | | | R-1 | ⬜ |

（计划阶段填充）

## 待用户拍板

- [x] Q1 低频项处置 → 收进「高级」折叠
- [x] Q2 分级表 → 照单执行
- [x] Q3 `preview.debug_scroll_sync` → 移除
- [x] Q4 快捷键 → 独立分类
- [x] Q5 视觉力度 → 卡片分组 + 加宽 840
- [x] Q6 精简信息去向 → tooltip

## 门禁命令输出摘要

（实施阶段逐任务填充）

## 偏差记录

（暂无）
