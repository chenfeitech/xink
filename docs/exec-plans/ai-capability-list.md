# 模型能力清单 执行计划

| 属性 | 内容 |
|---|---|
| slug | ai-capability-list |
| 创建日期 | 2026-10-29 |
| 需求文档 | [docs/ai/ai-capability-list-requirements.md](../ai/ai-capability-list-requirements.md) |
| 设计文档 | [docs/ai/ai-capability-list-design.md](../ai/ai-capability-list-design.md) |
| 分支 | `feature/ai-capability-list` |
| 关联 commit | `61899e6`(P1) `93031e8`(P2) `e078b4c`(P3) |

## 状态机

需求 ✅ → 设计 ✅ → 计划 ✅ → 实施 ✅ → 测试 ✅ → 质量 ⬜（人工验收 ACCEPTANCE §21）

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 四节齐全；R-1…R-5 编号唯一；现状 7 项均附 `文件:行号`；§6 拍板清单已清空（Q1 静态表+可改 / Q2 只展示不改行为 / Q3 探针不动） |
| G2 设计 | ✅ | 十章齐全、含 5 个被否决方案；用户 2026-10-29 确认开工 |
| G3 计划 | ✅ | P1–P4 每期含涉及文件、验收命令、关联 R-n，每期可独立编译验证 |
| G4 实施 | ✅ | 见下方命令输出摘要 |
| G5 测试 | ✅ | `cargo test` 638 → 653（新增 15 条） |
| G6 质量 | ⬜ | 人工验收项已登记 [ACCEPTANCE.md §21](../ACCEPTANCE.md) |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| P1 | 新模块 `core/ai/capability.rs`：能力字典 `info()` + 推断 `infer()` 纯函数 | — | P0 | `src/core/ai/capability.rs`（新）、`src/core/ai/mod.rs` | T-1 / T-2 + G4 四连 | R-1 / R-3 | ✅（`61899e6`） |
| P2 | `ModelEntry` 加 `capabilities_pinned`；`models.rs` 合并时尊重它 | P1 | P0 | `src/core/config.rs`、`src/core/ai/models.rs` | T-3 + G4 四连 | R-4 | ✅（`93031e8`） |
| P3 | UI：能力 chip 行（悬停看说明）+「编辑」勾选菜单；`capability_line` 拆出可测的 `capability_summary` | P1 / P2 | P1 | `src/ui/settings.rs` | T-4 + G4 四连 + 人工 AC-6…AC-11 | R-2 / R-5 | ✅（`e078b4c`） |
| P4 | 状态回写 + 验收登记 | P3 | P2 | `docs/` | G6 | — | ⬜ |

## 待用户拍板

- [x] Q1 OpenAI 兼容侧能力从哪来 → **静态表 + 可手动改**
- [x] Q2 是否让能力"可选用"（开关）→ **只展示说明，不改行为**
- [x] Q3 Ollama 侧是否扩展探针 → **不动**

## 门禁命令输出摘要

### P1 提交前（2026-10-29，`61899e6`）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 638 → 647 passed, 0 failed, 3 ignored
git diff --stat            : src/core/ai/{capability.rs (新), mod.rs}、tests/core/ai/capability.rs（新）
```

新模块靠 `src/core/ai/mod.rs` 的 `pub mod capability;` 挂载；
单测用 `#[path = "../../../tests/core/ai/capability.rs"]` 挂进模块内
（`Cargo.toml:13` `autotests = false`）。

### P2 提交前（2026-10-29，`93031e8`）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 647 → 648 passed, 0 failed, 3 ignored
git diff --stat            : src/core/{config.rs,ai/models.rs}、tests/core/ai/models.rs
```

### P3 提交前（2026-10-29，`e078b4c`）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 648 → 653 passed, 0 failed, 3 ignored
git diff --stat            : src/core/ai/capability.rs、src/ui/settings.rs、tests/{core/ai/capability.rs,ui/settings.rs}，+210-29
```

`capability_row` 初版带了 `cx` 参数但没用到（chip 用 `.ghost()` 自带样式），
clippy 报 `unused_variables` → 去掉参数。

## 偏差记录

（暂无）
