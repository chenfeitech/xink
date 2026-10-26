# 设计：`editor.keymap` 接线（快捷键可覆盖）

> 出处：`docs/config/config-wiring-design.md` §4 第 13 项、§6「P2 结构性接线」。
> 需求：`docs/config/config-management-requirements.md` C3.12（预留"动作名 → 按键串"结构）。

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| `editor.keymap: BTreeMap<String, String>` 存得下、导得出，但**没有任何消费点** | `src/core/config.rs:164-165`；全仓唯一读取处是默认值与序列化 |
| 键位全部硬编码在启动阶段 | `src/main.rs:59-84`（14 条 `KeyBinding::new`） |
| 设置界面**连入口都没有** | `src/ui/settings.rs` 的 `all_fields()` 无 keymap 项 |
| 同类死配置已按"要么接线、要么移除"处置完毕 | `language` / `color_scheme` 2026-10-21 移除，`spell_check` 2026-10-20 移除 |
| 生效时机属 **L3 重启**：绑定只在启动时下发一次 | `config-wiring-design.md` §5 |
| `KeyBinding::new` **panic 于解析失败**，但有可先校验的路径 | `gpui-pre-0.3.9/src/keymap/binding.rs:32-45`（`new` → `load().unwrap()`）；`Keystroke::parse` 公开且返回 `Result`（`platform/keystroke.rs:120`） |

结论：按文档既定方案**接线**（不移除）——键位表是用户可感知的能力，且需求 C3.12 明确预留。

## 2. 目标 / 非目标

**目标**

- G1：配置文件里的 `editor.keymap` 能覆盖内置键位，重启后生效。
- G2：设置界面可查看并修改每个动作的键位；清空即回到默认。
- G3：非法键串、未知动作名、重复键位都不会让应用起不来或键位错乱。
- G4：默认键位与今天**逐条一致**（接线本身零行为变化）。

**非目标**

- 不做快捷键录制器（按键捕获）：`settings-ui-design.md` §46 已定为不做，本期仍是文本输入。
- 不做 Vim 式多段序列（`cmd-k cmd-s`）：仅校验支持多段，**不承诺**行为正确，文档中标注。
- 不做"改完立即生效"：属 L3，UI 明确标注"重启后生效"。

## 3. 方案

### 3.1 新增 `core/keymap.rs`（领域层，可单测）

```rust
/// 一个可绑定动作：与 `src/actions.rs` 的 action 名一一对应
pub struct Binding {
    /// action 名，也是 `editor.keymap` 的键
    pub action: &'static str,
    /// 设置项的 id（同时也是输入框索引键）
    pub id: &'static str,
    /// 设置界面显示的中文名
    pub label: &'static str,
    /// 默认键位；空串 = 默认不绑定
    pub default: &'static str,
    /// 键位上下文（如 `Some("FilesPanel")`）
    pub context: Option<&'static str>,
}

pub const BINDINGS: &[Binding] = &[ /* 16 项，见 §3.2 */ ];

/// 归一化：去空白 + 转小写
pub fn normalize(raw: &str) -> String;
/// 键串是否合法（逐段 `Keystroke::parse`）
pub fn is_valid(raw: &str) -> bool;
/// 覆盖表 → 最终绑定列表（默认键位为准，覆盖优先；非法/空/未知一律跳过）
pub fn resolve(overrides: &BTreeMap<String, String>) -> Vec<KeyBinding>;
```

- `resolve` 顺序：按 `BINDINGS` 顺序生成 `(action, keystroke)`；覆盖值经 `normalize` +
  `is_valid` 校验后才采用，否则用默认值；默认值为空则该动作不产生绑定。
- **冲突处理**：同一键串被多个动作占用时保留 `BINDINGS` 中靠前的那个，
  其余丢弃并在启动时 `eprintln!` 一条警告（不弹窗、不阻断）。
- `resolve` 内部用 `KeyBinding::new`（已先校验，不会 panic）。

### 3.2 绑定表（与 `src/main.rs:59-84` 现状逐条对齐）

