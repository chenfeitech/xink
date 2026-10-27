# Clippy 清零 执行计划

| 属性 | 内容 |
|---|---|
| slug | clippy-cleanup |
| 创建日期 | 2026-10-27 |
| 需求文档 | 2026-10-27 质量评估发现：`cargo clippy --all-targets` 有 17 条 warning，违反 `03-rust-code-style.md` §1「提交前 clippy 无 warning」 |
| 设计文档 | **免**（缺陷修复类，现象与改法由 clippy 给出） |
| 关联 commit | dad555c（起点） |

## 状态机

需求 ✅ → 设计 ➖（豁免）→ 计划 ✅ → 实施 ✅ → 测试 ✅ → 质量 ✅

## 豁免判定

按 `00-pipeline.md` §4 三条：

| 条件 | 满足 |
|---|---|
| 现象明确、可复现 | ✅ clippy 给出文件与行号 |
| 改法唯一，无方案权衡 | ✅ clippy 给出建议 |
| 改动 ≤1 个模块 | ❌ 跨 4 个模块（`core/ai`、`ui/editor`、`ui/settings`、`tests/core/ai`） |

**结论：不走豁免**，立本计划走 G3 / G4。

## 门禁

| 门禁 | 结果 | 证据 |
|---|---|---|
| G1 需求 | ➖ | 豁免 |
| G2 设计 | ➖ | 豁免 |
| G3 计划 | ✅ | 本文件 |
| G4 实施 | ✅ | 四连全绿，输出见下 |
| G5 测试 | ✅ | `cargo test` 603 passed / 0 failed（与修复前一致，行为未变） |
| G6 质量 | ✅ | `INDEX.md` §10 已回写 |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收方式 | 状态 |
|---|---|---|---|---|---|---|
| T-1 | 修 `core/ai` 的 6 条 | — | P0 | `context.rs:33`、`ollama.rs:251/256`、`openai.rs:168/174`、`snapshot.rs:114` | clippy 该目录 0 warning + 测试绿 | ✅ |
| T-2 | 修 `src/ui/editor.rs` 的 3 条 | — | P0 | `1606`（文档注释错位）、`1679`、`2990`（if 折叠） | 同上 | ✅ |
| T-3 | 修 `src/ui/settings.rs` 的 1 条 | — | P0 | `1632`/`1633`（Default::default() 后赋值） | 同上 | ✅ |
| T-4 | 修 `tests/core/ai` 的 6 条 | — | P1 | `keychain.rs:7-10`、`models.rs:51-52` | 同上 | ✅ |
| T-5 | G4 四连 + 回写 `INDEX.md` | T-1..4 | P0 | — | build / clippy / fmt / test 全绿 | ✅ |

## 待用户拍板

- [x] 无 —— 用户已指示「先修」

## 已知不可修（Won't fix）

| 项 | 说明 |
|---|---|
| `block v0.1.6` | 第三方 crate，由依赖树间接引入，未来 Rust 版本会拒绝。本项目无法控制，跟踪方式：`cargo tree -i block`。**不为了让 clippy 归零而锁死或替换第三方依赖** |

## 原则

- **只做 clippy 建议的原样修复**，不顺手重构、不改命名、不动逻辑
- 每条改动必须能被 `cargo test` 证明行为不变
- 若某条修复需要改设计（不只是改写法）→ **停下来回报**，不自行扩大范围

## 门禁命令输出摘要

### T-1 – T-4 完成后（G4 四连）

```text
cargo build                : 1 warning（第三方 block v0.1.6，不可控）
cargo clippy --all-targets : 1 warning（同上，仅此一条；修复前 17 条）
cargo fmt --check          : clean
cargo test                 : 603 passed / 0 failed / 3 ignored
git diff --stat            : 8 个文件（src 5 + tests 3）
```

### 修复明细

| 位置 | 修法 |
|---|---|
| `core/ai/context.rs:33` | `(narrow + 3) / 4` → `narrow.div_ceil(4)` |
| `core/ai/ollama.rs`、`openai.rs` 各 2 处 | 嵌套 `if` → let-chains |
| `core/ai/snapshot.rs:114` | 去掉多余的 `as i64` |
| `ui/editor.rs` 2 处 | 嵌套 `if` → let-chains |
| `ui/editor.rs:1606` | **文档注释错位**（见偏差记录 2） |
| `ui/settings.rs:1632` | `Default::default()` + 赋值 → 结构体字面量 |
| `tests/core/ai/keychain.rs`、`models.rs` | 同上 |

## 偏差记录

1. **`block v0.1.6` 未修**：第三方 crate，由依赖树间接引入，未来 Rust 版本会拒绝。
   记为 Won't fix——**不为了让 clippy 归零去锁死或替换第三方依赖**。
   跟踪：`cargo tree -i block`。
2. **T-2 的改动超出"改写法"**：`editor.rs:1606` 表面是 lint，实为**文档注释错位**——
   描述 `insert_at_cursor`（2309 行）的 doc comment 被挂到了 `selected_text` 上，
   中间隔着 6 行普通注释。已按最小变更把注释移到真正的函数上方，**未改任何逻辑**。
3. **fmt 二次修正**：`cargo clippy --fix` 产出的 let-chains 缩进不符合 rustfmt，
   由 `cargo fmt` 统一后**复跑四连确认**（否则 G4 的 fmt 项会失败）。
