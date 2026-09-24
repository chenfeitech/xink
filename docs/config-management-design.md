# 配置管理 — 技术设计文档

> 状态：已实现（2026-09-21）
> 配套文档：[`config-management-requirements.md`](./config-management-requirements.md)
> 涉及文件：`src/core/config.rs`、`src/app.rs`、`src/ui/settings.rs`、`src/state/workspace.rs`、`src/core/highlight.rs`

---

## 1. 设计目标与约束

| 目标 | 约束 |
|---|---|
| 改动立即生效 | GPUI 是即时模式（IMGUI）框架：界面在 `render` 中重建，只要让窗口重绘即可看到新值 |
| 数据模型可独立测试 | 本仓是**纯 bin crate**（无 `[lib]`），`tests/` 集成测试无法引用内部类型，只能靠模块内单测 |
| 模型不依赖 UI 框架 | `core/` 层不得引用 `gpui_kit::component::*`（见 `core/mod.rs` 的层规约） |
| 新增配置成本低 | 新增项只应改动「数据模型 + 设置界面」两处 |
| 旧配置向前兼容 | 用户已存在的配置文件不能因为升级而失效 |

---

## 2. 总体结构

```
┌──────────────── 数据层（不依赖 GPUI，可单测） ────────────────┐
│ src/core/config.rs                                            │
│   AppConfig { basic, editor, preview }   ← serde + Default     │
│   sanitize()  load()  save()  export_to()  import_from()       │
└──────────────────────────┬────────────────────────────────────┘
                           │ config_path()
┌──────────────────────────┴────────────────────────────────────┐
│ src/state/workspace.rs   ~/Library/Application Support/xink/   │
│                          config.json                           │
└──────────────────────────┬────────────────────────────────────┘
                           │ 启动时 load()
┌──────────────────────────┴────────────────────────────────────┐
│ 运行时全局（src/app.rs）                                        │
│   AppState.config : AppConfig          （impl Global）          │
│   config(cx)           读取副本                                 │
│   update_config(cx, f) 唯一写入口                               │
│   apply_theme()        主题切换                                 │
└──────────────────────────┬────────────────────────────────────┘
                           │ 渲染时读取 / 改动后 refresh_windows()
┌──────────────────────────┴────────────────────────────────────┐
│ 消费方                                                          │
│   ui/file_tree.rs   目录树字体/字重/图标                         │
│   ui/editor.rs      编辑区字体、自动保存、工具栏图标              │
│   ui/preview.rs     代码主题、正文字体、最大宽度、同步滚动        │
│   ui/settings.rs    设置界面（写入口）                           │
└────────────────────────────────────────────────────────────────┘
```

**分层原则**：配置的数据与读写规则全在 `core/config.rs`（纯 Rust + serde），`app.rs` 只负责「持有全局实例 + 触发重绘」，各 UI 模块只负责「渲染时读取 + 应用」。因此配置模块可以脱离 GPUI 单测。

---

## 3. 数据模型

```rust
pub struct AppConfig {          // #[serde(default)]
    pub basic: BasicConfig,
    pub editor: EditorConfig,
    pub preview: PreviewConfig,
}

pub struct BasicConfig {        // 基本
    pub theme: ThemeMode,       // System | Light | Dark
    pub language: Language,     // Zh | En
    pub icon_size: u32,         // 12–48，默认 16
    pub tree_font: FontConfig,
}

pub struct EditorConfig {       // 编辑器
    pub default_mode: EditorMode,      // Source | Split | Wysiwyg
    pub auto_save: bool,               // 默认 true
    pub auto_save_delay_ms: u64,       // 200–60000，默认 300
    pub indent: IndentConfig,          // use_tabs: bool, tab_size: 1–16
    pub show_line_numbers: bool,
    pub spell_check: bool,
    pub font: FontConfig,              // 默认等宽
    pub color_scheme: ColorScheme,     // Auto | Light | Dark
    pub keymap: BTreeMap<String, String>,
}

pub struct PreviewConfig {      // 预览
    pub font: FontConfig,
    pub code_theme: String,            // syntect 主题名
    pub image_display: ImageDisplay,   // FitWidth | Original | Hidden
    pub link_open: LinkOpen,           // InApp | External
    pub sync_scroll: bool,
    pub max_width: u32,                // 0 = 不限
}

pub struct FontConfig {         // 目录树 / 编辑器 / 预览各持一份
    pub family: String,
    pub size: f32,              // 8–72
    pub line_height: f32,       // 0.8–3.0
    pub weight: FontWeight,     // Regular | Medium | Semibold | Bold
}
```

