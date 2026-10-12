# 设计：JSON / YAML / TOML 文本文件编辑与格式化预览

> 需求见 `docs/json-yaml-editing-requirements.md`。
> 本设计**已做源码级可行性核实**（依据均标注 文件:行号），未核实项在 §9 单独列出。
> **状态：已实施**（四项拍板结论见 §0，S1–S5 记录见 §11）。

## 0. 已确认的决策

| 项 | 结论 |
|----|------|
| 编辑区语法高亮 | **开启 tree-sitter**。`Cargo.toml` 增加 `features = ["tree-sitter","tree-sitter-json","tree-sitter-yaml","tree-sitter-toml"]`；接受首次编译时间上升 |
| YAML 库 | **`noyalib = "0.0.57"`**（纯 Rust、零 unsafe、带 serde 集成）。原计划的 `serde_yml` **已废弃**（0.0.13 仅是转发到 `noyalib` 的兼容壳），`serde_yaml` 也已标记 `0.9.34+deprecated`，都不采用 |
| 格式化策略 | **只进预览，不写回文件**。不提供「格式化并写入」，用户原文永不被程序改写（YAML 注释不会丢） |
| 一期文件类型 | `.json` / `.yaml` / `.yml` / `.toml` |

## 1. 现状与问题

| # | 现象 | 代码位置 | 影响 |
|---|------|---------|------|
| 1 | 打开入口只放行 Markdown：`if !is_markdown(&path) { return false; }` | `src/ui/tabs.rs:182` | json/yaml/toml **根本打不开** |
| 2 | 目录树默认只显示 md + 图片：`show_all \|\| is_markdown() \|\| is_image_path()` | `src/core/vault.rs:137`（判据 `is_markdown` 在 108-110） | 不开"显示全部"就看不到配置文件 |
| 3 | 文件监听的可见判据同样只有 md + 图片 | `src/core/watcher.rs:99-109` | 外部改动这类文件，树不刷新 |
| 4 | `file_kind` 把 json/yaml 归到 `Other`（注释明确写了这个决策） | `src/core/media.rs:68-78`、注释 45-46 | 图标是通用文本图标，辨识度低 |
| 5 | 编辑内核创建时**未设置语言** | `src/ui/editor.rs:64-71` | 编辑区没有语法高亮（当前也不需要） |
| 6 | `gpui-kit` 默认未启用 tree-sitter：`default = ["component","assets"]` | `gpui-kit-0.6.4/Cargo.toml:46-49` | 即使设了语言也不高亮（工厂返回 `None`，`gpui-component-0.6.4/src/highlighter/mod.rs:19-22`） |
| 7 | 预览源文一律走 Markdown 预处理 | `src/ui/editor.rs:197-203`（`preview_source` → `sanitize_for_preview` + `resolve_image_urls`） | 把 JSON 当 Markdown 渲染 → 花括号、`#`、`-` 被当标题/列表，内容错乱 |
| 8 | 工具栏是 Markdown 专用（加粗/列表/表格…） | `src/ui/editor.rs:465-561` | 数据文件下这些按钮全是误操作入口 |

**已具备、可直接复用的能力（已核实）**：

| 能力 | 证据 |
|------|------|
| 预览是 `TextView` + `code_block_highlighter` 回调，逐块调 syntect | `src/ui/preview.rs:307-332` |
| syntect 默认语法集含 JSON / YAML / TOML，按 token 或扩展名查即可 | `src/core/highlight.rs:47`（`SyntaxSet::load_defaults_newlines`）、`66-72`（`find_syntax_by_token` → `find_syntax_by_extension` → plain text 兜底） |
| 编辑内核支持按语言高亮：`EditorState::new(..).language("json")` / 运行期 `set_highlighter(lang, cx)` | `gpui-base-0.6.4/src/input/base/state.rs:9238-9251`（builder）、`773-790`（`set_highlighter`）；`EditorState = InputBaseState<EditorMode>` 见 `gpui-base-0.6.4/src/input/editor/mod.rs:11` |
| 语言能力按 feature 开关，json / yaml / toml 都有 | `gpui-kit-0.6.4/Cargo.toml` 的 `tree-sitter-json`、`tree-sitter-yaml`（206-208）、`tree-sitter-toml`（194-196）；语言名映射见 `gpui-component-0.6.4/src/highlighter/languages.rs:181`（`"json"\|"jsonc"`）、`250-251`（`"yaml"\|"yml"`）、`244-245`（`"toml"`） |
| 编辑内核有诊断（波浪线）能力：`state.diagnostics_mut().push(Diagnostic::new(range, msg))` | `gpui-base-0.6.4/src/input/base/state.rs:840-846`；`diagnostics.rs:98`（`Diagnostic::new`）、`204`（`DiagnosticSet::push`）、`192`（`DiagnosticSet::new(&Rope)`） |
| `Diagnostic` 的 `Position` 就是 `lsp_types::Position { line, character }`（0-based） | `gpui-base-0.6.4/src/input/mod.rs:101` |
| 保存链路与文件类型无关：`Note::load` / `set_text` / `save`（原子写盘） | `src/core/note.rs:68-75`、测试 `src/core/note/tests.rs:38-56` |
| 标签/会话恢复只存路径，与扩展名无关 | `src/core/session.rs` + `src/state/workspace.rs`（`save_open_notes`） |

