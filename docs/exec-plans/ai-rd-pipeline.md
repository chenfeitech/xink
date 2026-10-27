# AI 端到端研发流水线 执行计划

| 属性 | 内容 |
|---|---|
| slug | ai-rd-pipeline |
| 创建日期 | 2026-10-27 |
| 需求文档 | 用户对话提出（2026-10-27），要点见设计文档 §1 现状与 §2 目标 |
| 设计文档 | [docs/ai-rd-pipeline-design.md](../ai-rd-pipeline-design.md)（v1.1，已确认） |
| 关联 commit | — |

## 状态机

需求 ✅ → 设计 ✅ → 计划 ✅ → 实施 ✅（M1–M4 文件全部产出）→ 测试 ⬜ → 质量 🔄（待整体验收）

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 六个环节诉求明确；边界由 Q1–Q3 拍板澄清，见设计文档 §8 |
| G2 设计 | ✅ | 用户于 2026-10-27 确认；含被否决方案 4 个（设计文档 §3.3） |
| G3 计划 | ✅ | 本文件：22 项任务，每任务有依赖 / 涉及文件 / 验收方式 |
| G4 实施 | ✅ | M1 ✅（6/6）；M2 ✅（4/5）；M3 ✅（4/5）；M4 ✅（5/6）；剩 T-11 / T-16 / T-22 均为人工演练 |
| G5 测试 | ⬜ | 对本需求「不适用」，替代校验见偏差记录 |
| G6 质量 | 🔄 | 待整体验收（T-11 / T-16 / T-22 三项人工演练） |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收方式 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | 新建 `00-pipeline.md`（alwaysApply） | — | P0 | `.codebuddy/rules/00-pipeline.md` | 文件存在 + 常驻生效 | R-1 | ✅ |
| T-2 | 新建 `07-quality-gates.md`（alwaysApply） | — | P0 | `.codebuddy/rules/07-quality-gates.md` | G1–G6 判定项齐全 | R-6 | ✅ |
| T-3 | 修订 5 条 rules 加载声明 | — | P0 | `.codebuddy/rules/01..05` | frontmatter 含 globs | R-1 | ✅ |
| T-4 | 建 `docs/exec-plans/` + 模板 | — | P0 | `docs/exec-plans/README.md` | 模板可渲染 | R-3 | ✅ |
| T-5 | INDEX §14 归属规则 | T-4 | P0 | `docs/INDEX.md` | 目录树 + 第 5 条规则 | R-3 | ✅ |
| T-6 | 回写设计文档拍板结论 | — | P1 | `docs/ai-rd-pipeline-design.md` | Q1–Q3 标 ✅ | R-1 | ✅ |
| T-7 | 新建 `08-artifacts.md` 产物规范 | T-1 | P0 | `.codebuddy/rules/08-artifacts.md` | 四类文档章节 + ID 规则 | R-1 R-2 | ✅ |
| T-8 | skill `requirement-clarify` | T-7 | P0 | `.codebuddy/skills/requirement-clarify/` | 四步 SOP + 边界清单 | R-1 | ✅ |
| T-9 | skill `solution-design` | T-7 | P0 | `.codebuddy/skills/solution-design/` | ≥2 方案 + ADR 模板 | R-2 | ✅ |
| T-10 | skill `rd-orchestrator` 元技能 | T-8, T-9 | P0 | `.codebuddy/skills/rd-orchestrator/` | 阶段判定 + 派发 + 守门 | R-1 | ✅ |
| T-11 | M2 验收演练 | T-10 | P0 | — | 提真实需求，停在 G2 等人 | R-1 R-2 | ⬜ 人工 |
| T-12 | 新建 `06-git-workflow.md` | T-1 | P0 | `.codebuddy/rules/06-git-workflow.md` | 提交粒度 + 最小变更 | R-4 | ✅ |
| T-13 | skill `exec-plan` | T-7 | P0 | `.codebuddy/skills/exec-plan/` | 五步 SOP + C-1~C-6 | R-3 | ✅ |
| T-14 | skill `implement-task` | T-2, T-12 | P0 | `.codebuddy/skills/implement-task/` | 七步 SOP + G4 四连 | R-4 | ✅ |
| T-15 | 修订 2 个已有 skill 为技术底座 | T-14 | P1 | `skills/gpui-desktop-dev/`、`skills/notes-app-dev/` | 加流程位置声明 | R-4 | ✅ |
| T-16 | M3 验收演练 | T-13..15 | P0 | — | 拆任务 + 逐步自校验 | R-3 R-4 | ⬜ 人工 |
| T-17 | 新建 `09-testing-standard.md` | T-1 | P0 | `.codebuddy/rules/09-testing-standard.md` | 覆盖底线 + 边界九项 | R-5 | ✅ |
| T-18 | 新建 `10-acceptance.md` | T-1 | P0 | `.codebuddy/rules/10-acceptance.md` | 人工验收唯一去处 | R-6 | ✅ |
| T-19 | skill `test-verify` | T-17 | P0 | `.codebuddy/skills/test-verify/` | 四步 SOP + C-1~C-6 | R-5 | ✅ |
| T-20 | skill `quality-review` | T-18 | P0 | `.codebuddy/skills/quality-review/` | 五步 SOP + C-1~C-5 | R-6 | ✅ |
| T-21 | 修订 `03-rust-code-style` 指向 07 | T-2 | P2 | `.codebuddy/rules/03-rust-code-style.md` | 引用闭环 | R-4 | ✅ |
| T-22 | 全链路演练 + INDEX 回写 | 全部 | P0 | `docs/ACCEPTANCE.md`、`docs/INDEX.md` | 一句需求走完六阶段 | R-1..R-6 | 🔄 人工 |