| action | 默认键位 | 上下文 |
|---|---|---|
| `OpenFolder` | `cmd-o` | — |
| `NewNote` | `cmd-n` | — |
| `Save` | `cmd-s` | — |
| `ClosePanel` | `ctrl-w` | — |
| `RenameItem` | `f2` | `FilesPanel` |
| `OpenSettings` | `cmd-,` | — |
| `FormatBold` | `cmd-b` | — |
| `FormatItalic` | `cmd-i` | — |
| `FormatStrikethrough` | `cmd-shift-x` | — |
| `FormatInlineCode` | `cmd-e` | — |
| `FormatLink` | `cmd-k` | — |
| `ToggleTheme` | `cmd-shift-l` | — |
| `OpenCommandPalette` | `cmd-p` | — |
| `OpenSearch` | `cmd-shift-f` | — |
| `ToggleAiChat` | `cmd-shift-a` | — |
| `NewFolder` | （空） | — |
| `DeleteItem` | （空） | — |

`ClosePanel` 来自 gpui-kit（`gpui_kit::component::dock::ClosePanel`），不是 `actions.rs` 里的
项目 action，但同样是 `Action`，一并纳入表内。

### 3.3 `main.rs` 改为按配置下发

```rust
let cfg = AppConfig::load();
cx.bind_keys(core::keymap::resolve(&cfg.editor.keymap));
```

仅此一处：启动阶段硬编码表被 `resolve` 取代，配置为空时输出与今天完全一致（G4）。

### 3.4 设置界面入口

复用现有元数据表，**不加新控件类型**：每个动作一项 `text_field`。

- id：`editor.keymap.<Action>`（如 `editor.keymap.Save`），分类 `编辑器`、分组 `快捷键`。
- `text_value`：取 `cfg.editor.keymap.get(action)`，**没有覆盖时返回空串**
  （这样默认配置下 `is_default` 为真、不显示 ⟲）。
- `text_set`：空串 → 从 map 移除（回到默认）；非空 → 插入归一化后的值。
- 说明文案统一含"（重启后生效）"，并把默认键位写进 `desc`
  （如 "保存当前笔记 · 默认 cmd-s（重启后生效）"），用户清空后仍能看出默认是什么。
- 输入框 placeholder 显示默认键位：`text_field` 现有构造只传 id/标题/说明，
  placeholder 需要在 `SettingsView::new` 建输入框时按 `field.desc` 之外的来源取——
  **改法**：`SettingField` 增加一个 `placeholder: &'static str`（默认空），
  建输入框时用它；keymap 项填 `default`。

### 3.5 配置校正（`sanitize`）

`AppConfig::sanitize` 里对 `editor.keymap` 做一次校正，与既有 `limit::*` 钳制同一套路：

- 逐项 `normalize`；空串 → 删除该键；
- `!is_valid` → 删除该键（不留下"存了但不生效"的垃圾）；
- 未知 action 名 → 删除该键。

## 4. 数据模型与持久化

- 字段不变：`editor.keymap: BTreeMap<String, String>`（键 = action 名，值 = 键串）。
- 无新增配置项、无新增落盘文件；`BINDINGS` 是编译期常量，不落盘。
- `SettingField` 新增 `placeholder: &'static str`（仅 UI 元数据，不落盘）。

## 5. 交互流程

1. 设置 → 编辑器 → 快捷键：每行一个动作，输入框显示当前键位（空 = 用默认）。
2. 输入 `cmd-shift-s` → `update_config` 立即落盘；行尾出现 ⟲；标注提示重启后生效。
3. 点 ⟲ 或清空输入框 → 从 map 删除 → 回到默认键位（重启后）。
4. 重启：`main.rs` 读配置 → `resolve` → `bind_keys`。

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 键串非法（如 `cmd-` / `cmd-ctrl-`） | `sanitize` 时删除；即便绕过（手改文件）`resolve` 也跳过 → 用默认键位 |
| 未知 action 名（手改文件塞进去） | `sanitize` 删除；`resolve` 忽略 |
| 两个动作配了同一键串 | `resolve` 保留靠前者，启动日志一条警告 |
| 覆盖值写空串 | 视为"用默认" |
| 大小写 / 多余空格 | `normalize` 统一（`CMD-S` ≡ `cmd-s`） |
| 用户把 `cmd-s` 改成 `cmd-shift-s` | 重启后 Save 用新键位，旧键位释放 |
| 配置文件被清空 / 字段缺失 | map 为空 → 全部走默认 |