## 2. 目标 / 非目标

**目标**
1. `.json` / `.yaml` / `.yml` / `.toml` 能在树中显示、双击打开成标签。
2. 编辑区有语言级语法高亮（tree-sitter）。
3. 预览区显示**格式化后**的内容，带代码块高亮。
4. 非法内容给出 行/列 + 原因，编辑区打标记 + 状态栏文字。
5. 保存链路完全复用现有防抖 + 原子写盘，不改 `core/note.rs`。

**非目标**
- 不改动 Markdown 的任何现有行为（判据扩展"只增不改"）。
- 不做 Schema 校验、不做树形/表格化键值编辑。
- **不写回格式化结果**（§3.3）：磁盘内容始终等于用户输入，程序只在预览里做排版。
- 不做视频预览（§9-Q5）。

## 3. 方案概述

### 3.1 整体形态

```
目录树（新增 Data 类型，可点开）
   └─ 双击 → tabs::open_note（门放宽为 is_editable_text）
        └─ EditorPanel::load_note
             ├─ EditorState.set_highlighter("json")       ← 编辑区高亮（S4）
             ├─ 工具栏切换为「数据文件模式」                  ← 隐藏 Markdown 按钮
             └─ preview_source 分支：非 md → 代码块源文      ← 预览格式化
                  └─ 校验：core/datafile.rs 纯函数 → Diagnostic / 状态栏
```

### 3.2 预览渲染：为什么用「围栏代码块」

| 方案 | 做法 | 结论 |
|------|------|------|
| **A. 围栏代码块** | 预览源文拼成 ```` ```json\n<格式化后>\n``` ```` | **选它**：完全复用现有 `TextView` + syntect 管线（`preview.rs:313-329`），零新 UI，天然带高亮、等宽、不折行；改动集中在 `preview_source` 一个函数 |
| B. 自绘键值树 | 用 `div()` 拼折叠树 | 否决：要自研折叠、缩进、换行、明暗主题适配，工作量与风险都高，且与"只读展示"的预览语义重复 |
| C. 转 Markdown 表格 | — | 否决：嵌套结构表达不了，还会触发 Markdown 专用预处理 |

关键：**非 md 文件跳过 Markdown 预处理**（`sanitize_for_preview` / `resolve_image_urls` 都不适用，
`src/ui/editor.rs:201-202`），直接产源文，省掉正则扫描与解析器 panic 防御的无效开销。

### 3.3 格式化的边界（已确认：只预览，不写回）

`serde` / `noyalib` 往返是有损的：**YAML 注释一定丢失**，引号风格、空行、anchor/alias 会被规范化。

因此：

- 格式化结果**只喂给预览**，永不写盘；
- 保存链路只写**用户原文**，任何情况下都不隐式格式化；
- 不提供「格式化并写入」按钮（放弃原 R3.6）；
- 预览顶部对 YAML 常驻一行说明：`预览为规范化结果（注释不保留）`，避免用户误以为"注释没了"。

代价：想排版文件得用户自己动手（或复制预览文本）。这是明确的产品取舍，换来了"程序绝不改坏你的文件"。

## 4. 数据模型与持久化

### 4.1 新增 `src/core/datafile.rs`（纯领域层，无 UI 依赖）

