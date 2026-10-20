# 编辑区语法高亮：调研与实施记录

> 类型：技术调研（`eval`）+ 实施记录
> 日期：2026-10-20
> 范围：M0（Markdown 正文高亮）+ M1（json / yaml 高亮）
> 状态：**已实施，待人工验收**

## 1. 结论先行

框架能力是**现成的**，缺的只是没开 feature。最终改动量：1 个 Cargo feature + 1 处语言名判定。

| 能力 | 框架是否支持 | 本项目的开关状态 |
|---|---|---|
| 编辑区 Markdown 语法高亮 | ✅ 支持 | 需要 `tree-sitter-markdown` |
| 围栏代码块按语言注入高亮（injection） | ✅ 支持 | 需要 markdown + 目标语言都注册 |
| 编辑区 json / yaml / toml 高亮 | ✅ 支持 | 本来就已启用 |
| 明暗主题跟随 | ✅ 支持 | 用 Zed 格式主题，与预览区 syntect 是两套 |

## 2. 现状与问题（改动前）

| 场景 | 改动前 | 证据 |
|---|---|---|
| 编辑区：JSON / YAML / TOML 数据文件 | ✅ 有高亮 | `src/ui/editor.rs:394` `data_kind(...).lang()` |
| 编辑区：**Markdown 笔记正文** | ❌ 无 | 同一处 `unwrap_or("text")`，注释自述"沿用现状：不高亮" |
| 预览区：Markdown 代码块 | ✅ 有（syntect） | `src/core/highlight.rs` |

根因：`Cargo.toml` 只开了 `tree-sitter` / `-yaml` / `-toml`，`Cargo.lock` 里只有
`tree-sitter`、`tree-sitter-json`、`tree-sitter-toml-ng`、`tree-sitter-yaml` 四个语法包。

## 3. 框架侧核实（基于 gpui-component 0.6.4 源码）

- 语言注册表：`src/highlighter/languages.rs` 的 `enum Language`，含 `Markdown` / `MarkdownInline` /
  `Rust` / `Python` / `Bash` … 共 30+ 门，逐个由 `#[cfg(feature = "tree-sitter-xxx")]` 控制。
- `set_highlighter(name, cx)`（`gpui-base-0.6.4/src/input/base/state.rs:772`）
  只做两件事：改语言名 + 清空缓存 highlighter。
- **语言名大小写敏感**：`registry.rs` 的测试明确 `Language::from_str("JSON")` 匹配不上。
  传 `"Markdown"` 会静默失效，必须全小写。
- **Markdown 二段语法**：`Markdown` 用 `tree_sitter_md::LANGUAGE`，`MarkdownInline` 用
  `tree_sitter_md::INLINE_LANGUAGE`，两者共用 `tree-sitter-markdown` 一个 cfg。
- **injection 已实现**：`languages/markdown/injections.scm`
  ```scm
  (fenced_code_block (info_string (language) @injection.language)
                     (code_fence_content) @injection.content)
  ```
  围栏代码块按 info_string 的语言着色——但**被注入的语言自身也必须已注册**。
- **未注册 = 静默退化**：`SyntaxHighlighter::new` 只 `tracing::warn!` 后退回 `"text"`，
  无 panic、无 UI 提示。这是排查时最坑的一点。
- 性能：增量解析（`InputEdit` + 复用 `old_tree`）；>256KB 才走后台线程（150ms 防抖）。

## 4. 做了什么

1. `Cargo.toml`：gpui-kit features 增加 `"tree-sitter-markdown"`。
2. `src/core/media.rs`：新增 `editor_language(path) -> &'static str`，
   **收敛**"编辑区语言名"的唯一判据（数据文件复用 `DataKind::lang()`，Markdown 走 `is_markdown`，其余 `text`）。
3. `src/ui/editor.rs`：载入笔记时改用 `editor_language(note.path())`。
4. `tests/core/media.rs`：新增 3 个回归测试（语言映射 / 兜底 / 大小写）。

代码块内的 ```json / ```yaml 高亮**不需要额外改动**——json 随 `tree-sitter` 自带，
yaml 已启用，开 markdown 后由 injection 自动生效。toml 保持启用。

### 4.1 装饰层（路径 B）

tree-sitter 只管"按语法着色"，行首标记的弱化与链接下划线得靠装饰层：

5. `src/core/markdown.rs`：新增 `decoration_spans(text) -> Vec<DecorationSpan>`，
   扫出四类字节范围——标题 `#`、列表符号、块引用 `>`、链接文字。
   **领域层只产出范围与语义，不碰颜色**（`core` 不引用 gpui 类型）。
   围栏代码块与 frontmatter 内部一律跳过。