### 3.1 兼容策略

- 每个结构体都带 `#[serde(default)]`：**缺字段按该结构体的 `Default` 补齐**，因此新增配置项不会让老配置文件失效。
- 未知字段由 serde 忽略，删除过的旧字段不会导致解析失败。
- 枚举序列化用 `rename_all = "lowercase"`（单词）或 `"snake_case"`（多词，如 `fit_width`），保证 JSON 可读、可手改。
- 默认值用 `#[derive(Default)]` + `#[default]` 变体表达；只有 `IndentConfig`（默认 4 空格而非 0）等少数结构体保留手写 `Default`。

### 3.2 取值校正（`sanitize`）

载入与导入后统一校正，**只夹取、不拒绝**，保证界面永远拿到合法值：

| 配置项 | 合法区间 | 非法值处理 |
|---|---|---|
| 字号（三处） | 8 – 72 px | 越界夹取；`NaN` → 8 |
| 行高（三处） | 0.8 – 3.0 | 越界夹取 |
| 图标大小 | 12 – 48 px | 越界夹取 |
| 自动保存延迟 | 200 – 60000 ms | 越界夹取 |
| 缩进宽度 | 1 – 16 | 越界夹取 |
| 正文最大宽度 | 0 – 4096 px | 越界夹取 |
| 字体名 | 非空 | 空白串 → 系统默认字体 |
| 代码主题名 | 非空 | 空串 → 默认深色主题 |

---

## 4. 存储设计

| 项 | 值 |
|---|---|
| 路径 | `~/Library/Application Support/xink/config.json`（`state::workspace::config_path()`） |
| 格式 | JSON，`serde_json::to_string_pretty`，便于手工查看与编辑 |
| 写入时机 | 每次 `update_config`；写入失败仅打印告警，不影响主流程 |
| 读取时机 | 启动时一次（`AppConfig::load()`） |
| 容错 | 文件缺失 / JSON 损坏 → `unwrap_or_default()`，回落默认配置后照常启动 |

> 与 `DESIGN.md` 2.4 一致：`config.json` 存放偏好设置，`workspace.json` 放工作区状态，索引类数据可随时重建。

---

## 5. 生效机制

### 5.1 唯一写入口

```rust
pub fn update_config(cx: &mut App, f: impl FnOnce(&mut AppConfig)) {
    cx.update_global::<AppState, _>(|state, cx| {
        f(&mut state.config);      // 1. 应用改动
        state.config.sanitize();   // 2. 校正越界值
        state.config.save();       // 3. 落盘
        apply_theme(None, cx);     // 4. 主题跟随
        cx.refresh_windows();      // 5. 重绘所有窗口 → 立即生效
    });
}
```

所有界面都在 `render` 中通过 `crate::app::config(cx)` 读取当前配置；`refresh_windows()` 让它们重新渲染，于是**任何改动都是即时的，不需要重启**。

把写入收敛到单一入口的好处：落盘、校正、重绘这三件事不会被某个调用方漏掉。

### 5.2 主题切换

`apply_theme(window, cx)` 按 `basic.theme` 分派：

- `System` → `Theme::sync_system_appearance(window, cx)`（跟随系统外观）
- `Light` / `Dark` → 直接改写全局 `Theme` 的 `colors`

启动时在 `Workspace::new` 中调用一次（`AppConfig::load()` 之后），保证首帧就是用户选择的主题。

### 5.3 动作注册的阶段约束（已踩过的坑）

`Cmd-,` 由 `OpenSettings` 动作触发。最初在 `Workspace::new` 中用 `cx.on_action(...)` 注册，启动即崩溃：

```
panicked at gpui-pre-0.3.5/src/window.rs:6572:
this method can only be called during paint
```

原因：`Context::on_action` 会转发到 `Window::on_action`，后者内部有 `debug_assert_paint()`，**只能在 paint 阶段调用**。

**结论**：动作绑定必须放在渲染阶段的元素上——与 `EditorPanel` 中 `div().on_action(cx.listener(Self::save))` 的既有写法一致：

