# exec-plans：执行计划与流水线状态

> 每个需求一个文件 `docs/exec-plans/<slug>.md`，是**进度的唯一真值**。
> AI 开工先读它，阶段结束回写它——不依赖对话记忆。
> 规则依据：`.codebuddy/rules/00-pipeline.md`（阶段判定）、`07-quality-gates.md`（门禁）。

## 它是什么，不是什么

| 是 | 不是 |
|---|---|
| 阶段状态机（六环节走到哪） | 需求规格（那是 `docs/<slug>-requirements.md`） |
| 任务清单与依赖 | 技术方案（那是 `docs/<slug>-design.md`） |
| 门禁结果与命令输出摘要 | 人工验收清单（那是 `docs/ACCEPTANCE.md`） |
| 待用户拍板的问题列表 | 长期设计文档 |

命名沿用 `docs/INDEX.md` §12：小写短横线 `<slug>.md`，与需求 / 设计文档同 slug。

## 生命周期

```
需求提出 → 建文件 → 逐阶段推进并回写 → 六阶段全 ✅ → 归档候选
```

归档：功能交付且 `docs/INDEX.md` 已回写后，本文件成为一次性记录，
可移入 `docs/archive/`（与 `P1-PLAN.md` 同等对待）。

## 状态文件模板

````markdown
# <需求中文名> 执行计划

| 属性 | 内容 |
|---|---|
| slug | <slug> |
| 创建日期 | YYYY-MM-DD |
| 需求文档 | docs/<slug>-requirements.md |
| 设计文档 | docs/<slug>-design.md |
| 关联 commit | — |

## 状态机

需求 ⬜ → 设计 ⬜ → 计划 ⬜ → 实施 ⬜ → 测试 ⬜ → 质量 ⬜

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ⬜ | |
| G2 设计 | ⬜ | 用户确认于 YYYY-MM-DD |
| G3 计划 | ⬜ | |
| G4 实施 | ⬜ | 见下方命令输出摘要 |
| G5 测试 | ⬜ | |
| G6 质量 | ⬜ | 验收项已进 ACCEPTANCE.md |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | | — | P0 | | | R-1 | ⬜ |

## 待用户拍板

- [ ] Q1：<问题>？（影响：<若不确认的默认取值>）

## 门禁命令输出摘要

### T-1 提交前

```text
cargo build             : 0 warning
cargo clippy --all-targets : clean
cargo fmt --check       : no diff
cargo test              : N passed, 0 failed
git diff --stat         : <文件清单，与计划一致>
```

## 偏差记录

（实施与计划的任何偏差：改了计划外的文件、换了方案、降级了范围。没有就留空。）
````
