# 文档索引

> `docs/` 共 39 份 Markdown，按领域分组如下。
> **状态以当前 `src/` 代码为准**（核对日期 2026-10-20），不是文档自述。
> 新增文档请按 §7 的命名约定命名，并回到本文件登记一行。

## 状态图例

| 标记 | 含义 |
|---|---|
| ✅ | 已实现：功能在代码中落地，文档仍有效 |
| ⚠️ | 有偏差：主体已落地，个别点未做或描述与代码不符（见备注） |
| ❌ | 不做：已决定放弃，配置项/功能已移除，文档留作知识资产 |
| 📦 | 归档候选：一次性文档（计划 / 评估），完成后参考价值低 |
| 📄 | 非工程文档 |

---

## 1. 总纲（docs/ 根目录）

| 文档 | 说明 | 状态 |
|---|---|---|
| [REQUIREMENTS.md](./REQUIREMENTS.md) | 需求规格：P0（R1–R4）+ P1（R5–R13）+ P2 非目标 | ✅ P0 全落地；P1 仅 **R8 跨库检索 ❌ 不做** |
| [DESIGN.md](./DESIGN.md) | 总体技术设计：选型、分层、状态模型、数据流、里程碑 | ✅ M0–M8 已完成（内容已随代码修订） |

## 2. 文件树 — `docs/file-tree/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [file-tree-interactions-design.md](./file-tree/file-tree-interactions-design.md) | 刷新 / 拖入导入 / 粘贴 / 树内拖拽移动 | `ui/file_tree.rs`、`core/fs.rs` | ✅（1b 拖拽排序按决策不做） |
| [file-tree-incremental-refresh-design.md](./file-tree/file-tree-incremental-refresh-design.md) | 刷新时只更新受影响节点，保展开态/选中/滚动 | `ui/file_tree.rs`、`core/watcher.rs` | ✅ |
| [file-tree-hidden-files-design.md](./file-tree/file-tree-hidden-files-design.md) | 显示隐藏文件与引用其他文件 | `core/vault.rs`、`core/config.rs` | ✅ |
| [file-tree-layout-design.md](./file-tree/file-tree-layout-design.md) | 长文件名下的缩进 / 图标列 / 截断 | `ui/file_tree.rs` | ✅ |
| [file-tree-icons-design.md](./file-tree/file-tree-icons-design.md) | 按文件类型给不同图标 | `core/media.rs`、`ui/file_tree.rs` | ✅（音频/视频已补齐） |
| [file-tree-reveal-active-note-requirements.md](./file-tree/file-tree-reveal-active-note-requirements.md) | 切换活动笔记时目录树自动定位（展开 / 选中 / 滚动）的需求 | `ui/file_tree.rs`、`ui/editor.rs` | 📝 **待实现** |
| [file-tree-reveal-active-note-design.md](./file-tree/file-tree-reveal-active-note-design.md) | 同上：钩子挂在 `EditorPanel::set_active`，复用框架 `TreeState::reveal_item` | `ui/file_tree.rs`、`ui/editor.rs` | 📝 **待实现** |

