# 设置界面优化（梳理 / 精简 / 美化）— 技术设计

> 状态：**待用户确认（G2 未过）**，确认前不动 `src/`
> 需求：[`settings-ui-polish-requirements.md`](./settings-ui-polish-requirements.md)（R-1…R-7、AC-1…AC-12）
> 上一版设计：[`settings-ui-design.md`](./settings-ui-design.md)
> 状态文件：[`docs/exec-plans/settings-ui-polish.md`](../exec-plans/settings-ui-polish.md)

---

## 1. 现状与问题（附证据）

| # | 现状 | 证据 |
|---|---|---|
| S1 | 设置项 57 项，元数据驱动，唯一清单 `all_fields()` | `src/ui/settings.rs:352-1016`、`src/ui/settings.rs:1023-1053` |
| S2 | 4 个分类（基本/编辑器/预览/AI），`SettingsTab` 枚举；快捷键 16 项混在编辑器 | `src/ui/settings.rs:51-82`、`src/core/keymap.rs:36-173` |
| S3 | 分组**无注册表**，靠"group 字符串与上一项不同就插标题"的顺序推断 | `src/ui/settings.rs:2806-2822` |
| S4 | 单项行高固定 `ROW_H = 52`，描述最长 89 字（≥30 字者 27 条） | `src/ui/settings.rs:1291`、`src/ui/settings.rs:2933`、各条目 desc |
| S5 | 分组标题与项标题同为 `text_sm`，仅差 `font_weight(600)`；无卡片容器 | `src/ui/settings.rs:2813`、`src/ui/settings.rs:2941` |
| S6 | 控件宽度不一：`Input w(240)`、数值 `w(64)` + 步进按钮、Switch、segmented、dropdown | `src/ui/settings.rs:307`、`src/ui/settings.rs:1148`、`src/ui/settings.rs:212-217` |
| S7 | 窗口固定宽 `SIDEBAR_W(160) + CONTENT_W(560) = 720` | `src/ui/settings.rs:1286-1302`、`src/ui/settings.rs:3104-3156` |
| S8 | AI 另有 6 个列表面板，各自用 `border_t_1` 分隔 + `text_sm/600` 标题，与元数据分组不同款 | `src/ui/settings.rs:1428-2671` |
| S9 | 已有 8 条设置项单测锁死 id 命名空间 / 元数据完整 / reset 幂等 / unavailable 空集 | `tests/ui/settings.rs:26-140` |
| S10 | 组件库提供 `GroupBox`（`title` / `content_style` / `footer` / 三种 variant）与 `Accordion`；本项目均未使用 | `gpui-component-0.8.0/src/group_box.rs:73-120`、`.../accordion.rs:18-82` |

问题归纳：主次不分（S1/S2/S3）、描述过长且行高固定（S4）、层级与容器缺失（S5/S8）、控件列不齐（S6）、宽度偏窄（S7）。

---

## 2. 目标 / 非目标

**目标**

| # | 目标 | 对应 R |
|---|---|---|
| D1 | 57 项分「核心 / 高级」两级，高级默认折叠、搜索可达 | R-1 |
| D2 | 移除 `preview.debug_scroll_sync`（配置字段保留） | R-2 |
| D3 | 快捷键独立成第 5 个分类 | R-3 |
| D4 | 全部描述 ≤ 20 字，长说明进 tooltip | R-4 |
| D5 | 卡片分组 + 三级层级 + 控件右对齐一列 + 窗口加宽到 840 | R-5 |
| D6 | AI 6 个面板与元数据分组同款容器 | R-6 |
| D7 | 搜索命中高级项时自动展开并定位 | R-7 |

**非目标**

- 不新增设置项、不改 `config.json` 结构与 `sanitize` 语义、不改 `src/core/config.rs` 字段
- 不改设置项 id、不改 `FieldKind` 五个变体
- 不换用 `gpui_component::setting::*` 框架（见 §3-B）
- 不改 AI 6 个面板的业务逻辑（只统一容器与标题层级）
- 不做快捷键录制器、主题自定义、Profile、云同步

---

## 3. 方案概述

### 3.1 选定方案 A：元数据加一层「tier」+ 自绘卡片与折叠

在**现有元数据机制上做加法**，不动渲染骨架的整体结构（左分类栏 / 右滚动列 / 搜索 / footer 保持）：

