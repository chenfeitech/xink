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

需求 ✅ → 设计 ✅ → 计划 ✅ → 实施 ✅ → 测试 ✅ → 质量 ✅（人工验收待做）

## 门禁

| 门禁 | 结果 | 证据 / 输出摘要 |
|---|---|---|
| G1 需求 | ✅ | 文档含目标/非目标/边界/验收四节；R-1…R-7 编号唯一；现状均附 `文件:行号`；§6 拍板清单已清空（2026-10-28 用户拍板：高级折叠 / 照单执行 / 移除 debug_scroll_sync / 快捷键独立分类 / 卡片分组+加宽 840 / 描述进 tooltip） |
| G2 设计 | ✅ | 十章齐全、含 4 个被否决方案；用户确认于 2026-10-28 |
| G3 计划 | ✅ | T-1…T-5 每个任务含依赖、涉及文件、验收命令、关联 R-n，可独立编译验证 |
| G4 实施 | ✅ | 5 个任务各一次提交，四连全绿（见下方摘要） |
| G5 测试 | ✅ | 新增 4 条单测（高级白名单 / keymap 归属 / debug 项缺席 / 描述 ≤20 字）；`cargo test` 619 passed / 0 failed |
| G6 质量 | ✅ | clippy + fmt 干净、无 Blocker；人工验收项已登记 `docs/ACCEPTANCE.md` §18；`docs/INDEX.md` 已回写 |

## 任务

| T-n | 内容 | 依赖 | 优先级 | 涉及文件 | 验收命令 | 关联 R-n | 状态 |
|---|---|---|---|---|---|---|---|
| T-1 | 分级与分类：`FieldTier` + `ADVANCED_IDS` 白名单；快捷键独立成 `SettingsTab::Keymap`；移除 `preview.debug_scroll_sync` 项 | — | P0 | `src/ui/settings.rs`、`tests/ui/settings.rs` | 新增 3 条测试 + G4 四连 | R-1 / R-2 / R-3 | ✅ |
| T-2 | 描述精简：全部 desc ≤20 字；新增 `tip` 承载细节（标题旁 ⓘ 悬停）；keymap desc 同步；快捷键页顶部加「重启后生效」说明 | T-1 | P0 | `src/ui/settings.rs`、`src/core/keymap.rs`、`tests/ui/settings.rs` | 新增长度断言 + G4 四连 | R-4 | ✅ |
| T-3 | 视觉：`GroupBox` 卡片分组、四级层级（页标题/分组标题/项标题/说明）、控件右列 240 右对齐、行高改 `min_h(44)`、窗口 180+660=840 | T-2 | P1 | `src/ui/settings.rs` | G4 四连 + 人工 AC-6 / AC-12 | R-5 | ✅ |
| T-4 | 「高级」受控折叠（`advanced_open`）+ 搜索命中自动展开与高亮 | T-1 | P1 | `src/ui/settings.rs` | G4 四连 + 人工 AC-7 / AC-8 | R-1 / R-7 | ✅ |
| T-5 | AI 5 个列表面板改用与元数据分组同款容器（密钥面板仍就地挂在模型行下） | T-3 | P2 | `src/ui/settings.rs` | G4 四连 + 人工 AC-11 | R-6 | ✅ |
| T-6 | AI 模型分区：Ollama 独立区 + 其余按厂家（base_url 域名）分列；模型行改两行（主行 + 能力小字）；操作列定宽 340 右对齐 | T-5 | P1 | `src/ui/settings.rs`、`tests/ui/settings.rs` | 新增加 3 条单测 + G4 四连 + 人工 AC-15 | R-8 / R-9 / R-10 | ✅ |
| T-7 | 快捷动作：提示词移到名称下一行（预览截断 100 字）；「编辑」改行内就地编辑（保存 / 取消在这一行），底部表单只管新增 | T-5 | P1 | `src/ui/settings.rs`、`tests/ui/settings.rs` | 新增 1 条单测 + G4 四连 + 人工 AC-17 | R-11 / R-12 | ✅ |

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

### T-2 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 619 passed, 0 failed, 3 ignored（新增 descriptions_are_short_enough）
git diff --stat            : src/ui/settings.rs +155-48、src/core/keymap.rs +37-20、tests/ui/settings.rs +16（与计划一致）
```

描述精简明细：37 项超长（20 项元数据 + 17 项快捷键）全部压到 ≤20 字，
最长由 88 字（preview.sync_mode）降到 6 字；被压掉的内容进了 `TIPS`（17 条）。

### T-3 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 619 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/settings.rs +51-25（与计划一致）
```

### T-4 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 619 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/settings.rs +36-1（与计划一致）
```

### T-5 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 619 passed, 0 failed, 3 ignored
git diff --stat            : src/ui/settings.rs +40-56（与计划一致）
```

### T-6 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 622 passed, 0 failed, 3 ignored（新增 3 条：厂家归组 / 数字压缩 / 能力小字）
git diff --stat            : src/ui/settings.rs +477-267、tests/ui/settings.rs +62-4（与计划一致）
```

T-6 拍板（用户 2026-10-28）：厂家按 base_url 域名自动归组（不改数据结构）、
只做视觉分区仍逐模型配、能力小字含能力+上下文+工具、无地址模型进「其他」区。

### T-7 提交前（2026-10-28）

```text
cargo build                : Finished，0 warning
cargo clippy --all-targets : 0 warning
cargo fmt --check          : no diff
cargo test                 : 623 passed, 0 failed, 3 ignored（新增 truncate_chars 单测）
git diff --stat            : src/ui/settings.rs +121-25、tests/ui/settings.rs +11（与计划一致）
```

## 偏差记录

1. **快捷键分类图标**：设计写 `IconName::Keyboard`，实测 `gpui_component::IconName`
   的兼容子集（`gpui-kit-assets/default-icons.txt`）里没有 keyboard，
   改用 `IconName::SquareTerminal`，代码注释已注明。
2. **`tip` 字段推迟到 T-2**：T-1 只引入 `tier`，`tip` 等描述精简时一起加，
   否则会有 `field is never read` 警告（G4 要求 0 warning）。
3. **「高级」放在每个分组卡片内，而不是全局一张卡片**：设计文档 §3.1
   写的是"收进一个默认折叠的「高级」卡片"。实现时发现全局一张会把
   图标大小、字体行高、JSON 缩进、keep_alive 混在一起，分区反而更乱；
   改成每个分组卡片底部一段「高级（N 项）」，分组语义保留，
   展开状态仍是全局一个 `advanced_open`。
4. **提示挂在标题旁的 ⓘ 而非整行 tooltip**：`div().tooltip()` 要的是
   `Fn(&mut Window, &mut App) -> AnyView`，而 `&mut App` 上没有 `new_view`，
   为一句提示单建一个 View 不划算，改用 `Button::tooltip(&str)`（框架已支持）。