```rust
pub enum DataKind { Json, Yaml, Toml }

impl DataKind {
    /// 预览围栏语言标签，同时用作 set_highlighter 的语言名
    pub fn lang(&self) -> &'static str;      // "json" | "yaml" | "toml"
    pub fn label(&self) -> &'static str;      // 工具栏徽标显示
}

/// 扩展名 → 数据文件种类；**唯一判据来源**（树、监听、打开入口共用）
pub fn data_kind(path: &Path) -> Option<DataKind>;

pub struct ValidationError {
    /// 0-based，直接喂 lsp_types::Position
    pub line: usize,
    pub column: usize,
    pub message: String,
}

pub enum Parsed {
    /// 合法：formatted 为格式化结果
    Ok { formatted: String },
    /// 非法：original 原样返回，供预览兜底展示
    Err { original: String, error: ValidationError },
}

pub fn parse(text: &str, kind: DataKind, indent: usize) -> Parsed;
```

- **JSON**：`serde_json::from_str::<serde_json::Value>`（依赖已有）；
  出错取 `err.line()` / `err.column()`（1-based）−1 转 0-based；
  格式化用 `serde_json::ser::PrettyFormatter::with_indent(indent_bytes)`，缩进宽度接配置。
- **YAML**：`noyalib::from_str::<noyalib::Value>`，格式化 `noyalib::to_string(&value)`；
  出错取错误自带的 location（有则用，没有则退化为 `line 0, column 0`，**绝不 panic**）。
  具体 API 名（`Value` / `from_str` / `to_string` / 错误 location 方法）在 S1 用最小用例实测确认——
  `noyalib` 提供 `compat-serde-yaml` feature 与 `noyalib-serde-yaml` 兼容包作为退路。
- **TOML**：`toml = "0.8"`（`Cargo.lock` 已有 `toml 0.8.23`，pin 到 0.8 可复用、不产生第二份）；
  `toml::from_str::<toml::Value>` + `toml::to_string_pretty`。

### 4.2 配置新增

```rust
// src/core/config.rs
pub struct EditorConfig {
    ...
    /// 数据文件（json/yaml/toml）相关
    pub data_file: DataFileConfig,
}

pub struct DataFileConfig {
    /// 预览格式化缩进宽度（空格数）
    pub indent: usize,           // 默认 2
    /// 超过该字节数不做格式化预览（防卡顿），0 表示不限制
    pub max_format_bytes: usize, // 默认 1_000_000
}
```

`#[serde(default)]` + `Default` 实现，旧 `config.json` 缺字段自动取默认，**无迁移成本**。

### 4.3 磁盘真值不变

- `open_notes.json` 仍只存路径；旧版本读到 json 路径会在 `open_note` 被拒（不登记标签），
  升级后自然可用，无需数据迁移。
- 写盘路径完全复用 `core/note.rs`，包括临时文件 + rename 与 `.tmp` 忽略逻辑。

## 5. 交互流程（正常路径）

1. 目录树双击 `config.yaml` → `open_note`（判据放宽后放行）。
2. `EditorPanel::load_note` 载入文本、`set_value`（不触发 Change，已有注释说明），
   并按 `data_kind` 调 `set_highlighter("yaml", cx)` 设置编辑区语言。
3. `preview_source` 判定为非 Markdown：
   - 体积在阈值内 → `parse()` → 合法则拼 ```` ```yaml\n{formatted}\n``` ````，
     非法则拼 ```` ```yaml\n{原文}\n``` ```` 并在**预览顶部**插一行错误提示；
   - 超阈值 → 拼 ```` ```yaml\n{原文}\n``` ```` + 顶部一行「文件过大，已跳过格式化」。
4. 用户编辑 → `on_change`（`editor.rs:371`）→ 防抖（现有 300ms，`schedule_save`）→
   同时重算预览 + 重跑校验 → 有错就 `diagnostics_mut().push(...)` + `cx.notify()`。
5. 保存：防抖自动保存 / `Cmd+S` → 写**原文**。

**工具栏（数据文件模式）**：

```
[ YAML ]  ✅ 格式正确            ← 或：❌ 第 12 行第 5 列：did not find expected node content
```

只有语言徽标 + 校验状态两样东西，没有写回按钮。Markdown 模式下工具栏与今天完全一致
（`editor.rs:465-561` 不改，只是多一个分支）。

## 6. 异常与边界

