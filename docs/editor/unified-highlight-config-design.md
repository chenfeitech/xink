# 统一编辑区与预览区的高亮配置

> 状态：**已确认并实施完毕**（2026-10-20）· **M1–M4 全部完成，待人工验收**
> 决策来源：用户拍板 —— 方案 1（统一配色源 + 两端适配）、预置主题可切换、
> 预览区一并升级为完整样式（粗体 / 斜体 / 下划线）、旧 `code_theme` 做一次兼容映射
>
> M1 落地清单：`core/theme.rs` + `ui/theme.rs` + `core/themes/xink-{dark,light}.json`、
> `HighlightConfig`、`apply_theme` 注入 `highlight_theme`、设置界面「高亮主题」下拉，
> 并从界面下线旧的「代码块主题」（字段保留到 M2，启动时迁移一次）。

## 1. 现状与问题

两端各自维护一套配色，**没有任何共享**：

| | 编辑区 | 预览区 |
|---|---|---|
| 引擎 | tree-sitter（gpui-component） | syntect |
| 配色来源 | 框架内嵌 Zed 格式主题 `default-theme.json` | `preview.code_theme` → syntect 主题名 → `base16-ocean.*` |
| 项目可配 | ❌ 无配置项 | ✅ `preview.code_theme`（默认 `auto`） |
| 样式能力 | 颜色 / 字重 / 斜体 / 下划线 / 背景 | 框架给完整 `HighlightStyle`，**项目只用了颜色** |

证据：

- `src/core/highlight.rs:21` `type Span = (Range<usize>, Hsla);` —— 只取了颜色
- `src/core/highlight.rs:42-43` `DEFAULT_DARK_THEME = "base16-ocean.dark"` —— 预览区色源
- `src/core/config.rs:217` `pub code_theme: String` —— 唯一的预览区配置项
- `src/core/config.rs:19` `AUTO_CODE_THEME = "auto"` —— 跟随明暗的特殊值
- `src/ui/editor.rs`（`refresh_decorations`）用 `cx.theme().accent` —— 编辑区取的是框架主题

**一个反直觉的事实**：框架给预览区的回调返回的就是完整的 `HighlightStyle`，
不是只有颜色。

```rust
// gpui-base-0.6.4/src/text/text_view.rs:23
pub(crate) type CodeBlockHighlighterFn =
    dyn Fn(&CodeBlock) -> Vec<(Range<usize>, gpui::HighlightStyle)> + Send + Sync;
```

"两端能力不对等"里有一半是**项目自己降级**造成的。

## 2. 目标 / 非目标

**目标**

1. 高亮配色只有一个来源，编辑区与预览区都从它派生。
2. 配置项只有一份，且**同时**作用于两端——改一次两边都变。
3. 预置 2~3 套主题，跟随明暗自动切换；用户选主题名，不单独调色。
4. 预览区代码块也能有粗体 / 斜体 / 下划线（把被降级的样式能力补回来）。
5. 配置结构预留扩展：以后要开放到单个语法元素，不用改结构。

**非目标**

- 不做"逐 token 完全一致"。tree-sitter 与 syntect 的 token 划分本就不同，
  强求一致只能换引擎（见 §3 被否决方案）。
- 不做自定义主题编辑器 / 导入外部 .tmTheme。
- 不改编辑区的语法引擎（tree-sitter 的增量解析与 injection 都要保留）。

## 3. 方案概述

### 3.1 选定方案：单一主题源 + 两端派生

```
        内嵌 JSON 主题（预置 N 套 × 明暗）
                    │
                    ▼
        serde → gpui::HighlightTheme
                    │
        ┌───────────┴────────────┐
        ▼                        ▼
  编辑区：直接注入             预览区：按语义名读色
  theme.highlight_theme        syntax.style("keyword")
  （tree-sitter 用它着色）      → 构造 syntect Theme
                               → HighlightLines
```

关键在于 `SyntaxColors::style()` 是 **pub**：

```rust
// gpui-component-0.6.4/src/highlighter/registry.rs:241
pub fn style(&self, name: &str) -> Option<HighlightStyle>
```