6. `src/ui/editor.rs`：新增 `refresh_decorations()`，把范围翻译成 `TextDecoration`
   下发给编辑控件，并挂在自动保存的防抖上（保存 / 预览 / 装饰共用一个防抖）。
7. `tests/core/markdown.rs`：装饰扫描 8 个回归测试（含中文多字节偏移、围栏、
   frontmatter、`#hashtag` 不是标题）。

样式映射：

| 语义 | 样式 | 说明 |
|---|---|---|
| 标题 `#` / 列表符号 | `fade_out: 0.45` | 框架自带淡化，跟随主题，不用自己挑颜色 |
| 块引用 `>` | `color: theme.accent` | 框架无行级绘制，**画不出竖线**，只能给标记上色 |
| 链接文字 | `underline`（实线 1px） | 颜色留空即跟随文字色 |

> 主题层做不了下划线：`ThemeStyle` 只有 `color / font_style / font_weight`，
> 且 `FontStyle::Underline` 被映射成 `Normal`（空实现）。所以下划线**只能**走装饰层。

## 5. 人工验收清单

- [ ] 打开一篇 `.md` 笔记：标题 `#`、粗体 `**`、行内代码、链接、列表符号应有着色
- [ ] 在笔记里写 ```json 代码块：编辑区内应有 JSON 着色（切到预览区也应着色）
- [ ] 在笔记里写 ```yaml 代码块：同上
- [ ] 打开 `.json` / `.yaml` / `.toml` 文件：仍按各自语言高亮（回归，不应退化）
- [ ] 打开 `.txt` 或未知扩展名：纯文本，无着色（不应残留上一门语言的配色）
- [ ] 先打开 json 再打开 md：不应残留 json 的配色（面板复用场景）
- [ ] 切到浅色主题：着色跟随，无"深色残留"
- [ ] 一篇几百行、含多个代码块的笔记：滚动与输入不卡

**装饰层**

- [ ] `#` 与列表符号 `-` / `1.` 是否淡化（但不消失）
- [ ] 链接文字是否有下划线，括号与 URL 不带下划线
- [ ] `>` 块引用标记是否有颜色（**不是竖线**，竖线框架做不出来）
- [ ] 在围栏代码块里写 `# 注释` / `- 项`：不应被装饰
- [ ] 编辑标题时装饰是否跟随（防抖结束后），不应出现错位
- [ ] 切到 json / yaml 文件：不应有 Markdown 装饰
- [ ] 关闭标签后新建空白页：不应残留上一篇的装饰

## 6. 开放问题（未做，等拍板）

1. **是否给"整篇 Markdown 高亮"加开关？**
   参照 Obsidian：它编辑 Markdown 源码时默认**不高亮**（只高亮代码块）。
   整篇高亮是审美偏好。本次默认打开；若要可配置，属 M2。
2. **编辑区与预览区主题不一致。**
   编辑区走 Zed 格式 JSON 主题，预览区走 syntect + base16
   （`core/highlight.rs` 的 `base16-ocean.dark/light`）。
   同一个 ```rust 块两处配色可能不同；且 syntect 认识的语言远多于已注册的
   tree-sitter 语言，会出现"预览有色、编辑无色"。属 M2。
3. **是否继续加语言？**
   当前 json / yaml / toml / markdown 四门。若笔记里常有 ```rust / ```python / ```bash，
   需要各自加 feature（每个一个 C 语法包）。建议先扫一遍库里代码块的语言分布再决定，
   不要直接开 `tree-sitter-languages`（31 门全量，编译与体积代价大）。
4. **装饰的重算成本未实测**：每次防抖结束都对整篇全量重扫一遍
   （`decoration_spans` 是 O(全文)）。笔记在几百行内应该无感，
   但超大文档（几千行）是否掉帧没验证过。真有问题再考虑增量 / 只扫可见区。
5. **`.markdown` 扩展名当前不可用**（既有问题，非本次引入）：
   `note::is_markdown` 只认 `.md`，但 `media::file_kind` 认 `md | markdown`。
   结果是 `.markdown` 文件在目录树显示 Markdown 图标，却**双击打不开**。
   这与 `media.rs` 模块文档强调的"判据必须只有一份"相悖。修法一行，待确认。

## 7. 代价

- 新增 1 个语法包 `tree-sitter-md`（C 源码，需 `cc` 编译）。
- 二进制体积增量未精确测量（单个语法包，量级在数百 KB）。
- 编译时间：首次需编译该 C 语法源码；增量编译不受影响。