| 场景 | 处理 |
|------|------|
| 语法错误 | 预览顶部红字「第 L 行第 C 列：原因」；编辑区打 Diagnostic 波浪线；**照常允许保存**（xink 是编辑器不是 IDE） |
| YAML 多文档（`---` 分隔） | 一期按单文档处理；多文档给出「暂不支持多文档」提示，不崩 |
| 空文件 | 视为合法，预览空代码块 |
| 超 1MB | 跳过格式化，预览原文 + 顶部提示（配置可调） |
| 文件被外部改动 | 复用 `handle_external_change`（`editor.rs:434-459`），逻辑与 md 一致，无需改动 |
| 非法 UTF-8 | `Note::load` 现有覆盖，打开失败走 `status` 提示 |
| tree-sitter 语言未注册 / 高亮工厂返回 None | 退化为纯文本，**不 panic**（`highlighter/mod.rs:19-22`） |
| 目录树里 `.json.bak`、无扩展名文件 | 不命中 `data_kind`，维持"普通文件"行为 |

## 7. 测试计划

**可自动化（`cargo test`，core 层纯函数，符合分层规则 02）**

| 文件 | 用例 |
|------|------|
| `src/core/datafile/tests.rs` | `data_kind` 扩展名判据（大小写 `.YAML`、多点名 `a.b.json`、无扩展名、`.json.bak` 不命中） |
| | JSON 合法 → 缩进 2 / 4 的格式化结果快照；非法 → 行列正确（行尾缺逗号、多余逗号、注释） |
| | YAML 合法 → 格式化；非法 → 行列；**固化"注释会丢"的行为**（既然只进预览，这是预期行为） |
| | TOML 合法 / 非法各一例 |
| | 空字符串、纯标量、超长单行、1MB 阈值判定 |
| `src/core/media/tests.rs` | `FileKind::Data` 判定（json/yaml/yml/toml 命中；`a.json.bak` 不命中；目录不命中） |
| `src/ui/editor/tests.rs` | `preview_source` 分支：md 走原路径；json 产出 ```` ```json ```` 围栏；超大文件跳过格式化；非法内容仍产出可渲染源文 |

**需人工验收**

- 编辑区确实有 JSON/YAML/TOML 颜色（tree-sitter 效果只能看）。
- 波浪线位置与实际错误位置一致。
- 大 JSON（>1MB）输入时不卡。
- 明暗主题下代码块高亮均可读。

## 8. 人工验收清单

- [ ] 树里能看到 `.json` / `.yaml` / `.toml`（不开"显示全部"）
- [ ] 双击打开成标签，可编辑，300ms 后自动落盘，`Cmd+S` 立即落盘
- [ ] 编辑区有语法高亮
- [ ] 预览区是缩进整齐 + 高亮的格式化结果
- [ ] YAML 预览顶部有"注释不保留"的说明
- [ ] 打错逗号 → 状态栏给出行列、编辑区有标记、预览顶部红字，应用不崩
- [ ] 保存后磁盘内容与编辑器**逐字节一致**（未被格式化改写）
- [ ] Markdown 笔记的所有行为回归无变化（工具栏、预览、图片、滚动同步）
- [ ] 外部修改 json 文件后，树与编辑区按既有规则提示/重载
- [ ] 关闭再启动，json 标签能被恢复
- [ ] `cargo tree -d` 无重复版本（重点看 `toml`、`serde`、`serde_json`）

## 9. 风险与开放问题

| # | 问题 | 状态 |
|---|------|------|
| Q1 | 是否开 tree-sitter | ✅ 已定：开启（json + yaml + toml） |
| Q2 | YAML 库选型 | ✅ 已定：`noyalib 0.0.57`（`serde_yml` / `serde_yaml` 均已废弃，不采用） |
| Q3 | 是否允许写回格式化 | ✅ 已定：不允许，只进预览 |
| Q4 | 一期是否含 toml | ✅ 已定：含 |
| **Q5** | **视频预览** | **未核实、未决**：GPUI 生态是否有可用视频解码/播放组件尚未查证（本次未做 Spike）。若要做需单独出一期 Spike 设计。另：GPUI 是桌面框架，原需求里的"移动端适配"在本仓库**无法实现** |
| **R1** | `noyalib` 具体 API 名未实测 | 缓解：S1 先写最小用例验证 `Value` / `from_str` / `to_string` / 错误 location；退路是 `noyalib-serde-yaml` 兼容包（drop-in）或 `compat-serde-yaml` feature |
| **R2** | 开启 tree-sitter 后编译时间显著上升 | 缓解：S4 单独一次提交，便于回退；若不可接受可退回"仅预览侧高亮" |
| **R3** | `toml` 版本选择 | 缓解：pin `0.8`（`Cargo.lock` 已有 0.8.23），改完跑 `cargo tree -d` 确认没有第二份 |

## 10. 实施步骤（每期可独立验证）

| 期 | 内容 | 产出可验证点 |
|----|------|-------------|
| **S1** | 新增 `core/datafile.rs` + 单测（含 `noyalib` API 实测）；依赖 `noyalib = "0.0.57"`、`toml = "0.8"`；扩展判据 `is_editable_text`（唯一来源放 `core/datafile.rs`，供 `media.rs` / `vault.rs` / `watcher.rs` / `tabs.rs` 共用），同步 `vault.rs:137`、`watcher.rs:107`、`tabs.rs:182`；`file_kind` 加 `FileKind::Data` + 图标 | `cargo test` 通过；树里能看到 json/yaml/toml；双击能打开成标签（此时无高亮、预览按 md 渲染） |
| **S2** | `preview_source` 分支 + 预览格式化 + 错误/说明提示行 + `DataFileConfig` | 打开 json 能看到格式化高亮预览；打错有提示；配置可改且生效 |
| **S3** | 工具栏数据文件模式（语言徽标 + 校验状态） | 状态徽标正确；Markdown 工具栏不受影响 |
| **S4** | 编辑区语法高亮：`Cargo.toml` 加 4 个 feature；`load_note` 里按 kind `set_highlighter` | 编辑区有颜色；`cargo tree -d` 干净 |
| **S5** | 诊断波浪线（Diagnostic）+ 大文件保护；回写 `docs/REQUIREMENTS.md`（R3.x）与本文档"已实施" | 波浪线位置正确；大文件不卡 |

> S1–S3 不触碰任何 feature 开关，编译风险最低，可先交付可用版本；
> S4 是唯一会显著改变构建时间的一步，单独提交以便回退。

## 11. 实施记录

| 期 | 状态 | 落地位置 |
|----|------|---------|
| S1 | ✅ | 新增 `src/core/datafile.rs`（`DataKind` / `data_kind` / `parse` / 行列错误）+ 单测；`core/media.rs` 新增 `is_editable_text` 与 `FileKind::Data`；`vault.rs` / `watcher.rs` / `tabs.rs` 改用同一判据；依赖 `noyalib 0.0.57` + `toml 0.8` |
| S2 | ✅ | `ui/editor.rs::preview_source` 按文件种类分支；`data_preview_source` 产出"提示 + 围栏代码块"；`DataFileConfig`（缩进 / 体积上限）+ 设置页两项 |
| S3 | ✅ | 工具栏数据文件模式：语言徽标 + 校验状态（`render_data_toolbar`），Markdown 工具栏行为不变 |
| S4 | ✅ | `Cargo.toml` 启用 `tree-sitter` + `tree-sitter-yaml` + `tree-sitter-toml`（**JSON 语法随 `tree-sitter` 自带**，gpui-kit 没有 `tree-sitter-json` 这个 feature）；`load_note` 里 `set_highlighter`，Markdown 显式设 `"text"` 以免面板复用时残留配色 |
| S5 | ✅ | `sync_diagnostics` 把校验结果下发成编辑区波浪线（`DiagnosticSet` 先 `reset` 再 `push`）；大文件保护按 `max_format_bytes` 跳过格式化 |

**实测结论（与调研时的预期不同，已按实测修正）**：

- `noyalib::to_string` 不追加结尾换行，输出形如 `name: myapp`；
- `noyalib::Error::location()` 的 line / column 是 **1-based**，与 `serde_json` 一致；
- `toml 0.8` 的 `de::Error` **没有** `line_col()`，只有 `span() -> Option<Range<usize>>`，
  行列由 `offset_to_position` 自己换算；
- `toml 0.8.23` 早已在依赖图里（`cbindgen` / `rust-i18n` 用），pin 到 0.8 未引入新的重复版本；
- `cargo tree -d` 剩下的重复项（`toml 1.1.6`、`winnow`、`base64`）是本项目改动前就存在的。

**已知限制（未解决， consciously 接受）**：

1. **滚动同步对数据文件不准**：编辑器是原文行数、预览是格式化后行数，两者不再一一对应，
   同步只能"大致对应"。要精确需按行映射，成本高于收益，暂不做。
2. **内容含 ` ``` ` 会截断预览代码块**：JSON/YAML/TOML 极少出现，最坏只是排版错乱，不崩。
3. **`.markdown` 扩展名仍不可打开**：既有行为（`note::is_markdown` 只认 `.md`），
   本次为守"不改 Markdown 现有行为"未动，只在 `media/tests.rs` 里注释说明。
4. **视频预览（R13.10）未做**：`§9-Q5`。
