---
name: implement-task
description: 按计划实施编码。计划通过 G3 后使用：读状态文件 → 加载技术底座 → 改前先编译 → 编码 → G4 四连自校验 → 范围自检 → 回写状态文件。触发关键词：开始实施、写代码、做这个功能、实现 T-n、继续做、按计划来。
---

# 实施研发

## 角色

按 `exec-plan` 的任务逐个落地，遵守全部技术铁律，**提交前自校验**。

- 前置：状态文件存在且 G3 通过
- 门禁：`../../rules/07-quality-gates.md` G4（**每任务提交前**）
- 提交规范：`../../rules/06-git-workflow.md`

## 七步 SOP

### Step 1 读状态文件

确认当前 `T-n`、前置完成情况、计划的「涉及文件」范围。
**状态文件是唯一进度真值**，不靠对话记忆。

### Step 2 加载技术底座（按需）

| 场景 | 技能 |
|---|---|
| GPUI 组件用法 / 编译报错 / 窗口与浮层 / 异步任务 | `gpui-desktop-dev` |
| Vault / Note / 索引 / 写盘 / 文件监听 / 防抖 | `notes-app-dev` |

技术铁律（`01-gpui-stack`、`02-architecture`、`03-rust-code-style`、
`04-notes-domain`）按 globs 自动加载，改动前先确认自己读过。

### Step 3 改前先编译

GPUI 全量编译在分钟级。**先确认起点是绿的**，
否则会把时间浪费在排查与本次任务无关的历史错误上。

### Step 4 编码

| 关注点 | 要求 |
|---|---|
| 分层 | `ui → core` 单向；`core/` 不含 UI 组件。判据：能在 `#[test]` 里不建窗口跑通，就该在 `core/`（`../../rules/02-architecture.md` §2） |
| 错误处理 | `core/` 用 `thiserror` 可匹配枚举；`ui/` 用 `anyhow` + `context`（`../../rules/03-rust-code-style.md` §2） |
| 笔记领域 | 原子写盘（temp + rename）、防抖 300ms、外部变更必须提示（`../../rules/04-notes-domain.md`） |
| 性能 | 耗时操作进 `cx.background_spawn()`，`render()` 内不做 IO |

### Step 5 G4 四连自校验

```bash
cargo build                     # warning 数为 0
cargo clippy --all-targets      # 无 warning
cargo fmt --check               # 无 diff
cargo test                      # 全绿
```

**结果必须贴进状态文件**（含实际数字），只写"通过"视为未做。
这是防自评放水的关键——事后可核对。

### Step 6 范围自检

`git diff --stat` 与计划「涉及文件」比对。
超出部分按 `../../rules/06-git-workflow.md` §3 判断：属于四类允许的扩张式改动则注明，
否则**回退**，改为独立提案。

### Step 7 回写状态文件

任务置 ✅、填门禁输出摘要、记录偏差（改了计划外的文件 / 换了方案 / 降级了范围）。

## 强约束

| 编号 | 约束 |
|---|---|
| C-1 | G4 未全绿**不提交** |
| C-2 | 不做计划外的改动 |
| C-3 | 不为消除 clippy 告警牺牲可读性；确需 `#[allow]` 必须写原因 |
| C-4 | 新增的 `core/` 逻辑必须有 `#[test]` |
| C-5 | 1 个任务 = 1 次提交 |
| C-6 | 遇到方案级分歧（计划里的做法行不通）**停下来回报**，不自行改设计 |

## 参考

- 技术底座：`../gpui-desktop-dev/SKILL.md`、`../notes-app-dev/SKILL.md`
- 挂载测试的方式：`../../../docs/TESTS.md`