## 3. 编辑器 — `docs/editor/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [editor-features-analysis.md](./editor/editor-features-analysis.md) | Tab 缩进 / 行号 / 拼写检查三项的可行性分析 | `ui/editor.rs`、`core/config.rs` | ✅ 前两项已实现；**拼写检查 ❌ 不做**（2026-10-20，配置项已移除） |
| [editor-core-migration-eval.md](./editor/editor-core-migration-eval.md) | `Textarea` → `EditorState` 内核升级的 Spike 与决策 | `ui/editor.rs` | 📦 已实施完毕，归档候选 |
| [markdown-editor-toolbar-requirements.md](./editor/markdown-editor-toolbar-requirements.md) | 编辑器顶部 13 个 Markdown 快捷按钮 | `ui/editor.rs` | ✅ |
| [text-format-design.md](./editor/text-format-design.md) | 选中文字的背景色 / 文字色 / 字体 | — | ❌ **功能未实现且文档自述已撤回**，代码中无对应实现 |
| [json-yaml-editing-requirements.md](./editor/json-yaml-editing-requirements.md) | 数据文件编辑需求 R13.1–R13.10 | `core/datafile.rs` | ⚠️ R13.1–R13.9 已交付；**R13.10 视频预览不做** |
| [json-yaml-editing-design.md](./editor/json-yaml-editing-design.md) | JSON / YAML / TOML 编辑与格式化预览的实现设计 | `core/datafile.rs`、`ui/editor.rs` | ✅ |
| [tab-close-button-design.md](./editor/tab-close-button-design.md) | 标签关闭按钮的样式收敛 | `ui/editor.rs` | ✅（第三轮取值） |
| [editor-syntax-highlight-eval.md](./editor/editor-syntax-highlight-eval.md) | 编辑区语法高亮的调研与实施（markdown / json / yaml / toml） | `ui/editor.rs`、`core/media.rs` | ⚠️ **已实施，待人工验收**；开放问题见该文档 §6 |
| [unified-highlight-config-design.md](./editor/unified-highlight-config-design.md) | 统一编辑区与预览区的高亮配置（单一配色源 + 两端适配） | `core/theme.rs`、`core/highlight.rs`、`ui/theme.rs`、`ui/preview.rs` | ⚠️ **M1–M4 已实施完毕，待人工验收**（3 套预置主题：xink / ocean / solarized，2026-10-20） |
| [editor-scroll-wheel.md](./editor/editor-scroll-wheel.md) | 编辑区鼠标滚轮 / 触控板滚动异常的排查与修复（同步写入与手势竞争） | `ui/editor.rs`、`ui/preview.rs`、`core/sync.rs` | ⚠️ **已实施，待人工验收**（2026-10-20）；含分诊步骤与验收清单 |

## 4. 索引与导航（P1）— `docs/navigation/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [markdown-facts-design.md](./navigation/markdown-facts-design.md) | `parse_facts`：一次扫描提取标题/链接/双链/标签（带行号） | `core/markdown.rs` | ✅ |
| [note-index-design.md](./navigation/note-index-design.md) | 全库索引 `NoteIndex`：双链图 / 标签表 / 增量更新 | `core/index.rs` | ✅ |
| [index-derived-tables-design.md](./navigation/index-derived-tables-design.md) | 索引派生表：反向链接 / 标签 / 标题解析从"查询时全表反查"改为增量维护（修订前文档 §9 开放问题 2） | `core/index.rs` | ✅ 已实施，`tags()` 345ms → 0.22µs；实施中结构有简化，见该文档 §3.2 方案 D 与 §11 |
| [wikilink-design.md](./navigation/wikilink-design.md) | `[[双链]]` 预览可点跳转 + 输入补全 | `core/markdown.rs`、`ui/completion.rs` | ✅ |
| [command-palette-design.md](./navigation/command-palette-design.md) | `Cmd+P` 文件名模糊搜索面板 | `ui/command.rs`、`core/fuzzy.rs` | ✅（跨库搜索 ❌ 不做） |
| [outline-design.md](./navigation/outline-design.md) | 大纲面板与点击跳转行 | `ui/outline.rs` | ✅ |
| [index-panels-design.md](./navigation/index-panels-design.md) | 反链面板与标签面板 | `ui/backlink.rs`、`ui/tags.rs` | ⚠️ 功能已实现，但**承载位置已从 dock tab 改为右侧 rail 抽屉**，文档未更新（见 §6） |
| [P1-PLAN.md](./P1-PLAN.md) | P1 的任务拆解与 T1–T13 进度表 | — | 📦 全部 ✅ 完成，归档候选 |

## 5. 预览与界面 — `docs/preview/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [preview-layout-design.md](./preview/preview-layout-design.md) | 预览排版与布局密度三档 | `ui/preview.rs`、`core/config.rs` | ✅ |
| [image-preview.md](./preview/image-preview.md) | 本地图片显示（data URL 化）与点击预览 | `core/markdown.rs`、`ui/preview.rs` | ✅ |
| [right-rail-drawer-design.md](./preview/right-rail-drawer-design.md) | 最右侧按钮条 + 半透明抽屉（大纲/反链/标签） | `ui/rail.rs`、`ui/drawer.rs` | ✅ |
| [scroll-sync-design.md](./preview/scroll-sync-design.md) | 编辑区 ↔ 预览区滚动同步重构（源行锚点取代百分比） | `ui/editor.rs`、`ui/preview.rs`、`core/sync.rs` | ⚠️ **已实施完毕，待人工验收**（2026-10-20）；含 7 条根因、M1–M5 与验收清单 |
| [preview-scroll-jitter.md](./preview/preview-scroll-jitter.md) | 预览区滚动抖动的排查与修复（同步时机可切换：默认实时跟随 / 停止后对齐） | `ui/preview.rs`、`ui/editor.rs`、`core/config.rs`、`ui/settings.rs` | ⚠️ **已实施，待人工验收**（2026-10-21）；含与"高亮版本"的 git 对比结论 |