| # | 做法 | 落在哪 |
|---|---|---|
| A1 | `SettingField` 新增 `tier: FieldTier`（`Core` / `Advanced`），57 项逐条标注 | `src/ui/settings.rs:121-142`、`all_fields()` |
| A2 | 「高级」渲染为一个**受控**折叠卡片：状态存 `SettingsView.advanced_open: bool`，点击表头 toggle + `cx.notify()` | `SettingsView` 结构体（`settings.rs:1235-1278`）+ 渲染 |
| A3 | 分组容器改用 `GroupBox`（variant `Fill`，带 `title`，可选 `footer` 作分组说明） | 内容列渲染（`settings.rs:2805-2851`） |
| A4 | 层级：分类页标题（新增，`text_base/600`）> 分组标题（GroupBox title）> 项标题 `text_sm` > 描述 `text_xs/muted` | 同上 |
| A5 | 控件列统一 `w(px(CONTROL_W=240))` + `justify_end`，各控件在该列内右对齐 | `render_field`（`settings.rs:2921-2999`） |
| A6 | 行高去掉固定 `ROW_H`，改 `py_2` + `min_h(px(44))`，长描述不再被裁切 | `settings.rs:1291`、`settings.rs:2933` |
| A7 | 窗口加宽：`SIDEBAR_W 160→180`、`CONTENT_W 560→660`（合计 840）；保留 `min(viewport - 32)` 兜底 | `settings.rs:1286-1302`、`settings.rs:3104-3156` |
| A8 | 新增 `SettingsTab::Keymap`（图标 `IconName::Keyboard`，证据：`gpui-kit-assets-0.8.0/assets/icons/keyboard.svg`，build.rs 转 PascalCase），16 项迁过去；id 保持 `editor.keymap.*` 不变 | `settings.rs:51-82`、`keymap_fields()` |
| A9 | 描述精简到 ≤20 字；精简掉的内容进新字段 `tip`，悬停显示 | `SettingField.desc` / 新增 `tip` |
| A10 | AI 6 个面板改用 A3 同一款容器函数（`group_container(title, desc)`） | `settings.rs:1428-2671` |

### 3.2 被否决方案

| # | 方案 | 否决理由 |
|---|---|---|
| **B** | 整体换用 `gpui_component::setting::*`（`Settings` / `SettingPage` / `SettingGroup` / `SettingItem`） | ① 它的左栏是**可折叠分组**形态，与当前"4 个平铺分类 + 图标"不同，等于重做信息架构（`setting/settings.rs:34-107`）；② AI 的 6 个列表型面板要用 `SettingGroup::footer()` 或 `SettingItem::element()` 硬塞，语义扭曲（`setting/group.rs:18-157`）；③ 57 项元数据的 `disabled` / `is_default` / `reset` 回调要整体迁移到它的 `on_reset(is_dirty, reset)` 模型，改造面远超本次诉求；④ 用户已明确不选（需求 §6-Q5） |
| **C** | 「高级」折叠用 `Accordion` 组件 | `Accordion` 是 `RenderOnce`，展开状态存在 `render()` 内的局部 `RefCell<HashSet>`（`accordion.rs:99-155`），每次重绘以 `AccordionItem::open()` 的初始值为准 → **用户展开高级后改一个设置（触发 `update_config` → `refresh_windows`）就会被折回去**。必须受控，故自绘 |
| **D** | 低频项直接从 `all_fields()` 移除（仅保留配置字段） | 用户已选"收纳而非删除"（需求 §6-Q1）；收纳可逆、搜索可达，删除后用户改不了 |
| **E** | 描述精简 = 干脆不要描述 | 违反 R-4「精简掉的信息进 tooltip」；`tests/ui/settings.rs:50-56` 断言 desc 非空，全删会与既有测试冲突 |

---

## 4. 数据模型与持久化

### 4.1 `SettingField` 增改（内存结构，不落盘）

```rust
pub enum FieldTier { Core, Advanced }

pub struct SettingField {
    // …既有字段全部保留…
    tier: FieldTier,     // 新增：默认 Core
    tip: &'static str,   // 新增：tooltip 长说明，可为空
}
```

- `desc` 缩短为 ≤20 字；原长说明迁到 `tip`
- 构造器（`bool_field` / `choice_field` / `number_field` / `text_field`）签名**不变**，tier / tip 用 `with_tier()` / `with_tip()` 追加（沿用既有 `with_meta` / `with_placeholder` 的追加式写法，改动最小）

### 4.2 持久化

