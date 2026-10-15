# P1 任务分解与执行计划

> 目的：把 `docs/REQUIREMENTS.md` §4 的 P1（R5–R13）从"一条一条需求"拆成
> **可独立验收、可逐步提交**的细粒度任务，并明确依赖顺序与每步的验收方式。
> 本文件是**执行计划**，不等于设计文档；标注 `需设计` 的任务在动手前另写
> `docs/<slug>-design.md` 并经确认（规则 `05-design-first`）。

## 1. 现状盘点（证据）

| 项 | 现状 | 证据 |
|---|---|---|
| 索引 | **没有**。`core/` 无 `index.rs`，无任何全库索引 / 搜索 | `src/core/mod.rs:5-14` |
| Markdown 解析 | 只做图片 URL 处理，**不解析标题/链接/wikilink/标签** | `src/core/markdown.rs:46/199`（仅 `regex` 处理图片） |
| 打开笔记 | 唯一入口 `tabs::open_note(path, window, cx)`，**不支持跳到指定行** | `src/ui/tabs.rs:181`；调用点 `app.rs:475`、`file_tree.rs:380`、`preview.rs:392` |
| 面板与布局 | `DockArea("main-dock")` v2，左 Files / 中 Editor / 右 Preview | `src/app.rs:312-324, 502-522`；面板工厂注册 `src/main.rs:20-30` |
| Action / 键位 | 7 个 action，**无 Markdown 格式快捷键**（只有工具栏按钮） | `src/actions.rs:5-23`、`src/main.rs:46-56`、`src/ui/editor.rs:1001-1106` |
| 主题 | `ThemeMode::{System,Light,Dark}`，已跟随系统，**只有设置页入口** | `src/core/config.rs:39-44`、`src/app.rs:169-206`、`src/ui/settings.rs:225-252` |
| 数据文件 | R13 除"波浪线诊断"外基本完成 | `src/core/datafile.rs`、`src/ui/editor.rs:240-253,283-288,865-904` |
| 文件监听 | `notify` → `WatchEvent{Created,Removed,Modified}`，订阅在文件树 | `src/core/watcher.rs:18-24`、`src/ui/file_tree.rs:391-407` |
| 测试挂载 | `src` 末尾 `#[path]` 挂 `tests/`；`autotests = false` | `Cargo.toml:13`、`docs/TESTS.md:16-46` |

### 已核实的框架能力（避免误判）

| 能力 | 结论 |
|---|---|
| 命令面板 | **框架有**：`gpui_component::command::{Command, CommandState, CommandItem, CommandGroup}`，带 `on_query/on_select/on_confirm/on_cancel`、键盘导航、分组、空态 |
| 面板内置匹配 | **只有大小写不敏感的子串包含**（`command/item.rs:114-128`），**无模糊、无排序** → 需要自研 `core/fuzzy.rs` 并 `filterable(false)` + `on_query` 自行排序 |
| 输入框 | `gpui_component::input::{Input, InputState}` 可用（搜索框） |
| 列表 | `list/`（含 `ListItem` 分隔）、`virtual_list`、`searchable_list` 可用 |
| 浮层承载 | `dialog/`、`sheet.rs`、`popover.rs` 可用；**命令面板应以哪种承载 + 焦点/Esc 行为需先 Spike 确认** |

## 2. 任务总览

```
阶段 A 热身（各自独立、低风险）
  T1 Markdown 快捷键 R12 ── T2 主题入口 R11 ── T3 R13 收尾

阶段 B 公共底座（纯 core，全部可单测）
  T4 core/fuzzy.rs  ──  T5 markdown 事实解析  ──  T6 core/index.rs

阶段 C~H 功能（均依赖底座）
  T7 命令面板 R10 ← T4
  T8 大纲 R9      ← T5.2 + T8.1(跳转行能力)
  T9 双链 R5      ← T5.3 + T6
  T10 反链 R6     ← T6
  T11 标签 R7     ← T5.4 + T6
  T12 全文搜索 R8 ← T4 + T6
阶段 I 收尾
  T13 联调 / 性能 / 文档
```