## 6. 配置 — `docs/config/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [config-management-requirements.md](./config/config-management-requirements.md) | 配置需求 C1–C8 | `core/config.rs` | ✅ |
| [config-management-design.md](./config/config-management-design.md) | 配置的数据模型 / 存储 / 生效机制 | `core/config.rs`、`state/workspace.rs` | ✅ |
| [config-wiring-design.md](./config/config-wiring-design.md) | "死配置"审查与逐项接线方案 | `core/config.rs`、`ui/settings.rs` | ⚠️ 见 §9：`language` / `color_scheme` / `keymap` 三项仍是死配置 |
| [settings-ui-design.md](./config/settings-ui-design.md) | 设置窗口的交互与视觉设计 | `ui/settings.rs` | ✅（D5 搜索框曾只回显不可用，2026-10-20 已修复） |

## 7. 多库 — `docs/vault/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [multi-vault-design.md](./vault/multi-vault-design.md) | 多 Vault 注册 / 切换 / 启动校验 / 持久化 | `core/vault.rs`、`state/workspace.rs`、`ui/vault_manager.rs` | ✅（跨库搜索与跨库双链 ❌ 不做） |

## 8. AI 协助 — `docs/ai/`

| 文档 | 说明 | 对应模块 | 状态 |
|---|---|---|---|
| [ai-capabilities-eval.md](./ai/ai-capabilities-eval.md) | 技术调研：网络栈现状、Provider 选型（OpenAI 兼容 HTTP / CLI Agent）、依赖与 Skills 规范、分期 | `core/ai/`（待建）、`ui/settings.rs` | 📝 **调研完成，待确认**（2026-10-26，分支 `feat/ai-assist-research`） |
| [ai-assistant-requirements.md](./ai/ai-assistant-requirements.md) | 需求 A1–A9：单篇动作、库级管理、Skills、上下文与写入安全、设置与入口 | `core/ai/`（待建）、`ui/ai/`（待建） | 📝 **待确认**，确认后出 `-design.md` |

## 9. 规范与其它（docs/ 根目录）

| 文档 | 说明 | 状态 |
|---|---|---|
| [TESTS.md](./TESTS.md) | 测试规范：`tests/` 镜像 `src/`，用 `#[path]` 挂载 | ✅ 现状符合，规范仍有效 |
| [wechat-promo.md](./wechat-promo.md) | 公众号宣传文案 | 📄 非工程文档，不含需求或规范 |
| `imgs/image.png` | 被 [README.md](../README.md) 引用的截图 | 保留 |

---

## 10. 已知缺口（文档说要做、代码还没做）

- **编辑区 Markdown 高亮待人工验收**：feature 与代码已就绪（`editor-syntax-highlight-eval.md`），
  但着色效果、代码块 injection、明暗跟随都只能肉眼确认，尚未验收。
- **编辑区与预览区的高亮已合并为一套配色源**：`highlight.theme` → 预置主题 `xink`
  （设置界面「基本 · 外观 → 高亮主题」），编辑区注入 `highlight_theme`、预览区用它
  构造 syntect `Theme`，**两端同源**。仍有两处待人工验收确认：
  ① 映射表只对常见语言实测过（Rust `let` 因 scope 共用会与编辑区不同色，见设计文档 §9）；
  ② `ocean` / `solarized` 两套由 syntect 主题转换，明暗差异只在少数条目上。

| 缺口 | 出处 | 说明 |
|---|---|---|
| `language` / `color_scheme` / `keymap` 三项死配置仍在模型里 | `config-wiring-design.md` | `keymap` 连设置界面入口都没有，`main.rs` 键位全部硬编码。文档结论是"要么接线、要么移除"，两者都未执行 |
| "覆盖配置 → UI 行为改变"的测试仍为 0 | `config-wiring-design.md` §1 | `ui/settings.rs` 无 `#[path]` 测试挂载，防退化机制缺失 |
| 反链/标签面板的位置描述过时 | `index-panels-design.md` | 实际由 `ui/rail.rs` + `ui/drawer.rs` 承载 |
| 部分 `ui/` 模块无对应测试 | `TESTS.md` | `tabs` / `preview` / `settings` / `backlink` / `tags` / `search` / `drawer` / `rail` / `completion` / `vault_manager` |

