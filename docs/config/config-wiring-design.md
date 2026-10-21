# 配置生效链路与接线设计（补充文档）

> 状态：**待评审**（2026-09-24）
> 定位：对 [`config-management-design.md`](./config-management-design.md) §7「配置项落地矩阵」
> 与 [`settings-ui-design.md`](./settings-ui-design.md) 的**审查结论与补充设计**
> 解决的核心问题：**配置页面里可见可调的设置项，与程序实际运行行为未关联（"死配置"）**
> 涉及文件：`src/core/config.rs`、`src/app.rs`、`src/ui/settings.rs`、`src/ui/editor.rs`、
> `src/ui/preview.rs`、`src/ui/file_tree.rs`、`src/core/highlight.rs`、`src/core/markdown.rs`、`src/main.rs`

---

## 1. 审查结论（先给结论）

对全部配置字段做逐字段消费点排查后的盘点：

| 指标 | 数量 | 说明 |
|---|---|---|
| 叶子配置项（顶层字段展开后） | **29** | 18 个顶层字段（`default_mode` 已于 2026-09-24 移除，`code_theme` 已于 2026-10-20 下线，`language` / `color_scheme` 已于 2026-10-21 移除，新增 `editor.soft_wrap` / `preview.debug_scroll_sync` / `preview.sync_mode`），其中 `tree_font` / `editor.font` / `preview.font` / `indent` 为结构体 |
| 已真正作用于软件功能 | **25** | 含 Spike 后接线的 `indent.*` 两项、内核升级后的 `show_line_numbers`、P1 补接线的 `font.weight` ×2、滚动同步重构新增的 `soft_wrap` / `debug_scroll_sync` / `sync_mode` |
| **死配置（存了不生效）** | **1** | 值能保存、能导出，但不影响任何行为：仅剩 `keymap`（`spell_check` 已于 2026-10-20 **移除**；`language`、`color_scheme` 已于 2026-10-21 移除） |
| 其中：设置界面已标注 ⚠ | 0 | `language`、`color_scheme` 已随字段一并删除，不再占位 |
| 其中：界面有入口但未标注 | 0 | P1 已为两个 `font.weight` 补上设置界面入口并接线 |
| 其中：**连设置项入口都没有** | 1 | `editor.keymap`（`main.rs:46-56` 快捷键全部硬编码） |
| 覆盖"配置改动 → UI 行为改变"的测试 | **0** | `src/ui/settings.rs` 无 `tests.rs`，配置单测全部停留在数据层 |

**结论**：配置页面当前"看起来能调 20+ 项，实际只有 17 项真正接线"，且这条链路**没有任何测试守护**，
重构时极易退化。本文档给出：接线总表、生效链路规范、逐项接线方案、验证方法与防退化机制。

> 修正口径（2026-09-24）：`link_open` 已确认可接线（P1）、`spell_check` 判定移除、
> `indent.*` 两项**已接线**、`show_line_numbers` **已接线**（随编辑内核升级落地）、
> **`default_mode` 已移除**。
> **2026-10-09 P1 已收尾**：两个 `font.weight` 已接线并补界面入口、`code_theme` 支持跟随明暗
> （默认 `auto`）、`sync_scroll` 编辑器侧已判断、`link_open` 已随图片预览接线。
> **2026-10-20**：`spell_check` **已实际移除**——`core/config.rs`（字段 + `Default`）与
> `settings.rs` 的 `all_fields()` 条目均已删除，旧 `config.json` 里的该字段被 serde 忽略。
> **2026-10-21**：`language`、`color_scheme` **已实际移除**（数据模型 + `Default` +
> `settings.rs` 条目一并删除，旧 `config.json` 里的字段被 serde 忽略）。
> 因此**当前仍未接线的只剩 1 项**：`keymap`。
> （`link_open` 已随"图片预览"接线，`image_display` 与新增的 `image_width_percent` 已接线。）

### 1.1 三条根因

| 根因 | 表现 | 证据 |
|---|---|---|
| **R1 消费方式散落，无集中登记** | 配置是"各面板 `render` 内现读"（pull），消费点分散在 4 个 UI 文件，没有登记表，改的人无法知道某个字段有没有被用 | `file_tree.rs:604`、`editor.rs:664`、`preview.rs:162` 各自 `crate::app::config(cx)` |
| **R2 框架能力缺口** | `Textarea` 编译期不暴露行号（行号仅在 `EditorState`）；`TextView` 未暴露图片显示开关；框架无拼写检查接口 | `editor.rs:663-672`；核实记录见 §1.2 |
| **R3 功能未实现但配置先行，且无退出机制** | `language` 本地化、`keymap` 覆盖先在模型里占位，长期无法接线也无人删除（`default_mode` 已于 2026-09-24 移除，作为该问题的首个处置） | `core/config.rs` |
| **R4 无防退化校验** | `unavailable` 标记靠人工维护，未接线的项不会被 CI 发现 | `settings.rs:219-700` 的 `all_fields()` 无测试 |

### 1.2 框架能力核实（追加：推翻部分原假设）

「若无框架能力就移除」的前提是**先证明真的没有能力**。已对 `gpui-*` 依赖源码做核实，结论**推翻了 `link_open` 的原判断**：