| # | 任务 | 需求 | 依赖 | 规模 | 设计文档 |
|---|---|---|---|---|---|
| T1 | Markdown 快捷键 | R12 | — | 0.5d | 免 |
| T2 | 主题切换入口 | R11 | — | 0.5d | 免 |
| T3 | 数据文件收尾 | R13 | — | 0.5d | 免 |
| T4 | 模糊匹配器 | R10/R8 | — | 0.5d | 免 |
| T5 | Markdown 事实解析 | R5/R7/R9 | — | 1.5d | **需** |
| T6 | 全库索引 | R5/R6/R7/R8 | T5 | 2d | **需** |
| T7 | 命令面板 | R10 | T4 | 1.5d | **需**（含 Spike） |
| T8 | 大纲面板 | R9 | T5.2 | 1.5d | **需** |
| T9 | 双链 | R5 | T5.3 + T6 | 2–3d | **需**（含 Spike） |
| T10 | 反向链接 | R6 | T6 | 1d | 并入 T6/T9 设计 |
| T11 | 标签 | R7 | T5.4 + T6 | 1.5d | 并入 T6 设计 |
| T12 | 全文搜索 | R8 | T4 + T6 | 2d | **需** |
| T13 | 联调与收尾 | — | 全部 | 1d | 免 |

**执行原则**：一次只做一个 T；每个 T 结束必须 `cargo build` + `cargo test` + 人工验收通过再进下一个；
每个 T 单独一个 commit（规则 `git.md`）；`core/` 新增逻辑必须有 `#[test]`，UI 行为写进人工验收清单。

---

## 3. 阶段 A：热身（三天内可清完，互不阻塞）

### T1 — Markdown 快捷键（R12）

| 子任务 | 内容 | 落点 |
|---|---|---|
| T1.1 | 新增 actions：`Bold` / `Italic` / `Strikethrough` / `InlineCode` / `InsertLink` | `src/actions.rs` |
| T1.2 | 绑定键位 `cmd-b` / `cmd-i` / `cmd-shift-x` / `cmd-e` / `cmd-k` | `src/main.rs:46-56` |
| T1.3 | `on_action` 注册，复用已有 `Format::{Bold,Italic}` 与插入逻辑，不重写第二套 | `src/ui/editor.rs:977,1001-1106` |
| T1.4 | 抽取"包裹选区"为 `core` 纯函数（无选中时插入占位并把光标放中间），便于单测 | 新 `src/core/text_format.rs` 或并入 `markdown.rs` |
| T1.5 | 单测：有选中 / 无选中 / 已包裹再按一次（**先不做 toggle，只包裹**）/ 多行选区 | `tests/core/text_format.rs` |
| T1.6 | 人工验收：选中文字 `Cmd+B` 立即加粗、光标位置合理、输入框内不被全局快捷键抢 | — |

### T2 — 主题切换入口（R11）

| 子任务 | 内容 | 落点 |
|---|---|---|
| T2.1 | 新增 action `ToggleTheme`，绑定快捷键（**键位待定，见 Q5**，暂定 `cmd-shift-l`） | `src/actions.rs`、`src/main.rs` |
| T2.2 | 切换 `basic.theme` 后走既有 `apply_theme`，验证编辑区 / 预览 / 面板 / 代码块主题同步 | `src/app.rs:162,169-206`、`config.rs:202` |
| T2.3 | 验证"跟随系统"：系统切浅色/深色时应用跟随（不重启） | `src/app.rs:176-186` |
| T2.4 | 人工验收：切换无残留（不重启窗口）、重启后保持设置、代码高亮跟随明暗 | — |

### T3 — 数据文件收尾（R13）

| 子任务 | 内容 | 落点 |
|---|---|---|
| T3.1 | 核验非法内容是否显示波浪线（`sync_diagnostics`），能显示则收尾，不能则记入文档"已知限制" | `src/ui/editor.rs:306,333` |
| T3.2 | 按 `docs/json-yaml-editing-requirements.md` 逐条人工验收并在文档中勾选 | 该文档 |
| T3.3 | R13 到此结项，后续 P1 不再回头 | — |