## 11. 待判断：清理与归档候选

上次全量梳理（2026-10-20）中挑出、尚未处理的部分：

| 文档 | 建议 | 理由 |
|---|---|---|
| `text-format-design.md` | **删除** | 功能已撤回、代码中零实现，留着会误导 |
| `P1-PLAN.md` | 归档到 `docs/archive/` | P1 已全部完成，进度表无参考价值 |
| `editor-core-migration-eval.md` | 归档到 `docs/archive/` | 迁移已完成，属一次性决策记录 |
| `wechat-promo.md` | 移出 `docs/` | 宣传文案，与工程文档混放 |

## 12. 命名约定

- 功能设计：`<功能>-design.md`（如 `wikilink-design.md`）
- 功能需求：`<功能>-requirements.md`（如 `json-yaml-editing-requirements.md`）
- 调研/评估：`<主题>-eval.md`、`<主题>-analysis.md`
- 新增文档后：① 在本文件对应分组登记一行；② 标注状态与对应模块；③ 若对应代码尚未实现，状态写 ⚠️ 并进 §9。

## 13. 未立项的方向（不在 `docs/`，仅备忘）

- **PDF 预览**：`REQUIREMENTS.md` 列为 P2 非目标，尚未立项。已完成的调研结论（后端候选 `hayro` / `stet-pdf-reader` 均为纯 Rust + MIT/Apache；gpui 绘制 API 表达力不足以做矢量直绘）**未落成文档**，需要时从对话记录恢复或重新调研。

---

## 14. 目录结构与归属规则

文档按功能分文件夹存放，根目录只放跨功能文档：

```
docs/
├── INDEX.md          总索引（跨功能，留根）
├── REQUIREMENTS.md   需求规格（跨功能，留根）
├── DESIGN.md         技术设计（跨功能，留根）
├── TESTS.md          测试规范（跨功能，留根）
├── P1-PLAN.md        P1 计划（跨功能，留根）
├── wechat-promo.md   非工程文档（留根）
├── imgs/             被 README 引用的截图
├── file-tree/        文件树     5 份
├── editor/           编辑器     9 份
├── navigation/       索引与导航 6 份
├── preview/          预览与界面 3 份
├── config/           配置       4 份
├── ai/               AI 协助    2 份
└── vault/            多库       1 份
```

**归属规则**

1. **主功能优先**：按文档主要描述的功能归档，与本文档的分组一致。
2. **需求与设计同栖**：`<功能>-requirements.md` 与 `<功能>-design.md` 进同一文件夹。
3. **跨功能文档留根目录**：总纲、规范、总索引、跨功能计划不属于任何单一功能。
4. **不复制、只引用**：涉及多个功能的文档只在其主功能文件夹存**一份**，
   关联关系通过本文档的「对应模块」列表达，避免多份副本内容漂移。
5. **跨文件夹引用用相对路径**：同目录 `./x.md`，跨目录 `../<功能目录>/x.md`。

**多归属文档的归属说明**

| 文档 | 涉及功能 | 归入 | 理由 |
|---|---|---|---|
| `index-panels-design.md` | 导航 + 右侧抽屉 | `navigation/` | 主体是反链 / 标签面板，抽屉只是承载容器 |
| `right-rail-drawer-design.md` | 界面 + 大纲/反链/标签 | `preview/` | 主体是右侧栏与抽屉这一界面形态 |
| `editor-features-analysis.md` | 编辑器 + 配置接线 | `editor/` | 主体是编辑器能力的可行性分析 |
| `config-wiring-design.md` | 配置 + 编辑器/预览消费点 | `config/` | 主体是配置项的接线审查 |
| `note-index-design.md` | 索引 + 搜索/双链 | `navigation/` | 主体是索引这个公共底座 |
| `json-yaml-editing-*.md` | 数据文件编辑 + 预览 | `editor/` | 主体是编辑与格式化行为 |
| `image-preview.md` | 预览 + Markdown 图片处理 | `preview/` | 主体是预览呈现 |