| 配置项 | 核实结果 | 证据 | 决策 |
|---|---|---|---|
| `link_open` | **框架有能力** | `gpui-base-0.6.4/src/text/text_view.rs:333` 提供 `TextView::on_link_click(handler)`；`gpui-component-0.6.4/src/text/compat.rs:136` 同名转发；**未挂 handler 时默认走 `cx.open_url(&url)`**（`text_view.rs:96`） | **保留并接线，从 P3 提前到 P1** |
| `spell_check` | **框架无能力** | 全量搜索 `gpui-*` 依赖：无 `NSSpellChecker` 绑定、无任何拼写 API；`input/editor/diagnostics.rs:280` 的 "Spelling mistake" 只是 LSP 诊断的**测试 fixture 字符串**，不是拼写能力 | ✅ **已移除**（2026-10-20，见本节末） |
| `image_display` | 有能力（部分受限） | 内联 HTML 默认启用且支持 `<img style="width: N%">`；但渲染层硬编码 `max_w(relative(1.))`，无法突破容器宽 | 已接线 `FitWidth` + `Hidden`；`Original` 移除 |

**`link_open` 接线方案（P1，约 15 行）**

- 接线点：`preview.rs:192` 的 `TextView::new(&doc)` 链式追加 `.on_link_click(...)`；
- `External` → **不挂 handler**，沿用框架默认 `cx.open_url`（系统浏览器打开）；
- `InApp` → 挂 handler：url 为 `file://` 且指向当前库内 `.md` → 走 `ui/tabs.rs::open_note` 打开标签；否则回退 `cx.open_url`；
- 生效时机 **L1**（下一次渲染即生效）。

> ⚠️ 注意默认行为反转：当前默认值为 `InApp`，而"不挂 handler"的框架默认行为是浏览器打开。
> 实现时两种取值都必须显式处理，否则 `External` 与 `InApp` 会表现一致（都走浏览器）。

**`spell_check` 移除方案（已于 2026-10-20 执行完毕）**

- 判定依据：框架无能力 + 自研需新增依赖（objc 调 `NSSpellChecker` 或 hunspell + 词典）+ 编辑器装饰接入，属于**独立特性**而非"配置项"，与本地优先轻量定位不符；
- 移除范围：`config.rs`（字段 + `Default`）、`settings.rs`（`all_fields()` 该项）、`config/tests.rs`（默认值断言）、本文档 §2.2/§4/§7.3 对应行；
- 兼容性：旧 `config.json` 中的 `spell_check` 被 serde 忽略，**无迁移成本**；日后若自研成功，加回字段的成本约 5 行（同样的 serde 兼容）。
- 保留知识资产：自研路径记入 §9 Q7，避免重复调研。

---

## 2. 当前配置页面的功能范围与字段清单

### 2.1 功能范围

设置界面 `src/ui/settings.rs`（`Cmd-,` 打开**主窗口内居中浮层**，元数据驱动；2026-10-21 由独立窗口改为浮层，见 `settings-ui-design.md` §4.1）：

| 能力 | 落地情况 |
|---|---|
| 三大分类 + 分组 | ✅ 基本 / 编辑器 / 预览 |
| 搜索 | ✅ 顶部搜索框（**当前为只读回显 + 清除按钮，未接入可编辑输入实体**，见 `settings-ui-design.md` §13 偏差说明） |
| 控件类型 | ✅ 开关、分段按钮、下拉（含系统字体）、数值输入 + 步进、单项重置 ⟲ |
| 导入 / 导出 / 恢复默认 | ✅ 落盘于 `~/Library/Application Support/xink/config.json` |
| 单项重置 | ✅ 值 ≠ 默认时出现 ⟲ |
| 依赖禁用 | ✅ 关闭"自动保存"→"保存延迟"置灰 |
| 未生效标注 | ⚠️ 仅 8 项标了 ⚠，**遗漏 3 项**（两个 `font.weight` 无入口、keymap 无入口） |

### 2.2 字段清单与接线现状

> 状态列：**✅ 已接线** / **❌ 死配置**；"消费点"为当前代码中真正读取该值的位置。

#### 基本（BasicConfig，`config.rs:126-133`）

| 配置项 | 类型 / 默认 | 设置项入口 | 消费点 | 状态 |
|---|---|---|---|---|
| `theme` | `ThemeMode` / `System` | 外观 → 主题模式 | `app.rs:142-148` `apply_theme` | ✅ |
| ~~`language`~~ | — | — | — | ✅ **已于 2026-10-21 移除**：文案未本地化，不留死配置 |
| `icon_size` | `u32` 12–48 / `16` | 目录树 → 图标大小 | `file_tree.rs:606,710`、`editor.rs:508` | ✅ |
| `tree_font.family` | `String` / `.SystemUIFont` | 目录树 → 字体 | `file_tree.rs:716` | ✅ |
| `tree_font.size` | `f32` 8–72 / `14` | 目录树 → 字号 | `file_tree.rs:717` | ✅ |
| `tree_font.line_height` | `f32` 0.8–3.0 / `1.5` | 目录树 → 行高 | `file_tree.rs:718` | ✅ |
| `tree_font.weight` | `FontWeight` / `Regular` | 目录树 → 字重 | `file_tree.rs:719` | ✅（**唯一生效的 weight**） |

#### 编辑器（EditorConfig，`config.rs:138-150`）