所以同一份主题既能直接给编辑区用，又能被"读出颜色"喂给预览区——
**单一源成立，不需要维护两份配色文件**。

### 3.2 被否决的方案

| 方案 | 否决理由 |
|---|---|
| **预览区也换 tree-sitter** | 现在只注册了 json / yaml / toml / markdown 四门语言。换过去 ```rust、```python 会**直接失去高亮**，要补只能开 `tree-sitter-languages`（31 门全量，编译 + 体积代价大）。 |
| **编辑区换 syntect**（实现 `InputHighlighter`） | 会丢掉 tree-sitter 的**增量解析**与 Markdown **injection**，代码块内语言高亮和刚做的粗体/斜体全部失效。等于拆掉刚交付的功能。 |
| **两端各留一份配置文件** | 就是现状的翻版，违背本次目标。 |

## 4. 数据模型与持久化

### 4.1 配置结构

```rust
/// 高亮配置：编辑区与预览区共用（新增）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightConfig {
    /// 预置主题名（如 "xink" / "ocean"）。明暗由每套主题自带的两个变体决定。
    pub theme: String,
}
```

挂在 `Config.highlight` 上。

### 4.2 移除的旧配置

| 项 | 位置 | 处理 |
|---|---|---|
| `preview.code_theme` | `src/core/config.rs:217` | **删除** |
| `AUTO_CODE_THEME` | `src/core/config.rs:19` | **删除**——预置主题天然有明暗两个变体，不再需要 `auto` 这个特殊值 |
| `resolve_code_theme()` | `src/core/config.rs:25` | **删除** |
| `DEFAULT_DARK_THEME` / `DEFAULT_LIGHT_THEME` | `src/core/highlight.rs:42-43` | **删除**——不再用 syntect 内置主题集 |
| `ThemeSet::load_defaults()` | `src/core/highlight.rs:48` | **删除**——改用构造出的 `Theme`，顺带省掉加载内置主题集的开销 |

**实际落地（M2，与上面略有出入）**：用户选择"做一次兼容映射"，所以字段没有直接
删掉，而是换成只进不出的兼容字段：

```rust
/// 仅用于反序列化时捕获旧配置并迁移一次；`skip_serializing` 保证写回时不再出现
#[serde(default, skip_serializing)]
pub legacy_code_theme: Option<String>,
```

`sanitize_highlight()` 把它 `take()` 掉并映射到预置主题（`core::theme::migrate_code_theme`），
之后写回配置不再带这个字段，因此**迁移只发生一次**，不会覆盖用户后选的主题。

映射本身仍然是一对多的近似（syntect 主题名 → 预置主题名），目前只有一套预置主题，
所有旧值都收敛到 `xink`。

### 4.3 预置主题资源

用 `include_str!` 编译期内嵌（框架自己也是这么做的），不新增依赖：

```
src/core/themes/
├── xink-dark.json        # 默认，取自框架 default-theme.json 的 syntax 段
├── xink-light.json
├── ocean-dark.json       # 由 syntect 的 base16-ocean.dark 转换
├── ocean-light.json      # 由 base16-ocean.light 转换
├── solarized-dark.json   # 由 "Solarized (dark)" 转换
└── solarized-light.json  # 由 "Solarized (light)" 转换
```

格式 = 框架的 `HighlightTheme` JSON（`{ name, style: { syntax: { … } } }`），
与 `default-theme.json` 同构，可直接在框架源码里抄结构。

**第 2、3 套怎么来的**：不是手写的，而是用 `SCOPE_MAP` **反向**从 syntect 主题生成——
对每个语义名取其第一个映射选择器，用 `Highlighter::style_mod_for_stack()` 问出颜色
（没有规则命中就跳过该条目），再序列化成 JSON。好处是生成出的主题与预览区渲染
用的是同一张映射表，**不存在"主题写了但预览区取不到"的情况**。
生成脚本是一次性的（跑完即删），产物就是上面 4 个 JSON。

