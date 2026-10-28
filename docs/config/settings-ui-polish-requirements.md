# 设置界面优化（梳理 / 精简 / 美化）— 需求规格

> 状态：**G1 未过**（待用户拍板清单见 §6，未清空）
> 关联文档：[`settings-ui-design.md`](./settings-ui-design.md)（上一版设计，已实现）、
> [`config-management-requirements.md`](./config-management-requirements.md)、
> [`config-wiring-design.md`](./config-wiring-design.md)
> 落地位置（预计）：`src/ui/settings.rs`、`src/core/keymap.rs`（若调整快捷键文案）

---

## 1. 背景与目标

### 1.1 现状（全部附证据）

设置界面是 **Dialog 浮层**（`src/ui/settings.rs:3104-3156`），宽 `SIDEBAR_W + CONTENT_W = 720`
（`settings.rs:1286-1302`），左 160 分类栏 + 右滚动内容列 + 顶搜索 + 底 footer（`settings.rs:2704-2886`）。
设置项**元数据驱动**，唯一清单是 `all_fields()`（`src/ui/settings.rs:352-1016`）。

| 现状 | 证据 |
|---|---|
| 设置项 **57 项**（含快捷键 16 项，由 `keymap_fields()` 生成） | `settings.rs:352-1016`、`settings.rs:1023-1053` |
| 4 个分类：基本 / 编辑器 / 预览 / AI；分组靠**顺序推断**（`group` 是纯字符串，无注册表） | `settings.rs:51-82`、`settings.rs:2806-2822` |
| 单设置项行高固定 `ROW_H = 52` | `settings.rs:1291`、`settings.rs:2933` |
| 描述 ≥30 字约 **27 条**（占 47%），最长 89 字（`preview.sync_mode`），16 条快捷键中 14 条 ≥30 字且句式雷同 | `settings.rs:196-237` 各条目 desc、`src/core/keymap.rs:36-173` |
| 仅 2 项有条件禁用；`unavailable` 标记**当前为空集**且有单测锁死 | `settings.rs:520-527`、`settings.rs:717-725`、`tests/ui/settings.rs:126-140` |
| AI 分类另有 6 个**列表型面板**（路由 / 快捷动作 / 模型 / 密钥 / 快照 / Skills），**不参与搜索**（`query.is_empty()` 才渲染），各自维护本地状态 | `settings.rs:2840-2847`、`settings.rs:1428-2671` |
| 只用了 9 个主题 token；**未使用** `gpui_component::setting::*`（组件库自带 Settings/SettingGroup/SettingItem/GroupBox） | `settings.rs:22`；`gpui-component-0.8.0/src/setting/mod.rs:1-11` |

三个痛点对应本次三项诉求：

| # | 痛点 | 证据 |
|---|---|---|
| P1 | **主次不分**：57 项平铺，字体行高、JSON 缩进、keep_alive、调试开关等低频项与主题、字号混排 | 分组清单见 §3.1 |
| P2 | **描述过长**：固定行高 52 下，27 条长描述挤压换行，信息密度高 | `settings.rs:1291` + 最长 89 字 |
| P3 | **层级弱**：分组标题与设置项标题同为 `text_sm`，仅靠 `font_weight(600)` 区分；无卡片容器、无分组说明 | `settings.rs:2808-2822`、`settings.rs:2921-2999` |

### 1.2 目标

| # | 目标 | 衡量方式 |
|---|---|---|
| G1 | 主视图只留高频项，低频项收进「高级」且**可搜索到** | 主视图项数 ≤ 30；高级项搜索命中仍可跳转 |
| G2 | 描述一句话说清 | 全部 desc ≤ 20 字（**可自动断言**），长说明进 tooltip |
| G3 | 分区清晰、层级分明、控件对齐统一 | 分组用卡片容器；分组标题 / 项标题 / 描述三级字号与颜色可分；控件右对齐在同一列 |
| G4 | 不丢配置 | 「隐藏」= 收纳而非删除；已配过的值照常生效，出现 ⟲ 重置入口 |

---

## 2. 非目标

- **不新增配置项**：本次只做梳理、精简与视觉重排，不引入新的设置能力
- **不改持久化格式**：`config.json` 字段与 `sanitize` 语义不动，旧配置升级后不丢值
- **不做快捷键录制器**（沿用 `settings-ui-design.md` §1.3 的结论，仍是文本输入）
- **不做主题配色自定义**：仍只在既有明暗 + 代码高亮预设中选
- **不做配置 Profile 切换 / 云同步**
- **不重构 AI 的 6 个列表面板的业务逻辑**（只统一它们的视觉容器与标题层级）
- **不改动 `core/` 配置层**（`src/core/config.rs`）的字段与默认值