| 配置项 | 类型 / 默认 | 设置项入口 | 消费点 | 状态 |
|---|---|---|---|---|
| ~~`default_mode`~~ | — | — | — | **已移除**（2026-09-24）：仅源码模式可用，分屏由 DockArea 布局直接提供，所见即所得按 `REQUIREMENTS.md` 不在本期范围 |
| `auto_save` | `bool` / `true` | 编辑行为 → 自动保存 | `editor.rs:336,340` | ✅ |
| `auto_save_delay_ms` | `u64` 200–60000 / `300` | 编辑行为 → 保存延迟 | `editor.rs:335` | ✅ |
| `indent.use_tabs` | `bool` / `false` | 编辑行为 → 使用 Tab | 无 | ❌ |
| `indent.tab_size` | `u32` 1–16 / `4` | 编辑行为 → 缩进宽度 | 无 | ❌ |
| `show_line_numbers` | `bool` / `false` | 编辑行为 → 行号 | `editor.rs`（构造 + `render` 增量下发） | ✅ 已接线（2026-09-24，随内核升级为 `EditorState`） |
| ~~`spell_check`~~ | — | — | — | ✅ **已于 2026-10-20 移除**：数据模型与设置界面均无此项 |
| `soft_wrap` | `bool` / `true` | 编辑行为 → 软换行 | `editor.rs`（载入时下发 + `render` 增量下发） | ✅ 已接线（2026-10-20）：关闭后"源码行 ↔ 滚动位置"为 O(1) 精确换算，滚动同步更准 |
| `font.family` | `String` / 等宽 | 字体与排版 → 字体 | `editor.rs:664,669` | ✅ |
| `font.size` | `f32` / `14` | 字体与排版 → 字号 | `editor.rs:670` | ✅ |
| `font.line_height` | `f32` / `1.6` | 字体与排版 → 行高 | `editor.rs:671` | ✅ |
| `font.weight` | `FontWeight` / `Regular` | 编辑区字体 → 字重 | `editor.rs`（`Editor::new` 链） | ✅ 已接线（2026-10-09 P1）：渲染加 `.font_weight`，设置界面补"编辑器字重"入口 |
| ~~`color_scheme`~~ | — | — | — | ✅ **已于 2026-10-21 移除**：编辑区配色跟随全局主题与 `highlight.theme`，不再单列 |
| `keymap` | `BTreeMap<String,String>` / `{}` | **无入口** | 无 | ❌ `main.rs:46-56` 全硬编码 |

#### 预览（PreviewConfig，`config.rs:155-165`）

| 配置项 | 类型 / 默认 | 设置项入口 | 消费点 | 状态 |
|---|---|---|---|---|
| `font.family` | `String` / `.SystemUIFont` | 排版 → 字体 | `preview.rs:182` | ✅ |
| `font.size` | `f32` / `14` | 排版 → 字号 | `preview.rs:183` | ✅ |
| `font.line_height` | `f32` / `1.5` | 排版 → 行高 | `preview.rs:184` | ✅ |
| `font.weight` | `FontWeight` / `Regular` | 预览区字体 → 字重 | `preview.rs`（预览根容器） | ✅ 已接线（2026-10-09 P1）：渲染加 `.font_weight`，设置界面补"预览字重"入口 |
| ~~`code_theme`~~ | ~~`String` / `auto`~~ | ~~渲染 → 代码主题~~ | — | ⛔ **已下线**（2026-10-20）：语法配色统一到 `highlight.theme`（见 `editor/unified-highlight-config-design.md`）；旧值经 `preview.legacy_code_theme` 迁移一次后丢弃 |
| `image_display` | `ImageDisplay` / `FitWidth` | 渲染 → 图片显示 | `markdown.rs`（预处理改写） | ✅ 已接线（选项为"适应宽度 / 不显示"；原"原始尺寸"因框架限制移除） |
| `image_width_percent` | `u32` 20–100 / `100` | 渲染 → 图片宽度 | `markdown.rs`（写入 `<img style>`） | ✅ 已接线 |
| `link_open` | `LinkOpen` / `InApp` | 渲染 → 链接打开 | `preview.rs`（`on_link_click`） | ✅ 已接线（含点击图片打开预览） |
| `sync_scroll` | `bool` / `true` | 同步 → 同步滚动 | `preview.rs`、`editor.rs`（`tick_scroll_sync` 开头判断） | ✅ 已接线（2026-10-09 P1）：编辑器侧也判断开关，关闭后不再推送滚动比例 |
| `max_width` | `u32` 0–4096 / `0` | 排版 → 正文最大宽度 | `preview.rs:187-188` | ✅ |
| `debug_scroll_sync` | `bool` / `false` | 同步 → 调试信息 | `preview.rs`（面板标题显示两端行号与偏差） | ✅ 已接线（2026-10-20）：排查滚动同步用，默认关闭 |
| `sync_mode` | `ScrollSyncMode` / `ratio` | 同步 → 同步方式 | `preview.rs`、`editor.rs`（`tick_scroll_sync` / `current_anchor` / `apply_*` 分支） | ✅ 已接线（2026-10-21）：`ratio` 比例跟随（默认，连续平滑）/ `anchor` 块锚点（位置更准、块边界会跳）/ `on_stop` 停下 250ms 后对齐一次（绝对不抖） |