- **`config.json` 格式一字不改**，`sanitize()` 不动，旧配置升级后所有值照旧生效
- 分级与分组**只存在代码里**（`tier` 是 UI 层元数据），不写入配置，避免"旧配置带着过期的分组名"
- `preview.debug_scroll_sync`：**`AppConfig` 字段保留**（旧配置里的 `true` 仍生效、不丢数据），只从 `all_fields()` 移除；它仍被预览标题的调试显示读取，因此不存在死字段

### 4.3 快捷键文案

`src/core/keymap.rs:36-173` 的 `BINDINGS[*].desc` 精简（去掉"· 留空用默认 X（重启后生效）"，改为：动作说明 ≤12 字 + placeholder 显示默认键 + 分区顶部一句"改完重启后生效"）。
`tests/core/keymap.rs:22-48` 断言 desc 非空 → 仍满足。

---

## 5. 交互流程

| # | 流程 | 行为 |
|---|---|---|
| F1 | 打开设置（`Cmd-,`） | 左侧 5 个分类（基本 / 编辑器 / 预览 / AI / 快捷键）；右侧当前分类的分组卡片；顶部搜索；底部 footer 不变 |
| F2 | 切分类 | 内容列整体替换为该分类的分组卡片；搜索词清空（现状：切 tab 保留 query，保持现状） |
| F3 | 展开「高级」 | 卡片表头「高级」右侧显示 ▸/▾ 与项数；点击 toggle；状态在 `SettingsView` 内，设置窗口关闭后回到收起 |
| F4 | 搜索命中高级项 | 自动切到该项所属分类 + 展开高级卡片 + 高亮该行（复用既有 `highlight` 机制，`settings.rs:2674-2692`）；不修改 `advanced_open` 本身（用局部变量 `self.advanced_open \|\| hit_advanced`） |
| F5 | 改一个设置项 | 走 `update_config` → 即时生效 + 落盘 + 重绘（`src/app.rs:245-268`）；非默认项出现 ⟲ |
| F6 | 单项重置 | ⟲ 按钮，语义不变 |
| F7 | AI 分类 | 6 个面板仍排在元数据分组之后（现状 `settings.rs:2840-2847`），容器与标题换同款；密钥面板仍在对应模型行下方展开（`settings.rs:2174-2176`） |

---

## 6. 异常与边界

| # | 场景 | 处理 |
|---|---|---|
| E1 | 空搜索词 | 空态「没有匹配的设置项」保留（`EMPTY_H = 64`，`settings.rs:1295`）；高级卡片照常收起 |
| E2 | 搜索无命中 | 同上 |
| E3 | 某分类下核心项为空（如某分类只剩高级项） | 高级卡片照常在最上方渲染；不显示空的分组标题 |
| E4 | 项被禁用（`disabled` 返回原因） | 保持现状：描述替换为禁用原因，控件位置显示「已禁用」（`settings.rs:2958-2982`） |
| E5 | tooltip 为空 | 不加 tooltip（不显示空气泡） |
| E6 | 小屏（宽 < 840 + 32） | 沿用 `min(width - 32)` 兜底（`settings.rs:3110` 附近）；内容列 `min_w_0` + 滚动，**`min_h_0` 三处不能漏**（`settings.rs:2805`、`settings.rs:2862`、`settings.rs:3154`） |
| E7 | 导入非 UTF-8 / 非法 JSON | footer 导入逻辑不动，给明确错误提示，不 panic |
| E8 | 旧 `config.json` 含 `debug_scroll_sync: true` | 字段保留并继续生效（预览标题仍显示调试信息），只是设置界面不再提供开关 |

---

## 7. 测试计划

### 7.1 能自动化（`tests/ui/settings.rs` 由 `src/ui/settings.rs:3158-3161` 用 `#[path]` 挂载）

| # | 测试 | 断言 |
|---|---|---|
| T-1 | 既有 4 条 | id 唯一、id 命名空间、默认即 `is_default`、`reset` 幂等（**必须保持全绿**） |
| T-2 | 既有 `field_metadata_is_complete` | title/desc/group 非空 |
| T-3 | **新增** `descriptions_are_short_enough` | 全部项 `desc` 字符数 ≤ 20（含 keymap 项） |
| T-4 | **新增** `advanced_fields_match_the_documented_set` | 高级集合 == 文档 §3.1 登记的 id 列表（防止随手挪动） |
| T-5 | **新增** `keymap_fields_live_in_their_own_tab` | 16 项 keymap 的 tab 均为 `Keymap`，且不在 `Editor` |
| T-6 | **新增** `tier_is_set_for_every_field` | 每项 `tier` 已显式标注（防止漏标导致默认钻空子） |
| T-7 | G4 四连 | `cargo build` / `clippy --all-targets` 0 warning、`fmt --check` 无 diff、`cargo test` 全绿 |