---

## 4. 阶段 B：公共底座

### T4 — `core/fuzzy.rs`（零依赖）

| 子任务 | 内容 |
|---|---|
| T4.1 | 子序列匹配：`matches(query, text) -> Option<FuzzyMatch{score, positions}>` |
| T4.2 | 打分：连续命中、词首、路径分隔符后、驼峰边界加权；短文本与前缀命中加权 |
| T4.3 | **按字符迭代**（不能按字节切片），保证中文不 panic；大小写不敏感 |
| T4.4 | `positions` 供 UI 高亮命中字符 |
| T4.5 | 单测 ≥ 10 例：空查询、无匹配、大小写、CJK、路径 `foo/bar.md`、打分排序稳定性 |
| T4.6 | 明确**不引入第三方 crate**（自研约 100 行，零依赖、可控）；若自研效果不足再评估 `nucleo-matcher` |

### T5 — Markdown 事实解析（`core/markdown.rs` 扩展）— 需设计

设计要点（写进 `docs/markdown-facts-design.md`）：

| 子任务 | 内容 |
|---|---|
| T5.1 | 定义 `struct DocFacts { headings: Vec<Heading>, links: Vec<Link>, wiki_links: Vec<WikiLink>, tags: Vec<Tag> }`，每条带**行号**（供跳转）与字节范围 |
| T5.2 | 标题解析：`#`~`######`，支持尾部 `#`，**跳过围栏代码块内** |
| T5.3 | 链接解析：行内 `[a](b)`；wikilink `[[目标]]` / `[[目标\|别名]]` / `[[目标#锚点]]`（锚点可先解析不跳转） |
| T5.4 | 标签解析：正文 `#tag`（排除标题行首 `#`、URL 锚点、代码内）+ YAML frontmatter `tags: [a, b]` |
| T5.5 | 技术选型：**正则 + 围栏状态机**（与现状 `markdown.rs:21` 一致、零新依赖、能拿行号）；
 否决 `pulldown-cmark`（`markdown` crate 现为 dev-dependency，且 GFM 不含 wikilink，拿行号也麻烦） |
| T5.6 | 单测 `tests/core/markdown.rs` 扩充 ≥ 20 例（含围栏内外、嵌套、`#` 与标题歧义、CJK 标签） |

### T6 — `core/index.rs` 全库索引 — 需设计

设计要点（写进 `docs/note-index-design.md`）：

| 子任务 | 内容 |
|---|---|
| T6.1 | 数据模型：`NoteMeta{path,title,mtime}` + T5 的 `DocFacts`；派生表 `out_links`、`backlinks`、`tags` |
| T6.2 | 构建：`vault.scan()` → `cx.background_spawn()` 后台逐文件解析，**不阻塞 UI**；提供进度与取消 |
| T6.3 | 增量更新：① 笔记保存后重解析单文件 ② `watcher` 事件（`Modified/Created/Removed`）后重解析/剔除 ③ 重命名后清理旧路径条目 |
| T6.4 | 查询 API：`backlinks(path)`、`tags()`、`files(query)`、`resolve_wikilink(target) -> Resolved{Hit,Missing,Ambiguous}` |
| T6.5 | **不落盘**：索引是派生数据，启动后台重建（与"磁盘是唯一真值"一致）；记录 `mtime` 便于增量 |
| T6.6 | 单测 `tests/core/index.rs`：用 `tempfile` 建迷你库，覆盖 链接/反链/标签/重命名/删除/歧义目标 |
| T6.7 | 性能验收：万文件库后台构建目标 < 3s，构建中输入不掉帧 |

---

## 5. 阶段 C–H：功能

### T7 — 命令面板 R10（依赖 T4）— 需设计 + Spike