---

## 3. 配置生效链路（生成 → 存储 → 加载 → 生效）

### 3.1 现状链路

```
① 生成默认值
   config.rs:227-266  Default impl（code_theme 取 AUTO_CODE_THEME，即"跟随明暗"）

② 用户改动（唯一写入口）
   settings.rs 控件回调 → app.rs:122 update_config(cx, f)
       ├─ f(&mut state.config)        改内存（AppState.config，app.rs:60，impl Global）
       ├─ state.config.sanitize()     越界夹取（config.rs:296）
       ├─ state.config.save()         立即落盘（config.rs:337）
       ├─ apply_theme(None, cx)       主题跟随（app.rs:139-150）
       └─ cx.refresh_windows()        全窗口重绘 → 立即生效

③ 存储
   ~/Library/Application Support/xink/config.json
   （state/workspace.rs:32-34 config_path；pretty JSON）

④ 启动加载
   app.rs:270  AppConfig::load()（config.rs:327，失败回落 default + sanitize）
   app.rs:274  apply_theme(Some(window), cx)   ← 唯一被显式 apply 的配置

⑤ 消费（两类）
   A. 渲染时 pull：各面板 render 内 crate::app::config(cx)（file_tree.rs:604 /
      editor.rs:664 / preview.rs:162 / settings.rs:938）→ 下一帧见效
   B. 运行时读取：editor.rs:335 每次 schedule_save 现读 delay
```

**关键事实**：除 `theme` 外，所有配置都是 **"渲染时拉取"**，没有"启动时推送"，也没有"配置变更事件 → 面板订阅"的链路。
`cx.refresh_windows()`（`app.rs:134`）是改动生效的**唯一驱动**。

### 3.2 链路缺口（本次要补）

| 缺口 | 影响 | 补法 |
|---|---|---|
| G1 消费点无登记 | 无法回答"哪些配置接线了" | 消费点注释规范 + 校验脚本（§7） |
| G2 消费逻辑写在 UI 层 | 无法单测（UI 层依赖 GPUI） | 抽出 `core/config/resolve.rs` 配置解释器（§3.3） |
| G3 生效时机无表达 | 用户不知道改动何时生效、作不生效 | `SettingField` 增加 `effect` 字段（§3.4） |
| G4 死配置无退出机制 | 长期挂着的"假设置" | 处置三选一：接线 / 降级 / 移除（§5） |

### 3.3 改造一：引入配置解释器 `core/config/resolve.rs`

把"配置 → 渲染参数"的转换从 UI 层下沉到 `core`（纯 Rust，可单测），UI 只消费结果：

```rust
//! 配置解释器：把 AppConfig 转成各面板可直接使用的渲染参数。
//! 放在 core 层，因此可用单元测试守护"配置改动 → 参数变化"。

pub struct FontSpec { pub family: String, pub size: f32, pub line_height_px: f32, pub weight: f32 }

pub fn tree_font(cfg: &AppConfig) -> FontSpec;
pub fn editor_font(cfg: &AppConfig) -> FontSpec;
pub fn preview_font(cfg: &AppConfig) -> FontSpec;
/// "auto"（默认）按当前明暗选主题；否则按名解析，未知名由 highlight.rs 兜底
pub fn code_theme<'a>(cfg: &'a AppConfig, is_dark: bool) -> &'a str;
pub fn save_delay(cfg: &AppConfig) -> Duration;
pub fn max_width_px(cfg: &AppConfig) -> Option<f32>;      // 0 → None
pub fn indent_string(cfg: &AppConfig) -> String;           // use_tabs ? "\t" : " "*tab_size
pub fn image_policy(cfg: &AppConfig) -> ImagePolicy;       // FitWidth | Original | Hidden
pub fn link_policy(cfg: &AppConfig) -> LinkPolicy;         // InApp | External
```

收益：接线逻辑可测（§7.1 用例）、UI 层只剩一行调用、新增配置只需在解释器加一个函数。

### 3.4 改造二：设置项元数据表达"生效时机"

`settings.rs:75` 的 `unavailable: bool` 信息量不足，升级为枚举：

```rust
pub enum Effect {
    /// 下一帧即见（渲染时读取）
    Immediate,
    /// 影响下一次交互/触发，括号为说明
    NextInteraction(&'static str),
    /// 需重启应用
    Restart(&'static str),
    /// 尚未接线（附原因，UI 显示 ⚠）
    NotWired(&'static str),
}
```

UI 呈现规则：

| `Effect` | 徽标 | 控件 |
|---|---|---|
| `Immediate` | 无 | 可用 |
| `NextInteraction(s)` | `⟳ 下次生效 · {s}` | 可用 |
| `Restart(s)` | `⟳ 重启后生效 · {s}` | 可用 |
| `NotWired(s)` | `⚠ 暂未生效 · {s}` | 可用（值仍可保存/导出） |

---

## 4. 每项配置 ↔ 功能点 ↔ 应用逻辑（接线总表）

> **F** = 功能点，**L** = 应用逻辑（接线方案），**T** = 生效时机分级（见 §5）