## 7. 测试计划

新增 `tests/core/keymap.rs`（`#[path]` 挂到 `src/core/keymap.rs`）：

| 用例 | 断言 |
|---|---|
| `defaults_match_hardcoded_bindings` | 空覆盖下 `resolve` 输出 15 条，键位与 `BINDINGS` 默认逐条一致（G4 防退化） |
| `override_replaces_default` | `Save → cmd-shift-s` 生效且条数不变 |
| `empty_override_falls_back_to_default` | 空串 → 用默认 |
| `invalid_keystroke_is_ignored` | `cmd-` → 用默认 |
| `unknown_action_is_ignored` | 键名不在 `BINDINGS` → 不产生绑定 |
| `normalize_trims_and_lowercases` | `CMD-S` / ` cmd-s ` → `cmd-s` |
| `duplicate_keystroke_keeps_the_first` | 两个动作同键 → 只保留靠前者 |
| `sanitize_drops_invalid_and_unknown_entries` | 配一个 `tests/core/config.rs` 用例 |

现有设置描述表测试（`tests/ui/settings.rs`）无需改动即可覆盖新项：
id 前缀 `editor.` 合法、改值后出现 ⟲、重置后回到默认。

## 8. 人工验收清单

- [ ] 设置 → 编辑器 → 快捷键：17 行（15 条有默认键位 + 2 条为空），说明里能看到默认键位
- [ ] 把「保存」改成 `cmd-shift-s`，重启：⌘S 不再保存、⌘⇧S 保存
- [ ] 点 ⟲ 恢复，重启：⌘S 恢复保存
- [ ] 手改配置文件塞一个非法键串 `cmd-`，重启：应用正常启动，该动作回默认键位
- [ ] 两个动作填同一个键串，重启：靠前的生效，控制台有一条警告
- [ ] 未改任何键位时，全部快捷键行为与接线前一致

## 9. 风险与开放问题

| # | 类型 | 内容 | 处置 |
|---|---|---|---|
| 1 | 风险 | 键位只在启动下发，改了要重启才知道 | 接受 L3（`config-wiring-design.md` §5 Q4），UI 文案标注"重启后生效" |
| 2 | 风险 | `KeyBinding::new` 解析失败会 panic | 先 `Keystroke::parse` 校验，再 `new`；`resolve` 只走校验过的值 |
| 3 | 风险 | 多段序列（`cmd-k cmd-s`）解析通过但行为未验证 | 校验放行，文档标注"不承诺"；如验收发现问题再收窄为单段 |
| 4 | 开放 | 是否需要"恢复全部默认"按钮 | 底部已有"整体恢复默认"，本期不额外加 |
| 5 | 开放 | `NewFolder` / `DeleteItem` 默认无键位 | 纳入表内（默认空），用户可自行绑定；也可后续移除 |

## 10. 实施步骤

| 步 | 内容 | 可独立验证 |
|---|---|---|
| S1 | 新增 `core/keymap.rs`：`Binding` / `BINDINGS` / `normalize` / `is_valid` / `resolve` + `tests/core/keymap.rs` | 编译通过；新单测全绿，`defaults_match_hardcoded_bindings` 锁死现状 |
| S2 | `main.rs` 换 `bind_keys(resolve(...))`；`config.rs::sanitize` 校正 keymap | 编译通过；不配 keymap 时行为与现状一致 |
| S3 | 设置界面：`SettingField` 加 `placeholder`；`all_fields()` 追加 17 项 keymap | 编译通过；描述表测试仍全绿 |
| S4 | 文档：`docs/INDEX.md` 登记本文档、§10 移除 keymap 缺口行；`config-wiring-design.md` §4 第 13 项改"已接线" | 文档自洽 |