> **已知现象**：base16 / Solarized 的明暗两套只在**底色**上区分，强调色（base08~0F）
> 是同一套，所以 `ocean` / `solarized` 的语法色明暗差异很小（实测只有 comment 等少数
> 条目不同），切明暗时代码块观感变化不如 `xink` 明显。这是主题本身的性质，不是缺陷。

> **约束**：`SyntaxColors` 的 `ThemeStyle` 字段是**私有**的（`registry.rs` 中
> `color / font_style / font_weight` 均无 `pub`），无法从代码构造，
> 只能走 serde 反序列化。所以预置主题**必须是 JSON**，不能是 Rust 常量。

### 4.4 预览区样式类型

现在 `Span = (Range, Hsla)` 只带颜色，升级为：

```rust
/// 代码块高亮片段的样式
///
/// 保留 `Hsla`（`core` 早已依赖），但把被丢掉的粗体 / 斜体 / 下划线捡回来。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanStyle {
    pub color: Hsla,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}
```

缓存 key 同步调整：现在 `(语言, 代码哈希, 主题名)`，主题名改为
`format!("{theme_name}.{dark|light}")`，保证换主题 / 换明暗都会失效重算。

## 5. 交互流程

**切换明暗**（已有路径）

1. `apply_theme()` → `Theme::change(...)`
2. 紧随其后（跟现有 `sheet.margin_top` 同一处）：按 `highlight.theme + is_dark`
   加载对应 JSON → 反序列化 → `Theme::global_mut(cx).highlight_theme = Arc::new(...)`
3. 编辑区下一帧自动用新主题（tree-sitter 解析走 `highlight_theme`）
4. 预览区下一次渲染时，`core::highlight` 用同一份主题构造 syntect `Theme`

**切换预置主题**（设置界面新增下拉）

1. 设置项 `highlight.theme` 变更 → `update_config()`
2. 重跑 `apply_theme()`（与切换明暗同一条路径）
3. 两端同时生效

**预览区着色**

1. `TextView::code_block_highlighter` 回调 → `core::highlight::highlight(lang, code, theme)`
2. 内部：`HighlightLines` 用**由统一主题构造出的** syntect `Theme`
3. 输出 `Vec<(Range, SpanStyle)>` → ui 层转 `gpui::HighlightStyle`

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 主题名不存在（手改 config.json） | 校正为默认主题，**不 panic**（与现有 `Highlighter::new` 用 `get` 而非下标的做法一致） |
| JSON 反序列化失败 | 退回框架默认主题 `HighlightTheme::default_light/dark`，并在启动日志提示 |
| 某语法元素在主题里没配色（例如 `text.code.span`） | `style()` 返回 `None` → 该元素不着色，用前景色；**不报错** |
| syntect 不认识某语言 | 现有逻辑：退回纯文本，保持 |
| 旧的 `preview.code_theme` | `legacy_code_theme` 捕获 → 映射成预置主题后丢弃（见 §4.2） |
| 编辑区 injection 未覆盖的语言 | 保持现状（静默退化为纯文本），本设计不改变 |

## 7. 测试计划

**可自动化**

- `resolve_theme_json(name, is_dark) -> &'static str`：预置名 → 资源路径（纯函数，含未知名兜底）
- syntect `Theme` 构造：给定统一主题，断言 `keyword` 的 `StyleModifier.foreground` 等于主题里 `keyword` 的颜色
- `SpanStyle` 转换：syntect `Style`（含 BOLD / ITALIC / UNDERLINE 位）→ `SpanStyle` 三个标志位
- 缓存 key：换主题名 / 换明暗后 key 必须不同
- 两端一致性**单测**：对同一段 Rust 代码，断言"预览区取到的 keyword 颜色" ==
  "统一主题里 keyword 的颜色"（这条直接锁死"两端同源"）

**需人工**

