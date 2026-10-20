# 设计：Markdown 事实解析（P1-T5）

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| `core/markdown.rs` 只做图片 URL 处理，不解析标题 / 链接 / wikilink / 标签 | `src/core/markdown.rs:46,199` |
| 预览的 Markdown 解析由框架内部完成，应用拿不到 AST | `src/ui/editor.rs` 用 `TextViewState::markdown` |
| `markdown` crate 只是 dev-dependency（用于在测试里复现框架行为），生产路径不解析 | `Cargo.toml:75` |
| 大纲（T8）、双链（T9）、标签（T11）、索引（T6）都需要同一批"文档里有什么"的事实 | `docs/P1-PLAN.md` T5 条目 |

缺的不是"能不能解析"，而是**统一的、带行号的、可单测的**事实来源；否则四个功能各写一份正则，行为必然不一致。

## 2. 目标 / 非目标

**目标**：从一篇笔记的原文里解析出

- 标题（层级、文本、行号）
- 链接（目标、显示文本、行号）
- wikilink（目标、别名、锚点、行号）
- 标签（名称、行号）

**非目标**

- 不做完整 GFM 解析（不需要渲染，只提取事实）
- 不解析表格内容、脚注、HTML 块
- 不做"链接是否可解析"的判断——那是索引（T6）的事

## 3. 方案概述

**手写行扫描器 + 围栏状态机，零新增依赖。**

| 备选 | 否决理由 |
|---|---|
| `pulldown-cmark` | 要拿行号得靠 `OffsetIter` 与 `into_offset_iter` 二次换算；wikilink 不在 GFM 内，仍要自己扫；为一个"提取"需求引入一个解析 crate |
| 提升 `markdown` crate 为生产依赖 | 它的 AST（`markdown::mdast`）能拿 position，但同样不含 wikilink，且解析全量 AST 对"只要四件事"过重 |
| 正则全量匹配 | 无法表达"代码块内部不算"，必须先做围栏切分，正则反而更难维护 |

扫描按行进行，维护两个状态：

1. **围栏状态**：``` 或 `~~~` 成对出现之间的行全部跳过（含围栏行自身）；
2. **frontmatter**：文件第一行是 `---` 时，到下一个 `---` 之间的内容只做 `tags` 解析。

行内还要排除 **行内代码**（反引号包裹的片段），否则 `` `#不是标签` `` 会被误判。

## 4. 数据模型

```rust
/// 一篇笔记里解析出的事实（全部带 0-based 行号，供跳转使用）
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DocFacts {
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub wiki_links: Vec<WikiLink>,
    pub tags: Vec<Tag>,
}

pub struct Heading { pub level: u8, pub text: String, pub line: usize }
pub struct Link    { pub target: String, pub text: String, pub line: usize }
pub struct WikiLink{ pub target: String, pub alias: Option<String>,
                     pub anchor: Option<String>, pub line: usize }
pub struct Tag     { pub name: String, pub line: usize }
```

- 只存**路径/名字**，不存正文：索引是派生数据，正文永远读磁盘
- 不落盘，`DocFacts` 只是解析产物

## 5. 解析规则（逐条）

**标题**：ATX，`#`~`######`，`#` 后必须有空格；去掉尾部连续的 `#` 与空白。
Setext（`===` / `---` 下划）本轮不认（Obsidian 库里极少，且容易与分隔线混淆）。

**链接**：`[文本](目标)`；目标去两端空白，去掉 `<>` 包裹；图片 `![...](...)` 也记为链接
（双链/反链不需要区分，索引侧可按需过滤）。

**wikilink**：`[[目标]]`、`[[目标|别名]]`、`[[目标#锚点]]`、`[[目标#锚点|别名]]`。
目标与别名去两端空白；空目标（`[[]]`、`[[ ]]`）丢弃。锚点不参与目标解析（T9 再决定是否跳转）。

**标签**：`#` 后紧跟**字母、中文、下划线或 `-`**，后续允许字母数字、中文、`_`、`-`、`/`（嵌套标签）。
排除：

- 行首的 `#`（Markdown 标题）——即 `#` 位于行首且后面是空格或 `#`；
- 前面紧跟可见字符的 `#`（URL 锚点 `a.md#top`、颜色 `#fff`）；
- 纯数字开头（`#123`）——与 Obsidian 一致，避免把序号当标签；
- 围栏块内、行内代码内。

**frontmatter 标签**：只认 `tags:` 同一行的值，支持 `[a, b]` 与 `a, b`；
多行列表形式（`- xxx`）本轮不支持（结构上需要缩进上下文，收益低），在文档里写明。

## 6. 异常与边界

| 输入 | 行为 |
|---|---|
| 空文件 / 全空白 | 返回空 `DocFacts`，不 panic |
| 未闭合的围栏 ```` ``` ```` | 从该行到文件末尾都视为代码块内部（与 CommonMark 一致） |
| 超长行 / 超大文件 | 按行流式扫描，不做正则回溯，**不设大小上限**（调用方 T6 负责限流） |
| 非法 UTF-8 | 输入是 `&str`，不可能出现（读取时已是 UTF-8） |
| `#` 结尾（`标题 #`） | 尾部 `#` 视为闭合符去掉 |
| CJK 标签 `#笔记/想法` | 支持（`/` 表嵌套） |

## 7. 测试计划

`tests/core/markdown.rs` 增加（纯字符串，无需临时目录）：

- 标题：各级、尾部 `#`、代码块内不算、行首 `#` 与标签的区分
- 链接：普通、图片、带空白的目标
- wikilink：纯目标、别名、锚点、空目标丢弃、同一行多个
- 标签：基础、`#123` 不算、URL 锚点不算、行内代码内不算、CJK、嵌套
- frontmatter：`tags: [a, b]`、`tags: a, b`、正文标签与 frontmatter 合并
- 行号：每条事实的 `line` 与实际行一致（0-based）

## 8. 人工验收清单

本任务无界面，验收等同于"下游功能可用"：

- [ ] T8 大纲能列出标题并跳转到对应行
- [ ] T9 双链能解析出 `[[x]]` 目标
- [ ] T11 标签面板能聚合出 `#tag`

## 9. 风险与开放问题

| # | 问题 | 建议 |
|---|---|---|
| 1 | Setext 标题是否要支持 | 不支持（极少见，易与分隔线冲突） |
| 2 | frontmatter 多行列表 tags 是否支持 | 本轮不支持，写进文档 |
| 3 | 图片链接是否计入 `links` | 计入，索引侧按需过滤 |

## 10. 实施步骤

| 步 | 内容 |
|---|---|
| S1 | `DocFacts` 等数据结构 + `parse_facts(text) -> DocFacts` 骨架（围栏状态机） |
| S2 | 标题解析 + 单测 |
| S3 | 链接与 wikilink 解析 + 单测 |
| S4 | 标签解析（含 frontmatter）+ 单测 |