```rust
// Workspace::render 的根元素
.on_action(cx.listener(Self::open_settings))
```

已在代码注释中标注该约束，避免后续再次踩坑。

---

## 6. 设置界面设计

`src/ui/settings.rs`，以对话框呈现（`Cmd-,`）：

```
┌── 设置 ──────────────────────────────────────────┐
│ 基本    │ 主题模式        [跟随系统][浅色][深色]  │
│ 编辑器  │ 目录树字号      [−] 14 px [+]           │
│ 预览    │ 自动保存        ⟨开关⟩                  │
│         │ …（可滚动）                             │
│         │ ──────────────────────────────          │
│         │ [导入配置…] [导出配置…] [恢复默认]      │
└─────────┴────────────────────────────────────────┘
```

### 6.1 控件与配置类型的映射

| 配置类型 | 控件 | 说明 |
|---|---|---|
| 布尔 | `Switch` | 受控组件：`on_click` 只上报期望值，由 `update_config` 写回后重绘 |
| 枚举 | 分段按钮组 | 当前值高亮；选项多于 3 个时自动换行 |
| 数值 | `− 值 +` 步进器 | 按区间与步长钳制，避免用户手工输入非法值 |
| 字体 | 预设字体按钮组 | 预设覆盖常见中英文字体；若当前字体不在预设中，动态插入首位以免显示"未选中" |

> 为什么不用 `NumberInput` / `Select`：两者都需要为每个字段持有一个 `Entity<InputState>` / `SelectState`，在一个表单里管理十几份实体状态会显著增加复杂度；步进器与分段按钮是纯元素，无额外状态。

### 6.2 导入 / 导出

| 操作 | 实现 |
|---|---|
| 导出 | `cx.prompt_for_new_path()` 选路径 → 当前配置 `export_to(path)` |
| 导入 | `cx.prompt_for_paths()` 选文件 → `import_from(path)` → 成功后整体替换并 `update_config`（立即生效 + 落盘） |
| 失败处理 | 文件不存在 / JSON 损坏 → 打印错误并保持当前配置不变；只含无关字段的文件按默认配置处理 |

两者都在 `cx.spawn` 的异步任务里等待对话框结果，再通过 `cx.update` 回到主线程写配置，不阻塞 UI。

---

## 7. 配置项落地矩阵

> **补充与修正（2026-09-24）**：逐字段消费点排查、生效链路规范与接线方案见
> [`config-wiring-design.md`](./config-wiring-design.md)。本文 §7.2 的清单遗漏了
> `editor.font.weight`、`preview.font.weight` 与 `editor.keymap`（后两者连设置项入口都没有）；
> 且「仅持久化」项不再无限期挂起，改为「接线 / 降级 / 移除」三选一并按阶段处置。

数据模型与界面已覆盖全部需求项，但受 UI 框架能力限制，部分项只完成「持久化」，尚未接入渲染：

### 7.1 已接入生效

| 配置项 | 应用位置 |
|---|---|
| 主题模式 | `app.rs::apply_theme` |
| 图标大小 | `file_tree.rs`（目录树图标）、`editor.rs`（工具栏图标） |
| 目录树字体 / 字号 / 行高 / 字重 | `file_tree.rs` 行渲染 |
| 编辑器字体 / 字号 / 行高 | `editor.rs` 的 `Textarea` |
| 自动保存开关 + 延迟 | `editor.rs::schedule_save` |
| 代码高亮主题 | `preview.rs` → `highlight::highlight_with_theme` |
| 预览字体 / 字号 / 行高 | `preview.rs` 容器 |
| 正文最大宽度 | `preview.rs`（`max_w` + `mx_auto`） |
| 同步滚动 | `preview.rs`（发送侧与接收侧双向守卫） |

### 7.2 仅持久化（待接入）