---

## 3. 功能模块

### 3.1 R-1 配置项分级：核心 / 高级

把 57 项分为两级，分级结果**以数据形式挂在元数据上**（新增 `SettingField.tier` 或等价字段），
渲染时「高级」整体收进一个默认折叠的卡片，**配置项本身一条都不删**：

| 级别 | 呈现 | 数量目标 |
|---|---|---|
| 核心 | 主视图直接可见 | ≤ 30 项（不含快捷键） |
| 高级 | 折叠卡片内，默认收起；**搜索命中时自动展开并跳转** | 其余全部 |

建议划分（**待用户拍板**，§6-Q2）：

| 分组 | 建议进「高级」的项 | 保留在主视图 |
|---|---|---|
| 基本 · 外观 | — | `basic.theme`、`highlight.theme` |
| 基本 · 目录树 | `basic.icon_size`、`basic.tree_font.family`、`basic.tree_font.line_height`、`basic.tree_font.weight` | `basic.show_all_files`、`basic.tree_font.size` |
| 编辑器 · 编辑行为 | — | `editor.auto_save`、`editor.auto_save_delay_ms`、`editor.indent.use_tabs`、`editor.indent.tab_size`、`editor.show_line_numbers`、`editor.soft_wrap` |
| 编辑器 · 字体与排版 | `editor.font.line_height`、`editor.font.weight` | `editor.font.family`、`editor.font.size` |
| 编辑器 · 数据文件 | `editor.data_file.indent`、`editor.data_file.max_format_bytes` | — |
| 预览 · 渲染 | — | `preview.image_display`、`preview.image_width_percent`、`preview.link_open` |
| 预览 · 排版 | `preview.font.line_height`、`preview.font.weight`、`preview.max_width` | `preview.font.family`、`preview.font.size`、`preview.density` |
| 预览 · 同步 | `preview.debug_scroll_sync`（**另见 R-2，建议直接移除**） | `preview.sync_scroll`、`preview.sync_mode` |
| AI · 连接 | `ai.ollama.keep_alive`、`ai.ollama.num_ctx` | `ai.enabled`、`ai.ollama.base_url` |
| AI · 生成 | `ai.timeout_secs`、`ai.max_context_chars`、`ai.max_steps` | `ai.output_language` |
| AI · 扩展 | — | `ai.skills_enabled` |

### 3.2 R-2 移除冗余项

只有**明确判据**为冗余的项才删除（删 = 从 `all_fields()` 移除，配置字段仍保留以支持旧 `config.json`）：

| 候选 | 判据 |
|---|---|
| `preview.debug_scroll_sync` | 纯排查用（描述原文：`在预览面板标题显示两端当前的源码行号与偏差（排查滚动同步用）`），正常用户不会开；Debug 能力不必占设置列表 |

> 其余项一律**只收纳不删除**，避免"用户配过的东西凭空消失"。

### 3.3 R-3 快捷键独立分区

16 项快捷键（`src/core/keymap.rs:36-173`）单独成区，不再混在「编辑器」里：

- 分类栏新增 **快捷键** 一项（与 Obsidian 一致），并从「编辑器」移除该分组
- 每项保留：动作名 + 键位输入 + 默认键位提示（现为 placeholder，`settings.rs:331-335`）
- 「（重启后生效）」这类重复 16 遍的后缀，改为**分区顶部一句说明**，不再逐项重复

### 3.4 R-4 描述精简

- 全部 `desc` **≤ 20 字**，由单测断言（AC-2）
- 精简掉的细节迁到 **tooltip**（悬停可见），不丢失信息
- 快捷键 16 条统一句式：只留动作说明，默认键位与重启提示走 placeholder / 分区说明
- 阈值与单位优先用控件表达（已有 `−` / `+` 步进器与单位后缀，`settings.rs:1119-1161`），不再写进描述

### 3.5 R-5 视觉：分区清晰、层级分明、控件对齐

参照 Obsidian 设置界面（左分类 + 右滚动 + 卡片式分组 + 悬停说明）：