| # | 配置项 | 功能点 F | 应用逻辑 L（接线方案） | T | 阶段 |
|---|---|---|---|---|---|
| 1 | `theme` | 明暗外观 | 已接线：`apply_theme` 按 `System/Light/Dark` 分派 | L1 | — |
| 2 | `language` | 界面文案语言 | 新建 `core/i18n.rs`：`t(key, lang)` 查表；UI 文案全部改用 `t()`；`update_config` 后 `refresh_windows()` 即切 | L1 | P2 |
| 3 | `icon_size` | 目录树/工具栏图标边长 | 已接线 | L1 | — |
| 4 | `tree_font.*` | 目录树字体 | 已接线（含 weight） | L1 | — |
| 5 | ~~`default_mode`~~ | ~~打开笔记时的编辑形态~~ | **已移除**（2026-09-24）：仅源码模式可用，其余两种模式无实现路径；分屏由 DockArea 布局直接提供，不需要配置项 | — | 已移除 | `Source`=现状；`Split`=编辑面板内左右分屏（DockArea 已具备分栏能力）；**`Wysiwyg` 短期无实现路径 → 从枚举移除**，保留 `Source \| Split` | L1 | P4 |
| 6 | `auto_save` | 停止输入后自动写盘 | 已接线（`editor.rs:340` 只挡写盘，不挡预览刷新） | L2 | — |
| 7 | `auto_save_delay_ms` | 防抖延时 | 已接线；注意延时在 `schedule_save` 调用瞬间快照进异步任务 | L2 | — |
| 8 | `indent.use_tabs` / `tab_size` | Tab 键插入的字符 | **Spike 已验证并落地**：`set_tab_size` 属 `impl<M: MultiLineMode>`（`indent.rs:492`），`TextareaState` 可直接设 `TabSize { tab_size, hard_tabs }`，**无需更换编辑内核**。已在 `spike/editor-core` 实现（create 首次下发 + render 增量下发） | L1 | ✅ 已完成（待合入） |
| 9 | `show_line_numbers` | 编辑区左侧行号 | **已完成**：编辑内核升级为 `EditorState`（行号 API 仅它提供，`state.rs:9330`）。构造期按配置设置，`render` 中检测变更增量下发 → 开关即时生效；同时显式关闭 `auto_close` / `smart_indent` / `folding` / `indent_guides`，避免干扰 Markdown 写作 | L1 | ✅ 已完成（待人工验证） |
| 10 | `spell_check` | 拼写检查 | **已核实框架无能力 → 移除**（§1.2）。自研需新依赖 + 词典 + 编辑器装饰，属独立特性 | — | ✅ 已移除（2026-10-20） |
| 11 | `editor.font.*` | 编辑区字体 | **已完成**：family/size/line_height/weight 全部接线；`editor.rs` 的 `Editor::new` 链加 `.font_weight(FontWeight(font.weight.value()))`，设置界面补"编辑器字重"入口 | L1 | ✅ 已完成（待人工验证） |
| 12 | `color_scheme` | 编辑区配色独立于主题 | `Auto`=跟随全局主题；`Light/Dark`=在编辑区容器显式设前景/背景色（不依赖全局 Theme） | L1 | P3 |
| 13 | `keymap` | 快捷键覆盖 | 新增 `core/keymap.rs`：`default_bindings() + override(cfg.editor.keymap)`；`main.rs:46-56` 改为 `cx.bind_keys(resolve_bindings(&cfg))`；设置界面补键值对列表入口 | **L3** | P2 |
| 14 | `preview.font.*` | 预览区字体 | **已完成**：四项全部接线；`preview.rs` 预览根容器加 `.font_weight(...)`，设置界面补"预览字重"入口 | L1 | ✅ 已完成（待人工验证） |
| 15 | `code_theme` | 代码块高亮配色 | **已完成（含跟随明暗）**：默认值改为 `AUTO_CODE_THEME`（`auto`），渲染时按 `app::is_dark()` 解析为 `default_theme(is_dark)`；设置界面选项首位为"跟随主题"。旧配置里的具体主题名不受影响（原样使用） | L1 | ✅ 已完成（待人工验证） |
| 16 | `image_display` | 预览区图片渲染方式 | **已完成**：预处理阶段按配置改写——`Hidden` 去掉 `!` 退化为文本链接；`FitWidth` 渲染为 `<img style="width: N%">`（N 取 `image_width_percent`）。**`Original`（原始尺寸）已移除选项**：渲染层硬编码 `max_w(relative(1.))`，Markdown 侧无法突破 | L1 | ✅ 已完成 |
| 17 | `link_open` | 点击链接的行为 | **已完成**：`preview.rs` 挂 `on_link_click`。`InApp` → 库内 `.md` 走 `open_note`，非笔记回退系统浏览器；`External` → `cx.open_url`。**点击图片也走此回调并打开预览窗口** | L1 | ✅ 已完成 |
| 18 | `sync_scroll` | 编辑/预览滚动同步 | 已接线（预览侧判断）；**补编辑器侧判断**：`editor.rs:280-283` 发事件前先判 `sync_scroll`，关闭时不再发无用事件 | L2 | P1 |
| 19 | `max_width` | 预览正文最大宽度 | 已接线 | L1 | — |

### 4.1 附带修复（非配置项，但影响"配置可信度"）