| 配置项 | 受限原因 |
|---|---|
| 界面语言 | UI 文案尚未本地化，无文案可切换 |
| 默认编辑模式 | 仅源码模式实现；WYSIWYG 按 `REQUIREMENTS.md` P2 不在本期范围 |
| 缩进规则 | `TextareaState` 未暴露 `tab_size` 设置入口（该能力仅存在于代码编辑模式的内部 `TabSize`） |
| 行号显示 | `set_line_number` 只对 `LayoutMode::CodeEditor` 生效，当前编辑区是纯文本多行模式 |
| 拼写检查 | 框架未提供拼写检查接口 |
| 快捷键映射 | 仅预留 `BTreeMap<String, String>` 结构，键位覆盖逻辑待实现 |
| 图片显示方式 | 预览控件未暴露图片渲染开关 |
| 链接打开行为 | 预览控件未暴露链接点击回调 |

> 这些项在设置界面中可见可调，值会正常保存/导出；接入渲染需要框架侧能力或额外的自研实现。

---

## 8. 关键决策与权衡

| 决策 | 取舍 |
|---|---|
| 配置放在 `Global`（`AppState.config`）而非独立 `Entity` | 配置是全局只读快照，不需要订阅/事件；`Global` + `refresh_windows()` 更直接。代价是每次渲染克隆一份（结构体很小，开销可忽略） |
| 校正而非拒绝非法值 | 用户可能手工编辑 JSON。夹取保证应用永远可用；若严格拒绝，一个笔误就导致配置失效 |
| `#[serde(default)]` 容器级默认 | 新增字段天然兼容旧文件，代价是"漏配"不会报错（由默认值与测试兜底） |
| 自动保存与预览刷新共用防抖，但开关只控制写盘 | 关闭自动保存后仍要刷新预览，否则编辑完全无反馈。这里曾出现过"关自动保存连预览也停"的回归，已在实现中修正 |
| 默认保存延迟 300ms | 与 `REQUIREMENTS.md` R2.2 的既有约定保持一致，避免配置化改变默认行为 |
| 步进器 / 分段按钮替代 `NumberInput` / `Select` | 避免为每个字段维护实体状态，代价是没有自由输入（字体为预设选择） |
| 代码主题按名解析并缓存 | 缓存键由 `(语言, 代码哈希, 明暗)` 改为 `(语言, 代码哈希, 主题名)`，切换主题不会命中旧缓存；未知主题名退回默认而非 panic |

---

## 9. 与既有模块的关系

| 模块 | 改动 |
|---|---|
| `core/highlight.rs` | 新增 `theme_names()`、`highlight_with_theme()`、`default_theme()`；`Highlighter` 持有完整 `ThemeSet` |
| `core/note.rs` | 移除硬编码的 `SAVE_DEBOUNCE`（改由配置的 `auto_save_delay_ms` 驱动） |
| `app.rs` | `AppState` 增加 `config` 字段；新增 `config()` / `update_config()` / `apply_theme()`；渲染根元素绑定 `OpenSettings` |
| `ui/*` | 三个面板改为渲染时读取配置 |
| `actions.rs` / `main.rs` | 新增 `OpenSettings` 动作与 `Cmd-,` 绑定 |

---

## 10. 测试设计

配置模块不依赖 GPUI，因此全部是轻量单测（沿用本仓「模块内 `#[cfg(test)] mod tests`、独立 `tests.rs` 文件」的约定）：

| 分组 | 覆盖点 |
|---|---|
| 默认值 | 关键默认值符合总体需求（如保存延迟 300ms） |
| 兼容性 | 空对象、缺字段、未知字段、部分字段 |
| 序列化 | 往返一致；枚举 JSON 形式（`lowercase` / `snake_case`） |
| 校正 | 越界夹取、合法值不被改动、`NaN`、空字体名/主题名 |
| 导入导出 | 往返一致；文件缺失、JSON 损坏、无关字段 |
| 高亮 | 未知主题名兜底不 panic；主题列表包含默认值 |

合计新增 18 个用例，全仓 59 passed / 0 failed；`cargo clippy` 无新增警告。

---

## 11. 已知限制与演进

| 限制 | 演进方向 |
|---|---|
| 部分配置项仅持久化（见 7.2） | 待框架提供能力，或自研编辑控件（行号、缩进） |
| 字体只能从预设选择 | 引入带状态管理的文本输入，支持任意字体名 |
| 无多档案（Profile） | 如需，可在 `AppConfig` 之上增加「档案 → 配置」映射 |
| 快捷键映射未生效 | 实现「配置覆盖内置 `KeyBinding`」的加载逻辑 |
| 界面语言未本地化 | 建立文案表后，`language` 即可直接驱动 |