- 编辑区与预览区同一个 ```rust 块：配色是否一致
- 切换明暗：两端是否**同时**变
- 切换预置主题：两端是否**同时**变（不允许只变一边）
- 预览区代码块是否出现粗体 / 斜体

## 8. 人工验收清单

- [ ] 打开含 ```rust / ```json / ```python 的笔记：编辑区与预览区配色一致
- [ ] 切到浅色主题：两端**同时**切换，不存在"编辑区变了预览没变"
- [ ] 设置里切换预置主题：两端**同时**变
- [ ] 预览区代码块的粗体 / 斜体是否生效（换一个有明显粗体的语言验证）
- [ ] 手改 config.json 填不存在的主题名：不崩溃，退回默认
- [ ] 旧配置（含 `preview.code_theme`）升级后：正常启动，配置里不再出现该字段
- [ ] 启动耗时与二进制体积：应**下降**（不再 `ThemeSet::load_defaults()`）

## 9. 风险与开放问题

1. **语义名 → TextMate scope 映射表**（最大工作量，约 20~30 条）。
   syntect 用 scope 选择器（`keyword`、`keyword.control`、`string.quoted`…），
   跟框架的 41 个语义名不是一一对应。映射写错的表现是"预览区某一类不着色"，
   且**不报错**，只能靠人工验收发现。

   已实测确认的两处偏差（写进 `SCOPE_MAP` 注释与测试注释）：

   - Rust 的 `fn` 在 Sublime 语法里是 `storage.type.function`，不是 `keyword.*`；
     已在 `keyword` 里显式加上 `storage.type.function`（比 `type` 的 `storage.type`
     多一段，syntect 打分时优先命中），与 tree-sitter 的分类对齐。
   - Rust 的 `let` 与 `u32` **共用** `storage.type`，scope 层面无法区分，
     现在整类归到 `type`：预览区 `let` 是类型色，编辑区是关键字色。
     这是引擎差异，无法在静态主题里消除（主题没有语言上下文）。
2. **一致性的边界**：本方案保证**配色一致**，不保证 token 划分一致。
   同一个 ```rust 块两边着色位置可能有细微差异（引擎差异，无法消除）。
3. ~~旧的 `code_theme` 不做值迁移~~ —— 已按用户决策改为**做一次兼容映射**
   （`legacy_code_theme` + `migrate_code_theme`），见 §4.2。
   映射是一对多的近似，用户选过的 syntect 主题名本身不会保留。
4. ~~预置主题从哪来~~：第 1 套 `xink` 取框架 `default-theme.json` 的 syntax 段；
   第 2、3 套（`ocean` / `solarized`）由 syntect 内置主题经 `SCOPE_MAP` 转换生成，
   见 §4.3。**注意**这两套的明暗差异只在少数条目上（base16 / Solarized 强调色共用）。
5. **预览区粗体的实际观感**：代码块里加粗比较罕见，主要收益是斜体与下划线；
   如果实测发现加粗让代码块变难看，可以把 `bold` 关掉（配置结构已预留）。

## 10. 实施步骤

| 步骤 | 内容 | 可独立验证 |
|---|---|---|
| **M1** ✅ | 新增 `HighlightConfig`；加 1 套预置主题（`xink` 明暗两个 JSON）；编辑区适配器（注入 `theme.highlight_theme`）；设置界面加下拉 | 编辑区换主题生效（待人工验收） |
| **M2** ✅ | 预览区适配器：统一主题 → syntect `Theme`（`SCOPE_MAP` 语义名 → TextMate scope）；旧 `code_theme` 换成只进不出的 `legacy_code_theme`；删除 `AUTO_CODE_THEME` / `ThemeSet::load_defaults`；缓存 key 调整 | 预览区换主题生效，两端同源 |
| **M3** ✅ | `SpanStyle` 升级（粗体 / 斜体 / 下划线，`color: Option<Hsla>`） | 预览区代码块有字重与斜体 |
| **M4** ✅ | 补齐第 2、3 套预置主题（`ocean` / `solarized`，由 syntect 主题经 `SCOPE_MAP` 转换）；旧 `code_theme` 映射同步更新 | 主题可切换（3 套）；§8 验收项待人工跑 |

M1→M4 每步都能单独编译与验证，可以按步交付。
