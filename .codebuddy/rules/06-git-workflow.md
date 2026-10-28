---
description: Git 工作流：提交粒度、commit 格式、最小变更原则与禁止操作。在执行任何 git 操作前遵守。
globs: ["src/**", "tests/**", "docs/**", "Cargo.toml"]
---

# Git 工作流

> 流程位置见 `00-pipeline.md`，提交前门禁见 `07-quality-gates.md` G4。

## 1 提交粒度

- **1 个任务（`T-n`）= 1 次提交**，禁止多个任务混在一次提交里
- 禁止"先提交一半明天继续"——做到**可独立编译验证**的点再提交
- 文档与代码分开提交（`docs:` 与 `feat:` / `fix:` 不混）

## 2 Commit 格式

Conventional Commits：

```text
<type>(<scope>): <subject>

<body>

<footer>
```

| 项 | 取值 |
|---|---|
| `type` | `feat` / `fix` / `docs` / `style` / `refactor` / `perf` / `test` / `chore` |
| `scope` | 模块名：`vault` / `note` / `index` / `editor` / `preview` / `ai` / `config` / `rules` |
| `subject` | 祈使句，≤50 字，**结尾不加句号** |

```text
feat(index): 索引派生表改为增量维护
fix(editor): 修复滚动同步在长文档下的偏移
docs(ai): 补充 m3 库级任务设计
```

`body` 写清「做了什么 / **为什么**这么做」；行为变更必须在 `body` 写明。
`footer` 关联任务：`Refs: T-12` 或 `Closes: T-12`。

## 3 最小变更原则

不得改动与当前任务**无直接因果关系**的代码。允许的扩张式改动仅限四类：

1. 用户显式要求
2. 编译 / 类型 / lint 失败导致原任务无法完成
3. 复用已有抽象时发现其签名不足以承载新需求
4. 删除即将随本次改动失效的死链 / 死代码

其它"顺手优化"必须先以**独立提案**说明影响面，获确认后再做，
**禁止与主任务混提交**。

## 4 提交前自检

1. `git diff --stat` 与计划的「涉及文件」比对，超出须说明原因
2. 跑 G4 四连（见 `07-quality-gates.md`）
3. 确认无调试代码、临时文件残留（`*.md.tmp` 等）

## 5 禁止

- 不执行 `git push --force`（除非用户明确要求）
- 不 `git add -A` 后一次提交——先核对范围
- 不修改 git config
- 不在未说明的情况下 amend 已有提交
- **不漏掉 `Cargo.lock`**：依赖版本是硬约束（见 `01-gpui-stack.md` §4），
  `Cargo.lock` 必须提交进版本库

## 6 分支开发（用户 2026-10-28 要求）

- 新功能**一律在分支上做**：从 `main` 切 `feature/<slug>`（缺陷用 `bugfix/<xxx>`）
- 开发、提交、推送都只发生在分支上，**不往 `main` 推**
- **未经用户明确说「合并」，不得把分支合进 `main`**（也不得直接 push main）
- 功能做完就停在分支上：报告分支名 + 提交号 + 门禁结果，等用户发话再合并