| # | 要求 | 现状依据 |
|---|---|---|
| V1 | 分组用**卡片容器**承载（`GroupBox` 或自绘），带内边距与圆角 | 现无容器，仅一行 `text_sm/600` 标题（`settings.rs:2808-2822`） |
| V2 | 三级层级可辨：分类页标题 > 分组标题 > 设置项标题 > 描述 | 分组标题与项标题同为 `text_sm`（`settings.rs:2813`、`settings.rs:2941`） |
| V3 | 控件**右对齐在同一列**，行与行之间控件左边缘对齐 | 现为 `justify_between` + 右侧 `flex_none`（`settings.rs:2933-2937`），但不同控件宽度不一（分段按钮 / 下拉 / 步进器 / 输入框） |
| V4 | 留白合理：分组之间间距 > 组内项间距；内容区左右内边距统一 | 现 `gap_1` / `px_2` / `py_1` 混用 |
| V5 | 分组可带一句说明（分组说明 ≠ 项描述） | `GroupBox` 支持 `description()`；现无 |
| V6 | 窗口尺寸适配：加宽以容纳卡片与更宽的控件列 | 现固定 720（`settings.rs:1300-1302`） |

### 3.6 R-6 AI 列表面板统一视觉

6 个面板（路由 / 快捷动作 / 模型 / 密钥 / 快照 / Skills，`settings.rs:1428-2671`）改用与 R-5 一致的
分组容器与标题层级；**位置与交互逻辑不变**（尤其密钥面板必须就地挂在该模型行下，`settings.rs:2174-2176`）。

### 3.7 R-7 搜索覆盖「高级」与面板

- 搜索命中「高级」项时：自动切到对应分类 + **展开该高级卡片** + 高亮定位（现已有 `highlight` 高亮，`settings.rs:2674-2692`）
- AI 的 6 个面板**仍不参与搜索**（保持 `query.is_empty()` 条件，避免改动面失控）

---

### 3.8 R-8 AI 模型分区：Ollama 独立，其余按厂家分列（2026-10-28 追加）

| # | 要求 |
|---|---|
| R-8 | 模型区拆成多张卡片：**Ollama 单独一区**（含"从 Ollama 拉取"），其余模型**按厂家各一区**；厂家由 `base_url` 的域名推出（不改数据结构）；没填地址的进「其他」区，「其他」排最后 |
| R-9 | 模型能力**另起一行**小字展示：能力 + 上下文上限 + 工具结论（分"声明 / 实测"）+ 是否停用，不再与模型名、按钮挤在同一行 |
| R-10 | 模型行上下对齐：主行只放模型名，操作（设为默认 / 启用 / 类型 / 密钥 / 删除 / 测试）定宽 340 并右对齐；上下各行操作列左边缘对齐成一列 |

**非目标（追加）**：不引入厂家级配置表，同一厂家的 `base_url` 与密钥仍**逐模型**配置（只做视觉分区）。

### 3.9 R-11 / R-12 快捷动作：提示词另起一行 + 行内编辑（2026-10-28 追加）

| # | 要求 |
|---|---|
| R-11 | 快捷动作的提示词**显示在名称下方一行**（小字、预览截断 100 字），不再与名称、按钮挤在同一行 |
| R-12 | 「编辑」改为**在当前行内直接编辑**（名称 + 提示词输入框就地展开，行内保存 / 取消），不再复用列表底部那张"新增"表单；底部表单只管新增，编辑态时不显示 |

交互对齐的对象：与"点哪一行改哪一行"的表格编辑一致（就地编辑，不跳到页面另一处）。
配置数据结构、id、持久化**均不变**。

## 4. 边界与约束

逐条过边界九项（`requirement-clarify/references/boundary-checklist.md`）：

| # | 边界项 | 判定 | 处理方式 |
|---|---|---|---|
| 1 | 空输入 | **适用** | 空搜索词 → 空态（现 `EMPTY_H = 64`，`settings.rs:1295`）；空描述项：R-4 允许极简项**无描述**，需同步放宽 `field_metadata_is_complete` 断言 |
| 2 | 超大输入 | 不适用 | 设置界面不涉及大文本；字体下拉的字体数量由系统决定，非本需求范围 |
| 3 | 非法路径 | 不适用 | 设置项不含路径输入（Ollama 地址是 URL 文本，沿用现状） |
| 4 | 并发写 | 不适用 | 配置写盘走既有 `update_config`（`src/app.rs:245-268`） |
| 5 | 外部程序改文件 | 不适用 | 配置由本应用独占写 |
| 6 | 崩溃后半截文件 | 不适用 | 同上，写盘机制不变 |
| 7 | 多库切换 | 不适用 | 配置是全局的，不随库切换 |
| 8 | 隐藏与忽略文件 | 不适用 | — |
| 9 | 非 UTF-8 | **适用** | footer 的导入配置（`settings.rs:3040-3084`）读外部 JSON，非法 / 非 UTF-8 必须给明确提示且不 panic；本次不改动其逻辑，仅保证不被视觉重构误伤 |

补充约束：

