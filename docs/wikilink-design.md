# 设计：双链 `[[笔记名]]`（P1-T9 / R5）

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 已能解析双链（目标 / 别名 / 锚点 + 行号） | `core/markdown.rs::parse_facts`（T5） |
| 已能解析"双链目标 → 笔记路径"（含歧义） | `core/index.rs::resolve_wikilink`（T6） |
| 预览点击链接已有统一出口：库内 `.md` 走 `open_note`，否则浏览器 | `ui/preview.rs::handle_link_click`（`:384`） |
| 预览源文在 `preview_source` 里已经过"清洗 → 图片 URL 补全"两步 | `ui/editor.rs::preview_source`（`:242`） |
| 打开笔记的唯一入口是 `tabs::open_note` | `docs/DESIGN.md` §2.5 |

要做的是：把 `[[目标]]` 在预览里变成可点链接，并保证点击后走既有入口。

## 2. Spike 结论：`[[` 自动补全

| 结论 | 依据 |
|---|---|
| 框架**有** `CompletionProvider` trait 与 `Lsp::completion_provider` 字段 | `gpui-base-0.6.4/src/input/editor/lsp/completions.rs:37`、`lsp/mod.rs:41` |
| 但**没有触发链路**：全仓只有字段声明，没有任何请求补全的调用点 | `grep -rn "completion_provider"` 仅命中 `lsp/mod.rs:41,74` |
| 因此不能"接上去就用"，需要自建弹层 | — |

自建方案（本轮**实现**，与命令面板同一套范式）：

- 输入 `[[` 后在光标前匹配 `/\[\[([^\]\[\n]*)$/` → 打开候选浮层（Dialog + Command，复用 T7 的范式）
- 候选取索引里的笔记标题 / 文件名（模糊排序走 `core::fuzzy`）
- 选中即把 `[[查询` 这段替换为 `[[目标]]`，光标落到 `]]` 之后，焦点还给编辑区

风险与兜底：浮层会抢焦点，确认后必须显式还焦点与光标位置；
若实测焦点回落不稳，降级为"命令面板插入链接"（T7 的 `[[` 入口）。

## 3. 目标 / 非目标

**目标**

- 预览里 `[[目标]]` 渲染为可点链接，点击打开目标笔记（走 `open_note`）
- `[[目标|别名]]` 显示别名；`[[目标#锚点]]` 至少能跳到笔记（锚点本轮不定位）
- 输入 `[[` 给出候选补全

**非目标**

- 不改写用户磁盘上的原文（预览改写只发生在渲染前的源文副本）
- 不做"重命名笔记时自动更新引用它的链接"
- 锚点不定位到具体小节（索引里没有锚点表，留待后续）

## 4. 数据模型与改写规则

新增纯函数（可单测）：

```rust
pub fn rewrite_wikilinks(md: &str, resolve: impl Fn(&str) -> Option<PathBuf>) -> String
```

规则：

| 输入 | 输出 |
|---|---|
| `[[目标]]`（可解析） | `[目标](file://<编码后的绝对路径>)` |
| `[[目标\|别名]]`（可解析） | `[别名](file://…)` |
| `[[目标]]`（解析不到） | 原样保留（不可点，避免点开一个不存在的文件） |
| 围栏代码块内 / 行内代码内 | 一律不动（与 `parse_facts` 口径一致） |

`file://` URL 用既有的 `encode_path` 做 percent 编码（中文与空格路径才能被
`path_from_file_url` 正确还原，见 `handle_link_click`）。

## 5. 关键流程

```
preview_source(note)
  → sanitize_for_preview
  → resolve_image_urls
  → rewrite_wikilinks(resolve)        // 新增
      resolve = AppState.notes.resolve_wikilink(target).hit()
  → TextView 渲染
点击 → handle_link_click → path_from_file_url → open_note（唯一入口）
```

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 索引还没构建完（首次启动） | 解析不到 → 保留原样；构建完成后下次刷新预览即可点击 |
| 目标笔记在库外 / 是数据文件 | 索引只收 `.md`，自然解析不到 → 保留原样 |
| 同名笔记（歧义） | `resolve_wikilink` 返回 `Ambiguous` → 视为解析不到，保留原样 |
| 路径含空格 / 中文 | `encode_path` 编码；点击侧已能解码 |
| 数据文件（json/yaml）里有 `[[..]]` | 不进 Markdown 预览链路（`preview_source` 已分流） |
| 空目标 `[[]]` | `parse_wiki_inner` 已丢弃，不会被改写 |
| 浮层被点遮罩关掉（不走 `on_cancel`） | 编辑区 `completion_open` 会卡在 true；因此在浮层实体上挂 `cx.on_release` 兜底复位 |
| 浮层里打字却没反应 | dialog 的 build 闭包每次重绘都会重放，实体必须建在 `open_dialog` 外面（详见 `docs/command-palette-design.md` §3） |

## 7. 测试计划

`tests/core/markdown.rs` 增加（纯字符串，用假的 resolver）：

- 可解析 → 生成 Markdown 链接，显示文本取别名 / 否则目标
- 解析不到 → 原样返回（byte-for-byte）
- 围栏代码块内与行内代码内不改写
- 一行多处双链都能改写
- 锚点形式 `[[目标#小节]]` 以整篇为目标

## 8. 人工验收清单

- [ ] 预览里 `[[存在的笔记]]` 显示为链接，点击打开目标笔记（**不产生第二个标签**，若已打开则只激活）
- [ ] `[[目标|别名]]` 显示别名
- [ ] `[[不存在]]` 显示为普通文本且点击无副作用
- [ ] 代码块里的 `[[x]]` 不被改写
- [ ] 输入 `[[` 弹出候选，继续输入可过滤，回车补全为 `[[目标]]`，光标在 `]]` 后，焦点回到编辑区
- [ ] 中文路径的笔记能被正确打开

## 9. 开放问题

| # | 问题 | 决定 |
|---|---|---|
| 1 | 目标不存在是否点击即创建 | 本轮不做（渲染层能否安全传递自定义 scheme 未验证） |
| 2 | 补全候选是否包含"新建同名笔记" | 不做，同上 |
| 3 | 锚点定位 | 不做，索引无锚点表 |

## 10. 实施步骤

| 步 | 内容 |
|---|---|
| S1 | `rewrite_wikilinks` 纯函数 + 单测 |
| S2 | 接入 `preview_source`（索引解析） |
| S3 | `[[` 补全弹层（自建，复用命令面板范式） |