### 7.2 要人工（视觉 / 手感，交付前登记进 `docs/ACCEPTANCE.md` 第 18 节）

分层是否合意、卡片观感是否接近 Obsidian、控件是否对齐成一列、加宽后小屏是否裁切、高级折叠是否好用、搜索联动、AI 面板是否统一、旧配置是否照旧生效。逐条见需求 §5 的 AC-6…AC-12。

---

## 8. 人工验收清单

**唯一登记处：`docs/ACCEPTANCE.md` §18**（实施时写入，逐条可打勾）。本设计不重复铺开清单，只保留编号映射：

| AC | 一句话 |
|---|---|
| AC-6 | 卡片分组、留白、控件右对齐成一列 |
| AC-7 | 高级默认收起；搜索能自动展开并高亮 |
| AC-8 | 低频项不在主视图 |
| AC-9 | 快捷键独立分类，不再逐项重复「重启后生效」 |
| AC-10 | 旧配置值不丢，⟲ 仍可用 |
| AC-11 | AI 6 面板同款容器；密钥面板仍在对应行下方 |
| AC-12 | 加宽后小屏 / 大屏都不裁切、滚动正常 |

---

## 9. 风险与开放问题

| # | 风险 | 处置 |
|---|---|---|
| K1 | `GroupBox` variant `Fill` 的 `tokens.group_box` 在浅色下对比度可能偏弱 | 人工验（AC-6）；不合意就切 `Outline` |
| K2 | 控件列固定 240 可能容不下 segmented（4 个中文选项） | 分段按钮允许换行或缩字号；人工验后定 |
| K3 | 去掉固定行高后行高不齐 | 用 `min_h(px(44))` 兜底，视觉上仍成列 |
| K4 | 加宽到 840 后，AI 的 6 个面板行（按钮多）可能变挤 | 面板行已 `flex_wrap`（`settings.rs:1997` 附近） |
| K5 | 描述压缩后信息不足 | 全部进 `tip`，人工抽查 tooltip 是否补全 |

**开放问题（默认做法已定，不反对即按此执行）**

| # | 问题 | 默认 |
|---|---|---|
| O1 | 「高级」展开状态要不要记住（跨会话） | **不记**：视图内 bool，关闭设置即回到收起（不引入新持久化字段） |
| O2 | 分组说明（footer）文案由谁写 | 由我按现有 desc 提炼，每分组一句 ≤20 字；不需要说明的分组不写 |

---

## 10. 实施步骤（分期，每期可独立编译验证）

| 期 | 内容 | 涉及文件 | 关联 R | 验收 |
|---|---|---|---|---|
| **P1** 分级与分类 | 新增 `FieldTier` 与 `tier`；57 项标注；新增 `SettingsTab::Keymap`（图标 Keyboard）并迁移 16 项；移除 `preview.debug_scroll_sync` 项 | `src/ui/settings.rs` | R-1 / R-2 / R-3 | T-4 / T-5 / T-6 + 既有 6 条全绿 + G4 四连 |
| **P2** 描述精简 | 全部 desc ≤20 字；新增 `tip` 并挂 tooltip；keymap desc 同步精简；分区顶部加「重启后生效」说明 | `src/ui/settings.rs`、`src/core/keymap.rs` | R-4 | T-3 + G4 四连 |
| **P3** 视觉 | 分组换 `GroupBox`（Fill）；三级层级；控件列 240 右对齐；行高改 `min_h(44)`；窗口 160+660=840 | `src/ui/settings.rs` | R-5 | G4 四连 + 人工 AC-6 / AC-12 |
| **P4** 高级折叠与搜索联动 | `advanced_open` 受控折叠；搜索命中自动展开 + 高亮 | `src/ui/settings.rs` | R-1 / R-7 | G4 四连 + 人工 AC-7 / AC-8 |
| **P5** AI 面板统一 | 6 个面板改用 `group_container()` 同款容器与标题层级 | `src/ui/settings.rs` | R-6 | G4 四连 + 人工 AC-11 |

每期结束跑 G4 四连并把输出摘要贴进 `docs/exec-plans/settings-ui-polish.md`；全部完成后登记 `docs/ACCEPTANCE.md` §18 与 `docs/INDEX.md`。

**分支**：按 2026-10-28 约定，实施在 `feature/settings-ui-polish` 分支上进行，未得明确指示不合并 main。