| 子任务 | 内容 |
|---|---|
| T7.1 | **Spike**：确认 `Command` 的浮层承载方式（dialog / sheet / 自绘 overlay）、焦点获取、`Esc` 与失焦关闭、与现有 `DockArea` 的层级关系 |
| T7.2 | 新建 `src/ui/command.rs`：`CommandState` + `Command::filterable(false)` + `on_query` 用 T4 自排序 + `on_confirm` |
| T7.3 | 数据源：`VaultRegistry` 活动库 `scan()`，文件名 + 路径模糊匹配并显示高亮 |
| T7.4 | 确认后打开：**必须走 `tabs::open_note`**（命中已有标签只激活，不新建） |
| T7.5 | 绑定 `cmd-p`（见 Q4）；无库 / 空库时给出空态提示 |
| T7.6 | 可选扩展：命令项（切换主题、保存、新建笔记、打开设置）与文件项分组 |
| T7.7 | 人工验收：3–4 字符命中、上下键导航、回车打开、`Esc` 关闭、重复打开不产生第二个标签 |

### T8 — 大纲 R9（依赖 T5.2）— 需设计

| 子任务 | 内容 |
|---|---|
| T8.1 | `open_note` 扩展为 `open_note_at(path, line: Option<usize>)`，**仍是唯一入口**（现有调用点传 `None`）；编辑器侧实现"滚动到行 + 光标落行首" |
| T8.2 | 大纲面板：层级缩进展示标题，跟随活动标签刷新 |
| T8.3 | 点击标题 → `open_note_at(当前文件, line)` + 编辑区滚动 |
| T8.4 | 人工验收：切标签即时刷新、长文档滚动定位准确、标题含 CJK 正常 |

### T9 — 双链 R5（依赖 T5.3 + T6）— 需设计 + Spike

| 子任务 | 内容 |
|---|---|
| T9.1 | 目标解析规则：`[[名]]` 按 文件名（不含扩展）→ 相对路径 → 标题 匹配；大小写不敏感；歧义时取"路径最短/最近"并标记 `Ambiguous` |
| T9.2 | 预览渲染：在现有 `sanitize_for_preview` 链路把 `[[x]]` 转为可点链接（保留别名显示文本） |
| T9.3 | 点击 → 走既有 `on_link_click`（`config-wiring` 已接线）→ `tabs::open_note`，**绝不自建打开逻辑** |
| T9.4 | 目标不存在：提示"创建笔记"→ 新建并打开（复用 `Vault::create_note`） |
| T9.5 | **Spike**：编辑区 `[[` 自动补全的可行性（`EditorState` 补全 API）；不可行则降级为"命令面板插入链接" |
| T9.6 | 可选（**本期建议不做**）：重命名笔记时自动改写引用它的链接 |

### T10 — 反向链接 R6（依赖 T6）

| 子任务 | 内容 |
|---|---|
| T10.1 | `backlinks(path)` 返回 `(来源文件, 行号, 上下文片段)` |
| T10.2 | 面板 UI（与大纲同组 tab，见 Q2）；点击跳转到来源行的上下文 |
| T10.3 | 可选：**未链接提及**（正文出现标题文本但未加 `[[ ]]`） |

### T11 — 标签 R7（依赖 T5.4 + T6）

| 子任务 | 内容 |
|---|---|
| T11.1 | 聚合：标签 → 文件列表，含计数与排序（按数量 / 字典序） |
| T11.2 | 标签面板（位置见 Q3）；点击展开结果列表 |
| T11.3 | 结果项 → `open_note_at` 跳转；列表渲染复用 T7 的高亮与导航写法 |
| T11.4 | 单测：嵌套/重复标签去重、frontmatter 与正文标签合并 |

### T12 — 全文搜索 R8（依赖 T4 + T6）— 需设计

| 子任务 | 内容 |
|---|---|
| T12.1 | 检索核心放 `core`：后台并发读文件（不预存正文，见 Q1）、大小写不敏感、命中行号 + 上下文片段、结果上限（如 200）、大文件阈值 |
| T12.2 | 面板 UI：`cmd-shift-f`，输入防抖 200ms，结果按文件分组 |
| T12.3 | 点击结果 → `open_note_at(path, line)` |
| T12.4 | 单测 `tests/core/search.rs`：命中行号、上下文截取、大小写、CJK、二进制/超大文件跳过 |
| T12.5 | 性能验收：万文件库搜索后台完成 < 2s，输入不阻塞 UI |