| # | 约束 | 理由 |
|---|---|---|
| C1 | 设置项 **id 一律不变** | `tests/ui/settings.rs:38-47` 断言 id 命名空间；重置与搜索按 id 索引 |
| C2 | `FieldKind` 五个变体保留 | `tests/ui/settings.rs:84-118` 按 `Text` / `Number` 断言行为 |
| C3 | `unavailable` 保持空集 | `tests/ui/settings.rs:126-140` 锁死；若新设计要标记"暂未生效"必须同步该测试与 `docs/INDEX.md` §10 |
| C4 | `min_h_0` 三处不能漏 | Dialog content（`settings.rs:3154`）、内容列（`settings.rs:2805`）、根容器（`settings.rs:2862`）——漏一处就滚动不了 |
| C5 | 不得在 `tests/` 根新增文件 | `Cargo.toml:13` `autotests = false`，测试须由 `#[path]` 挂载 |

---

## 5. 验收标准

| # | 类型 | 标准 | 关联 |
|---|---|---|---|
| AC-1 | 自动 | `all_fields()` id 唯一、id 命名空间合法、默认配置下所有项 `is_default` 为真、`reset` 幂等 | R-1（现有 4 条测试保持全绿） |
| AC-2 | 自动 | **新增**：全部设置项 `desc` 长度 ≤ 20 字 | R-4 |
| AC-3 | 自动 | **新增**：分级白名单测试——「高级」集合与文档中登记的集合一致（防随意挪动） | R-1 |
| AC-4 | 自动 | **新增**：快捷键分区归属测试——16 项 keymap 项都在「快捷键」分类，不在「编辑器」 | R-3 |
| AC-5 | 自动 | `cargo build` / `clippy --all-targets` 0 warning、`fmt --check` 干净 | G4 |
| AC-6 | 人工 | 打开设置：分组是卡片、有留白、控件右对齐成一列，与 Obsidian 观感接近 | R-5 |
| AC-7 | 人工 | 「高级」默认收起；搜索高级项能自动展开并高亮定位 | R-1、R-7 |
| AC-8 | 人工 | 主视图看不到图标大小、字体行高、JSON 缩进、keep_alive、num_ctx 等低频项 | R-1 |
| AC-9 | 人工 | 快捷键独立分类；16 项不再重复「（重启后生效）」，分区顶部有一句说明 | R-3 |
| AC-10 | 人工 | 之前的自定义值仍然存在（`config.json` 未被改动），且非默认项仍有 ⟲ 可单项重置 | G4、R-1 |
| AC-11 | 人工 | AI 的 6 个面板视觉与元数据分组一致；密钥面板仍在对应模型行下方展开 | R-6 |
| AC-12 | 人工 | 窗口加宽后在小屏 / 大屏都不出现内容被裁切、滚动失效 | R-5、C4 |
| AC-13 | 自动 | 厂家归组：域名 → 厂家名（`vendor_of`），含端口 / 大写 / 空地址 | R-8 |
| AC-14 | 自动 | 能力小字的拼装（能力 + 上下文 + 工具结论 + 停用）与数字压缩 | R-9 |
| AC-15 | 人工 | AI 页：Ollama 一区、各厂家各一区、其他在最后；能力小字在模型名下一行；各行操作列对齐 | R-8 / R-9 / R-10 |
| AC-16 | 自动 | 提示词预览截断（短文本不动、超长加省略号） | R-11 |
| AC-17 | 人工 | 快捷动作：提示词在名称下一行；点「编辑」就地变成输入框，保存 / 取消都在这一行；底部不再出现编辑表单 | R-11 / R-12 |

人工项在实施后登记进 `docs/ACCEPTANCE.md`。

---

## 6. 决策记录（2026-10-28 用户已拍板，G1 通过）

| # | 问题 | 结论 |
|---|---|---|
| Q1 | 低频项处置 | **收进「高级」折叠**：一条都不删，搜索命中自动展开并跳转 |
| Q2 | §3.1 分级表 | **照单执行** |
| Q3 | `preview.debug_scroll_sync` | **从列表移除**（配置字段保留，旧 `config.json` 中的值不丢） |
| Q4 | 快捷键 | **独立成「快捷键」分类**；「（重启后生效）」改为分区顶部一句说明 |
| Q5 | 视觉改造力度 | **卡片分组 + 加宽到约 840 + 更大留白**；不换用 `gpui_component::setting::*` 框架 |
| Q6 | 精简掉的信息放哪 | **进 tooltip**（默认建议，未提出异议） |

G1 自检：需求文档 ✅ 存在；目标 / 非目标 / 边界 / 验收四节齐全 ✅；
R-1…R-7 编号唯一 ✅；现状均附 `文件:行号` 证据 ✅；待拍板清单已清空 ✅。