| 问题 | 位置 | 处理 |
|---|---|---|
| 退出应用时未 flush 未保存笔记 | `app.rs:318-331` `on_app_quit` 只存布局与标签 | 退出前若有未保存改动：自动保存开启 → 直接 flush；关闭 → 弹确认。纳入 P2 |
| 配置克隆开销 | `preview.rs:68/95` 每帧在滚动 tick 内再克隆一次全量配置 | P1 顺带：tick 内复用本帧已取的配置副本 |

---

## 5. 配置变更后的生效时机与作用范围

| 级别 | 含义 | 配置项 | 作用范围 | 是否需重启 |
|---|---|---|---|---|
| **L1 立即** | `update_config` → `refresh_windows()` → 下一帧全部窗口重绘；面板 `render` 内现读配置 | `theme`、`icon_size`、`tree_font.*`、`editor.font.*`、`preview.font.*`、`code_theme`、`max_width`、`language`（改造后）、`show_line_numbers`、`color_scheme`、`image_display` | 全部已打开窗口 + 之后新建的窗口 | 否 |
| **L2 下次交互** | 值在触发瞬间被读取/快照，改动不影响已在飞行中的任务 | `auto_save`（下一次防抖触发是否写盘）、`auto_save_delay_ms`（下一次输入触发的防抖周期）、`indent.*`（下一次按 Tab）、`sync_scroll`（下一次滚动事件） | 当前会话，下一轮触发起 | 否 |
| **L3 重启** | 绑定/注册在启动时一次性完成 | `keymap`（`main.rs` 的 `bind_keys` 在启动阶段执行） | 全应用 | **是** |
| **L4 不生效** | 无消费点（当前死配置，见 §2.2） | 12 项 | — | — |

> 写入语义（现状，保持不变）：**无"应用/取消"按钮**，任何改动立即改内存 + 立即落盘 + 立即重绘，
> 关闭设置窗口不会丢失改动（`settings.rs:108/725/753/778/786/894/1144` 全部直连 `update_config`）。

---

## 6. 实施计划

| 阶段 | 内容 | 涉及文件 | 验收 |
|---|---|---|---|
| **P0 防退化**（先做） | ① 消费点注释规范 `// config: <field.path>`<br>② `scripts/check-config-wiring.sh` 静态校验<br>③ `settings.rs` 补 `tests.rs`，断言 `all_fields()` 每项都有 `effect` 且 `NotWired` 项与本文档清单一致 | 全仓 + `scripts/` | 脚本可跑通；删除任一消费点注释 → 脚本失败 |
| **P1 低成本接线** | `editor.font.weight`、`preview.font.weight`、`code_theme` 跟随明暗、`sync_scroll` 编辑器侧判断、**`link_open`（`on_link_click`）**、配置克隆开销 | `editor.rs`、`preview.rs`、`settings.rs`、`core/config/resolve.rs`（新建） | §7.3 验收清单对应项通过；新增单测 ≥ 9 |
| **P2 结构性接线** | `language` 文案表（`core/i18n.rs`）、`keymap` 覆盖（`core/keymap.rs` + `main.rs`）、退出前 flush | `core/i18n.rs`（新建）、`core/keymap.rs`（新建）、`main.rs`、`app.rs` | 切英文生效；改 `cmd-s`→`cmd-shift-s` 重启后生效 |
| **P3 能力补齐/清理** | 缩进 Tab（已接线）、行号（已接线）、`color_scheme`、**移除 `spell_check`** | `editor.rs`、`core/markdown.rs`、`config.rs`、`settings.rs`、`config/tests.rs` | 每项要么生效，要么从模型与界面移除（不留死配置） |
| **P4 形态类** | ~~`default_mode`~~ **已取消**（2026-09-24）：分屏由 DockArea 布局直接提供，所见即所得按 `REQUIREMENTS.md` 不在本期范围 | — | 无需实施 |

**执行原则**：每个阶段结束后，§4 总表中该阶段的项必须从 `L4/NotWired` 变为某个明确级别；
做不到接线的项，在同一阶段内**从数据模型与设置界面移除**，不允许以"暂未生效"长期挂起。

---

## 7. 配置生效的验证方法

### 7.1 第一层：自动化单测（数据层 + 解释器层）

现有：`src/core/config/tests.rs` 13 个用例（默认值 / 兼容 / 往返 / 校正 / 导入导出）。

P1 起在 `src/core/config/resolve.rs`（或同名 tests）新增：

| 用例 | 断言 |
|---|---|
| `tree_font_maps_all_four_fields` | weight 变化 → `FontSpec.weight` 变化 |
| `editor_font_weight_is_applied` | 覆盖当前缺失的 weight |
| `preview_font_weight_is_applied` | 同上 |
| `code_theme_auto_follows_dark_mode` | `auto` + 明暗 → 分别为 `base16-ocean.dark` / `.light` |
| `code_theme_explicit_name_overrides_auto` | 显式名不受明暗影响 |
| `max_width_zero_means_unlimited` | `0 → None`，`800 → Some(800)`，`9999 → Some(4096)`（先 sanitize） |
| `save_delay_uses_configured_value` | `1500ms → Duration::from_millis(1500)` |
| `indent_string_honors_use_tabs` | `use_tabs=true → "\t"`；`false + 4 → "    "` |
| `image_policy_hidden_rewrites_markdown` | `Hidden` 时 `![a](u)` → `[a](u)`（配合 `core/markdown.rs`） |
| `resolve_is_pure` | 同一配置两次调用结果相等（无隐藏状态） |