## 待用户拍板

- [x] Q1 状态文件位置 → 采用 `docs/exec-plans/` 新目录
- [x] Q2 常驻 token 开销 → 接受
- [x] Q3 缺陷豁免 → 维持跳过 G1/G2，加三条硬约束

## 门禁命令输出摘要

### 说明

本需求交付物是 `.codebuddy/` 下的 rules 与 skills（Markdown），**不改动 `src/`**，
因此 G4 的 4 条 cargo 命令对本需求不适用（见下方偏差记录）。替代校验为「文件产出校验」：

```text
文件存在性   : 每个 T-n 的涉及文件已创建
frontmatter  : 含 name / description（skills）、description + 加载声明（rules）
引用闭环     : 规则与技能中引用的相对路径真实存在
章节合规     : 对照 08-artifacts.md 的强制章节逐项核
```

### M1（已通过）

```text
00-pipeline.md        : 创建，alwaysApply: true
07-quality-gates.md   : 创建，alwaysApply: true
rules/01..05          : frontmatter 已补 globs
docs/exec-plans/      : 目录 + README.md 模板
docs/INDEX.md         : §14 目录树 + 第 5 条归属规则
ai-rd-pipeline-design : v1.1，Q1–Q3 已回写
```

### M2（已通过，T-7 – T-10）

```text
08-artifacts.md                 : 创建，globs docs/**/*.md
requirement-clarify/SKILL.md    : 创建，四步 SOP + C-1~C-6
requirement-clarify/references  : boundary-checklist.md（边界九项 + 两类测不了）
solution-design/SKILL.md        : 创建，六步 SOP + C-1~C-6
solution-design/references      : adr-template.md（含写作要求）
rd-orchestrator/SKILL.md        : 创建，派发表 + 技能注册表 + C-1~C-5
引用闭环核查                     : 通过，待建规则均已标注（M3/M4 待建）
```

### M3（已通过，T-12 – T-15）

```text
06-git-workflow.md               : 创建，提交粒度 + Conventional Commits + 最小变更
exec-plan/SKILL.md               : 创建，五步 SOP + C-1~C-6
implement-task/SKILL.md          : 创建，七步 SOP + G4 四连 + C-1~C-6
gpui-desktop-dev / notes-app-dev : 加「阶段 4 技术底座，由 implement-task 调用」声明
```

### M4（已通过，T-17 – T-21）

```text
09-testing-standard.md           : 创建，覆盖底线 + 边界九项 + 三类测不了
10-acceptance.md                 : 创建，人工验收唯一去处
test-verify/SKILL.md             : 创建，四步 SOP + C-1~C-6
quality-review/SKILL.md          : 创建，五步 SOP + 缺陷分级 + C-1~C-5
03-rust-code-style.md            : 加指向 07（G4 四连）/ 09（测什么）的引用
待建标注清理                      : 00 / 08 / solution-design 的「M3/M4 待建」已移除
rd-orchestrator 技能注册表         : 六阶段全部 ✅ 可用

产出统计：rules 11 条（00–10）；skills 9 个（6 新增 + 2 已有 + 1 元技能）
```

## 偏差记录

1. **G4/G5 的 cargo 命令对本需求不适用**：本需求不改动 Rust 代码，无编译产物变化。
   替代为「文件产出校验」（见上）。若未来 rules/skills 变更伴随 `src/` 改动，仍须跑完整 G4。
2. **T-4 / T-5 / T-6 为实施中新增**：原设计 §7 的 M1 只列 2 项（T-1、T-2），
   实施时发现状态文件目录、INDEX 归属规则、设计文档回写必须同期完成，否则体系不自洽。
3. **M1 判定为已通过但未走完整 G1/G2**：本需求的需求与设计由用户在同一轮对话中直接确认，
   产物为设计文档 §1–§3，已满足 G1/G2 实质要求（含非目标 §2 N1–N4、方案对比 §3.3）。
