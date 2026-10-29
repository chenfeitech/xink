# AI 结果"不全面" 执行计划

| 属性 | 内容 |
|---|---|
| slug | ai-output-truncation |
| 创建日期 | 2026-10-29 |
| 需求文档 | [docs/ai/ai-output-truncation-requirements.md](../ai/ai-output-truncation-requirements.md) |
| 设计文档 | [docs/ai/ai-output-truncation-design.md](../ai/ai-output-truncation-design.md) |
| 分支 | `feature/ai-output-truncation`（已 rebase 到 `62b9426`） |
| 关联 commit | 待产出 |

## 状态机

需求 ✅ → 设计 ✅ → 计划 ✅ → 实施 ✅ → 测试 ✅ → 质量 ⬜（人工验收 ACCEPTANCE §20）

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 四节齐全；R-1…R-5 编号唯一；现状均附 `文件:行号`；§6 拍板清单已清空（Q1 症状 / Q2 修法 / Q3 落点 / Q4 档位 / Q5 参数范围） |
| G2 设计 | ✅ | 十章齐全、含 6 个被否决方案；用户 2026-10-29 拍板 Q4（档位 10240 起步）与 Q5（温度 + 上下文窗口一并开出口） |
| G3 计划 | ✅ | T-1…T-3 每任务含依赖、涉及文件、验收命令、关联 R-n，每期可独立编译验证 |
| G4 实施 | ✅ | T-1…T-3 每期四条命令全绿，输出摘要见下方 |
| G5 测试 | ✅ | `cargo test` 624 → 634（新增 10 条）；边界九项逐条标注见需求 §4「验证方式」列 |
| G6 质量 | ⬜ | 人工验收项已登记 [ACCEPTANCE.md §20](../ACCEPTANCE.md)，需用户亲自过一遍才能结项 |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | `StreamEvent` 新增 `Truncated` 变体；Ollama 解析 `done_reason=="length"`、OpenAI 解析 `finish_reason=="length"`；补全部 match 分支 | — | P0 | `src/core/ai/{mod,ollama,openai,agent_loop}.rs`、`src/ui/ai.rs` | T-1 / T-2 / T-4 测试 + G4 四连 | R-2 | ✅（`34c60db`） |
| T-2 | `AiChat` 加 `out_truncated` / `ctx_truncated`（请求前重置）；状态行两条红字；`assemble` 的 `truncated` 不再丢弃 | T-1 | P0 | `src/ui/ai.rs` | T-4 + G4 四连 + 人工 AC-7 / AC-8 | R-2 / R-3 / R-4 | ✅（`a4f2c6a`） |
| T-3 | 模型行三个参数档位下拉（最大生成 / 上下文窗口 / 温度）；`ctx_options()` 纯函数按 `context_limit` 过滤档位 | T-1 | P1 | `src/ui/settings.rs`、`src/core/config.rs` | T-3 / T-5 + G4 四连 + 人工 AC-6 / AC-10 | R-1 / R-5 | ✅（`df46f79`） |

## 待用户拍板

- [x] Q1 「不全面」是哪一种 → 两种都有（输出断在半句 + 后半篇没被处理）
- [x] Q2 修多深 → 只暴露设置项 + 截断可见化，不动默认值
- [x] Q3 `num_predict` 落点 → 逐模型
- [x] Q4 档位起点 → **10240**
- [x] Q5 `temperature` / `num_ctx` → **一并开出口**

**仍挂着（不阻塞本次）**：全局 `ai.ollama.num_ctx` / `ai.ollama.keep_alive` 是死配置项
（改了不生效，请求用逐模型 `params.*`）。方向二选一：让它生效 / 从设置里摘掉。见设计 §9-O3。

## 门禁命令输出摘要

### 分支准备（2026-10-29）

```text
git rebase main          : 成功，无冲突（分支原落后 main 10 个 commit）
cargo build              : Finished，0 warning
grep docs/INDEX.md       : ai-output-truncation 2 处登记 + 「已实施并合入 main」2 处，并存
```

文档两项改动在 `INDEX.md` 不同区域，rebase 自动合并，无需手工解冲突。

### T-1 提交前（2026-10-29，`34c60db`）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 624 → 629 passed, 0 failed, 3 ignored
git diff --stat            : 7 files，+113-8
```

新增 `StreamEvent::Truncated` 后编译报两处穷举 match 未覆盖：
`agent_loop.rs:126`、`ui/ai.rs:945`（预期内，需求 C1 已列）。
`mod.rs:305/433` 用的是 `matches!()`，不受影响。

### T-2 提交前（2026-10-29，`a4f2c6a`）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 629 → 632 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/ai.rs +75-9、tests/ui/ai.rs +40-1
```

`take_context` 要 `&mut self`，单测得先造 `AiChat`（要窗口与 cx）；
把提示映射抽成模块级纯函数 `context_notice` 再测，不值得为测而造实体。

### T-3 提交前（2026-10-29，`df46f79`）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 632 → 634 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/settings.rs +186-1、src/core/config.rs +5-2、tests/ui/settings.rs +26-1
```

`config::limit` 原本是私有模块（`config.rs:1106`），UI 拿不到上下限；
改成 `pub`，让控件范围与 `sanitize()` 的校正区间同源——两边各写一个数
必然漂移。这是计划外的一行改动，属 T-3 的必要前提。

## 偏差记录

- `src/core/config.rs` 的 `mod limit` 改 `pub`：计划「涉及文件」未列，
  原因是设置界面的档位上限要与 `sanitize()` 同源（详见 T-3 摘要）。改动 1 行。
