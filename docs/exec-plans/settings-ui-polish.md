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
| T-1 | 分级与分类：`FieldTier` + `ADVANCED_IDS` 白名单；快捷键独立成 `SettingsTab::Keymap`；移除 `preview.debug_scroll_sync` 项 | — | P0 | `src/ui/settings.rs`、`tests/ui/settings.rs` | 新增 3 条测试 + G4 四连 | R-1 / R-2 / R-3 | ✅ |
| T-2 | 描述精简：全部 desc ≤20 字；新增 `tip` 承载细节并挂 tooltip；keymap desc 同步；快捷键页顶部加「重启后生效」说明 | T-1 | P0 | `src/ui/settings.rs`、`src/core/keymap.rs` | 新增长度断言 + G4 四连 | R-4 | ⬜ |
| T-3 | 视觉：`GroupBox` 卡片分组、三级层级、控件右列 240 右对齐、行高改 `min_h(44)`、窗口 160+660=840 | T-2 | P1 | `src/ui/settings.rs` | G4 四连 + 人工 AC-6 / AC-12 | R-5 | ⬜ |
| T-4 | 「高级」受控折叠（`advanced_open`）+ 搜索命中自动展开与高亮 | T-1 | P1 | `src/ui/settings.rs` | G4 四连 + 人工 AC-7 / AC-8 | R-1 / R-7 | ⬜ |
| T-5 | AI 6 个列表面板改用与元数据分组同款容器 | T-3 | P2 | `src/ui/settings.rs` | G4 四连 + 人工 AC-11 | R-6 | ⬜ |

（计划阶段填充）

## 待用户拍板

- [x] Q1 低频项处置 → 收进「高级」折叠
- [x] Q2 分级表 → 照单执行
- [x] Q3 `preview.debug_scroll_sync` → 移除
- [x] Q4 快捷键 → 独立分类
- [x] Q5 视觉力度 → 卡片分组 + 加宽 840
- [x] Q6 精简信息去向 → tooltip

## 门禁命令输出摘要

### T-1 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning（仅第三方 block v0.1.6 的 future-incompat 提示）
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 618 passed, 0 failed, 3 ignored（新增 3 条：高级白名单一致 / keymap 归属 / debug 项缺席）
git diff --stat            : src/ui/settings.rs +94-14、tests/ui/settings.rs +66-1（与计划一致）
```

## 偏差记录

1. **快捷键分类图标**：设计写 `IconName::Keyboard`，实测 `gpui_component::IconName`
   的兼容子集（`gpui-kit-assets/default-icons.txt`）里没有 keyboard，
   改用 `IconName::SquareTerminal`，代码注释已注明。
2. **`tip` 字段推迟到 T-2**：T-1 只引入 `tier`，`tip` 等描述精简时一起加，
   否则会有 `field is never read` 警告（G4 要求 0 warning）。