### 7.2 第二层：静态接线校验（防退化，P0 交付）

约定：**每个读取配置字段的代码点必须写一行注释**，格式为 `// config: <字段路径>`，例如：

```rust
// config: editor.font.weight
.font_weight(FontWeight(font.weight.value()))
```

`scripts/check-config-wiring.sh`：

```bash
#!/usr/bin/env bash
# 校验：config.rs 中每个叶子字段都至少有一处 "config: <字段>" 消费点注释；
#       反之，每个注释里的字段都必须在模型中存在。缺一即失败（退出码 1）。
set -euo pipefail
cd "$(dirname "$0")/.."

# 1. 从 config.rs 提取叶子字段路径（含 tree_font.* / editor.font.* / preview.font.* / indent.*）
FIELDS=$(rg -o '^\s{4}pub [a-z_]+\.\.\.|^\s{4}pub [a-z_]+:' src/core/config.rs | ...)
# 2. 收集所有消费点注释
WIRED=$(rg -o '// config: [a-z_.]+' src --no-filename | sed 's|// config: ||' | sort -u)

missing=0
for f in $FIELDS; do
  grep -qx "$f" <<<"$WIRED" || { echo "未接线: $f"; missing=1; }
done
exit $missing
```

等价的一次性人工核查命令（不写脚本也能用）：

```bash
rg -o '// config: [a-z_.]+' src --no-filename | sed 's|// config: ||' | sort -u
```

与 §2.2 清单比对，缺失项即为死配置。

### 7.3 第三层：GUI 验收清单（每项给出操作 → 观察点 → 预期结果）

> 前置：`cargo run` 启动 → `Cmd-,` 打开设置 → 同一窗口内对照主窗口观察。

| # | 配置项 | 操作 | 观察点 | 预期结果 | 当前是否通过 |
|---|---|---|---|---|---|
| V1 | `theme` | 切换 浅色/深色/跟随系统 | 整个界面配色 | 立即切换，无需重启 | ✅ |
| V2 | `icon_size` | 16 → 32 | 目录树图标、编辑器工具栏图标 | 图标变大且不裁切 | ✅ |
| V3 | `tree_font.size` | 14 → 20 | 文件树行高与字号 | 立即变大 | ✅ |
| V4 | `tree_font.weight` | Regular → Bold | 文件名字重 | 立即加粗（目录名本就加粗，看文件名） | ✅ |
| V5 | `editor.font.size` | 14 → 18 | 编辑区文字 | 立即变大 | ✅ |
| V6 | `editor.font.weight` | — | 编辑区文字 | **当前无入口、改了无变化**；P1 后应加粗 | ❌ → P1 |
| V7 | `preview.font.weight` | — | 预览区正文 | 同上 | ❌ → P1 |
| V8 | `auto_save` | 关闭后输入文字 | 标题栏/保存标记、文件内容 | 不再自动写盘，但预览仍实时刷新 | ✅ |
| V9 | `auto_save_delay_ms` | 300 → 3000 | 停手后写盘时刻 | 约 3 秒后才落盘（下一次输入起生效） | ✅ |
| V10 | `code_theme` | 换一个主题名 | 预览区代码块配色 | 立即变化（换名后不命中旧缓存） | ✅ |
| V11 | `code_theme` | 主题切浅色，值改 `auto` | 代码块底色 | **当前浅色下仍是深色配色**；P1 后变浅色 | ❌ → P1 |
| V12 | `max_width` | 0 → 800 | 预览正文宽度与居中 | 正文限宽并居中 | ✅ |
| V13 | `sync_scroll` | 关闭后滚动编辑区 | 预览区是否跟随 | 不再跟随 | ✅ |
| V14 | `language` | 切 English | 界面文案 | **当前无变化**；P2 后切换 | ❌ → P2 |
| V15 | `keymap` | — | — | **无入口**；P2 后可改键位，重启生效 | ❌ → P2 |
| V16 | `indent.tab_size` / `use_tabs` | 4 → 8 后按 Tab；再勾选"使用 Tab 字符" | 插入字符 | 分别插入 8 空格与 `\t`，**改完即生效**（无需重开标签） | ✅ 已接线，待人工验证 |
| V17 | `show_line_numbers` | 打开开关 | 编辑区左侧的行号 | 开关打开后立即显示行号；关闭后隐藏（无需重开标签） | ✅ 已接线，待人工验证 |
| V18 | `image_display` / `image_width_percent` | ① 设为"不显示图片" ② 宽度改为 60% | 预览区图片 | ① 图片变成文本链接 ② 图片宽度变为预览区的 60%；**改完即生效**（配置变化时重算预览源文） | ✅ 已接线，待人工验证 |
| V19 | `link_open` | ① 设为"浏览器打开"，点外链<br>② 设为"应用内打开"，点库内 `.md` 链接 | ① 系统浏览器打开<br>② 在当前窗口打开为标签 | **当前两者都走浏览器**；P1 后按配置分流 | ❌ → **P1** |
| V20 | ~~`default_mode`~~ | — | — | **配置项已移除**：设置界面不再显示该项，导出 JSON 不含该字段 | ✅ 已移除 |
| V21 | `color_scheme` | 任意改动 | 编辑区配色 | 无变化；P3 接线或移除 | ❌ → P3 |
| V24 | `spell_check` | — | 设置界面是否已移除该项 | P3 后界面与模型均无此项，导出 JSON 不含该字段 | ✅ 已移除（2026-10-20） |
| V22 | 持久化 | 改任意项 → 退出 → 重启 | 设置值、实际效果 | 值保留且效果一致（配置确实被消费而非仅显示） | 需逐项复测 |
| V23 | 导入导出 | 导出 → 改乱 → 导入 | 全部设置项 | 完全还原，且效果同步还原 | ✅（数据层已测） |