### T13 — 收尾

| 子任务 | 内容 |
|---|---|
| T13.1 | 全链路联调：编辑保存 → 索引更新 → 各面板刷新；外部改动（watcher）→ 索引更新 |
| T13.2 | 性能与内存：大库索引/搜索/冷启动 |
| T13.3 | 文档：`REQUIREMENTS.md` 勾选 P1 完成项、`DESIGN.md` 补 M6+ 里程碑、README 状态 |
| T13.4 | `cargo build` 无 warning、`cargo test` 全绿、`cargo clippy` 干净 |

---

## 6. 需要你拍板的开放问题

| # | 问题 | 备选 | 建议 |
|---|---|---|---|
| Q1 | 全文检索是否把正文放进常驻索引 | ① 不存，搜索时后台并发读盘 ② 建索引时存正文 | ①（内存可控、实现简单；万文件库冷搜 ~1s 可接受） |
| Q2 | 右侧面板组织（预览 / 大纲 / 反链） | ① 同一 dock 的三个 tab ② 各自独立可拆分面板 | ①（改动小、不破坏现有布局持久化） |
| Q3 | 标签面板位置 | ① 左侧栏与文件树同组 tab ② 右侧 | ① |
| Q4 | 命令面板键位 | `cmd-p` / `cmd-o` 已占（打开文件夹） | `cmd-p`；若与系统冲突再调整 |
| Q5 | 主题切换快捷键 | `cmd-shift-l` / `cmd-shift-d` | `cmd-shift-l` |
| Q6 | 模糊匹配实现 | ① 自研 `core/fuzzy.rs` ② `nucleo-matcher` | ① 零依赖、可调打分；不够好再换 |

## 7. 进度

| 任务 | 状态 | 说明 |
|---|---|---|
| T1 Markdown 快捷键 | ✅ | 5 个格式 Action + `Format::action` 映射；工具栏提示显示键位 |
| T2 主题切换入口 | ✅ | `cmd-shift-l`，`ThemeMode::toggled` 纯函数 + 单测 |
| T3 R13 收尾 | ✅ | 需求表补状态列；新增"错误行列落在文本内"测试 |
| T4 模糊匹配 | ✅ | `core/fuzzy.rs`，12 项单测，暂标 `#[allow(dead_code)]` |
| T7 命令面板 | ✅ | 设计→实施完成，`tests/ui/command.rs` 覆盖候选收集；**待人工验收** |
| T5 Markdown 事实解析 | ✅ | `DocFacts`（标题/链接/双链/标签，带行号）+ 13 项单测；**待下游验收** |
| T6 全库索引 | ✅ | `core/index.rs` + `AppState.notes` + 后台构建 / 保存与监听增量更新，16 项单测 |
| T8 大纲面板 | ✅ | `open_note_at` 跳行 + `OutlinePanel`（右侧与预览同组 tab），4 项单测；**待人工验收** |
| T9 双链 | ✅ | 预览 `[[x]]` 改为可点链接（走 open_note）+ `[[` 补全浮层，7 项单测；**待人工验收** |
| T10–T13 | 待做 | 下一个：T10 反链（索引已就绪，设计先行） |

## 8. 推荐执行顺序

`T1 → T2 → T3 → T4 → T7（命令面板，最早见效）→ T5 → T6 → T8（大纲）→ T9（双链）→ T10（反链）→ T11（标签）→ T12（全文搜索）→ T13`

理由：T7 只依赖 T4 与现有 `vault.scan`，能最早交付一个完整可感的 P1 能力，
同时把"浮层 + 列表 + 高亮 + 跳转"这套 UI 范式打通，后面 T11/T12 直接复用。
T5/T6 是 R5–R8 的公共底座，放在第二个交付波次一次性做完。