**验收判定**：每一项都必须能从"改值"走到"观察到行为变化"。
做不到 V 类验证的配置项，按 §6 原则**移除**，不允许留在设置界面。

---

## 8. 防退化机制（P0 交付物）

新增或变更一个配置项时，必须同时提交"五件套"：

| # | 必交物 | 检查方式 |
|---|---|---|
| 1 | 数据模型字段（`core/config.rs`：字段 + 默认值 + `sanitize` 区间） | 单测 |
| 2 | **消费点**（真实读取并影响行为的代码 + `// config: <path>` 注释） | `check-config-wiring.sh` |
| 3 | 设置项（`settings.rs::all_fields()` 一行，含 `effect` 与说明） | `settings/tests.rs` 断言表完整 |
| 4 | 解释器单测（若走 `resolve.rs`） | `cargo test` |
| 5 | 本文档 §4 总表一行 + §7.3 验收清单一行 | 评审 |

CI/本地命令：`cargo test` + `bash scripts/check-config-wiring.sh`。
**缺 2 或未通过验收清单的项，一律标 `NotWired` 且不得声称"支持该项设置"。**

---

## 9. 风险与待决问题

| # | 风险 / 待决 | 影响 | 建议 |
|---|---|---|---|
| Q1 | ~~`Textarea` 是否吞掉 Tab 键、能否用于自研 gutter~~ | ~~决定 `indent.*` 与 `show_line_numbers` 能否落地~~ | **已关闭（2026-09-24）**：Tab 键由框架全局绑定 `tab → IndentInline`（`state.rs:182`，`"Input"` 上下文），无需自行处理；缩进已接线；行号随内核升级为 `EditorState` 已接线。自研 gutter 方案作废（soft wrap 下行号会错位） |
| Q2 | ~~`TextView` 是否提供链接点击回调~~ | ~~决定 `link_open` 生死~~ | **已关闭（2026-09-24）**：回调存在（`text_view.rs:333`），`link_open` 改为 P1 接线，详见 §1.2 |
| Q3 | 切到 `Editor`（代码编辑模式）换取行号能力 | 会牵动编辑体验、保存与光标逻辑 | **推荐"先做 1 天原型再决策"**，不直接全量替换。理由与 7 条验收清单见 `editor-core-migration-eval.md` §4/§6；不与 P1/P2 混做 |
| Q4 | `keymap` 只能重启生效 | 用户体验略差 | 接受 L3，UI 明确标注"重启后生效" |
| Q5 | 移除字段的兼容影响 | 老 `config.json` 含被删字段 | serde 忽略未知字段，无风险。**已验证**：移除 `default_mode` 后，含该字段的旧 `config.json` 仍可正常加载（多余字段被忽略），`cargo test` 85 passed |
| Q6 | 全窗口重绘（`refresh_windows`）粒度粗 | 改一个数字也全量重绘 | 本期不改（结构体小，开销可忽略）；若后续配置变大再考虑事件订阅 |
| Q7 | 拼写检查若日后要做，走哪条路 | 决定 `spell_check` 能否回归 | 已调研（勿重复）：框架无 API。可选 ①`objc` 调 macOS `NSSpellChecker`（无词典打包，需新增依赖）；②`hunspell` crate + 自带词典（跨平台，体积增大）。装饰层 GPUI 支持波浪下划线（`gpui-pre-0.3.5/src/style.rs:835`）。**须先做 POC 通过再决定加回字段** |
| Q8 | ~~`image_display` 的 `Original` 可否绕过~~ | ~~决定该项是否只能做到 `Hidden`~~ | **已关闭（2026-09-24）**：三处渲染都硬编码 `.max_w(relative(1.))`（`node.rs:1813`、`inline_flow.rs:170/1087`），Markdown 侧无论写 `<img width>` 还是 `style` 都会被钳到容器宽，**无法绕过**。该选项已从枚举与设置界面移除，并保留 `Legacy` 变体兼容旧配置 |

---

## 10. 与既有文档的关系

| 文档 | 本补充文档的作用 |
|---|---|
| `config-management-design.md` | **修正**其 §7.2「仅持久化（待接入）」清单：补充遗漏的 `editor.font.weight`、`preview.font.weight`、`keymap`；并把"待接入"升级为"接线/降级/移除三选一 + 有期限" |
| `settings-ui-design.md` | **细化**其 §5「暂未生效」状态：由单一 ⚠ 徽标拆为 `Immediate / NextInteraction / Restart / NotWired` 四类；补充其 §13 未提及的 keymap 与 weight 缺口 |
| `REQUIREMENTS.md` / `DESIGN.md` | 不改；本补充文档只约束"配置项必须真正生效"这一工程约束 |
