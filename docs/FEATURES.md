# xink 已实现功能清单

> **性质**：本文件是「代码里真实存在、且有入口可触达」的功能点清单，不是需求清单。
> **核对方式**：逐模块读 `src/` 实现并定位入口函数，未落地的规划不写进来（见 §12）。
> **基准**：提交 `2b458b0`，核对日期 2026-10-31，代码规模约 26.8k 行（`src/`），662 个单元测试（51 个测试文件）。
>
> 列「实现位置」的写法是 `文件:行` 的入口函数/类型；同一功能只标最关键的那一处。

---

## 1. 库与文件树

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| 打开文件夹作为库 | `ui/file_tree.rs:152` `open_folder` | 库根就是普通目录，可被 Obsidian 直接打开 |
| 多库注册表 | `core/vault.rs:190` `VaultEntry` / `ui/vault_manager.rs:295` | 新增、切换、改名、重新定位（id 不变）、删除（只移除记录） |
| 库状态徽标 | `core/vault.rs:200` `VaultStatus` | 当前库 / 失效 / 是文件 / 无权限 / 重复 / 嵌套 |
| 目录树扫描 | `core/vault.rs:41` `scan` | 目录在前、同层按名称排序；全量扫描（不做懒加载） |
| 忽略规则 | `core/vault.rs:112` `should_ignore` | `.git` / `node_modules` / `target` / `.DS_Store` / `*.md.tmp` 恒跳；点开头项由「显示全部」决定 |
| 文件类型图标 | `core/media.rs:91` `file_kind`、`ui/file_tree.rs:874` | 目录 / 笔记 / 图片 / 代码 / 数据文件 / 压缩包 / 表格 / 音频 / 视频 / 其他 |
| 新建笔记 / 目录 | `ui/file_tree.rs:179` / `:188` | 自动补 `.md`，重名按 `unique_name` 递增不覆盖 |
| 重命名 | `ui/file_tree.rs:292`、`core/fs.rs:63` `rename_name` | 只有 Markdown 补 `.md`；`.json` 改名不会变成 `.json.md` |
| 删除 | `ui/file_tree.rs:235`、`core/vault.rs:61` | 含非空目录；库根与库外路径禁止 |
| 移动到…（对话框） | `ui/file_tree.rs:390` | 复用 `core/fs.rs:115` `move_path` |
| 树内拖拽移动 | `ui/file_tree.rs:734` `drop_move` | 带文件名浮层、落点高亮；禁止移入自身/子目录；跨设备走复制+校验+删源 |
| 外部导入（访达拖入 / 复制模式） | `ui/file_tree.rs:621` `import_external`、`core/fs.rs:160` | 逐个返回目标路径或失败原因，单文件失败不影响其余 |
| 粘贴（访达文件 / 截图） | `ui/file_tree.rs:665` `paste_into` | 截图写成 `pasted-<时间戳>.<ext>` |
| 右键菜单 | `ui/file_tree.rs:1029` `render` 内 | 重命名、删除、在此新建、移动到…、复制相对路径、复制为 Markdown 链接、插入到当前笔记、粘贴到此目录 |
| 目录树自动刷新 | `core/watcher.rs:35` `classify_kind`、`ui/file_tree.rs:484` | 区分 创建/删除/修改/重命名；仅结构变化才重扫 |
| 保状态合并刷新 | `ui/file_tree.rs:983` `build_tree` | 刷新后保留展开态、选中项、滚动位置 |
| 切换笔记后目录树定位 | `ui/file_tree.rs:927` `reveal_note` | 展开 + 选中 + 滚动到当前笔记 |
| 库选择器（树底部） | `ui/file_tree.rs:1587` `render_vault_switcher` | 切换当前库；库管理窗口的入口也在这里 |

## 2. 笔记编辑

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| 源码模式编辑（Rope） | `core/note.rs:96` `sync_from_rope` | 只替换变化区间，不物化整篇文本 |
| 自动保存（防抖） | `ui/editor.rs:1398` `schedule_save` | 默认 300ms，可在 200–60000ms 配置；新任务覆盖旧任务 |
| 立即保存 | `ui/editor.rs`（`Save` action） | 绕过防抖立刻写盘 |
| 原子写盘 | `core/note.rs:118` `save` | 同目录 `.tmp` + `rename`，失败清理临时文件 |
| 脏标记 | `core/note.rs:53` `is_dirty` | 标签标题显示 `●` |
| 外部修改检测 | `core/note.rs:144` `take_external_text` | 内容一致只更新 mtime（避免光标被重置）；不一致才回填 |
| 放弃改动重新加载 | `core/note.rs:162` `reload` | 有未保存改动时保留用户内容并提示，绝不静默覆盖 |
| 标签唯一（同文件只有一个标签） | `ui/tabs.rs:181` `open_note` | 打开笔记的唯一入口；重复打开只激活 |
| 标签持久化与恢复 | `ui/tabs.rs:283` `persist_open_notes`、`app.rs:641` | 恢复顺序 + 活动标签；失效路径剔除 |
| 空白占位页 | `app.rs:129` `take_empty_panel` | 关掉最后一个标签不 empties 编辑区 |
| 缩进配置 | `core/config.rs:127` `IndentConfig` | Tab / 空格，宽度 1–16 |
| 软换行开关 | `ui/editor.rs:623` | 关掉时滚动同步更准 |
| 行号开关 | `ui/editor.rs:311` | |
| 格式工具栏（13 个按钮） | `ui/editor.rs:3045` `Format` | 加粗/斜体/删除线/标题/无序/有序/任务/引用/代码块/链接/图片/表格/分割线；有选区包裹，无选区插占位 |
| 格式快捷键 | `src/actions.rs`、`ui/editor.rs:3136` | `Cmd+B` / `Cmd+I` / `Cmd+Shift+X` / `Cmd+E` / `Cmd+K` |
| 编辑区语法高亮 | `core/media.rs:135` `editor_language` | markdown / json / yaml / toml；未启用语言退化纯文本 |
| 数据文件校验标记 | `ui/editor.rs:485` `sync_diagnostics` | 编辑区打波浪线（详见 §4） |

## 3. 预览

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| Markdown 渲染 | `ui/preview.rs:630` `render` | 标题、粗斜体、删除线、列表、任务列表、引用、表格、代码块、分隔线、链接、图片 |
| 本地图片内联 | `core/markdown.rs:209` `resolve_image_urls` | 相对/绝对/`file://` 转 data URL（桌面端 `NullHttpClient` 加载不出本地图） |
| 图片缓存 | `core/markdown.rs:143` `image_data_url` | 按 路径 + mtime + 大小 作指纹；单图上限 5MB |
| 图片显示方式 / 宽度 | `core/markdown.rs:174` `ImageOptions` | 适应宽度 20–100% 可调，或隐藏退化为链接 |
| 图片预览模式 | `app.rs:68` `show_preview_image`、`ui/preview.rs:124` | 点击图片在**同一预览面板**打开，切换/编辑笔记自动退出 |
| 代码块语法高亮 | `core/highlight.rs:158` `highlighter` | syntect 默认语法全集；语言/扩展名查不到回退纯文本；结果带缓存（256 条） |
| 高亮主题 | `core/highlight.rs:181` `syntect_theme`、`core/theme.rs:25` | 预置主题 xink / ocean / solarized；可跟随明暗 |
| 滚动同步（双向） | `ui/editor.rs:774` `tick_scroll_sync`、`ui/preview.rs:346` | 顶层块锚点 + 回显吸收防死循环；可关；图片模式不参与 |
| 版面密度 / 最大宽度 | `core/config.rs:236` `PreviewDensity`、`ui/preview.rs:806` | 紧凑 / 适中 / 宽松 + 单独最大宽度 |
| 链接点击行为 | `ui/preview.rs:846` `handle_link_click` | 应用内打开（尝试作为笔记）或系统浏览器，可配 |
| 防御性预处理 | `core/markdown.rs:47` `sanitize_for_preview` | 连续分隔线间补空行，规避底层解析器 panic 导致闪退 |
| 向左伸展（预览铺满） | `ui/preview.rs:586` `extend_toggle_button` | 遮住编辑区与目录树；状态不持久化 |

## 4. 数据文件（JSON / YAML / TOML）

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| 可打开类型 | `core/datafile.rs:125` `data_kind` | `.json` / `.yaml` / `.yml` / `.toml`（只对最后一个扩展名生效） |
| 编辑区高亮 | `core/media.rs:135` | 复用 `DataKind::lang`，与预览围栏标签同源 |
| 预览格式化结果 | `ui/editor.rs:567` `preview_source`、`core/datafile.rs:145` `parse` | 预览显示 pretty-print；**格式化只进预览，绝不改写磁盘文件** |
| 格式校验（行 / 列 + 原因） | `core/datafile.rs:75` `ValidationError` | 编辑区波浪线 + 预览顶部红字；不阻断保存 |
| JSON 缩进配置 | `core/config.rs:178` | 1–8 空格 |
| 大文件保护 | `core/config.rs:181` `max_format_bytes` | 默认 1MB，超限跳过格式化直接显示原文 |

## 5. 导航、索引与结构

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| 全库索引（标题/标签/双链/反链） | `core/index.rs:124` `build` | 后台线程构建；单文件失败只跳过 |
| 单篇重解析 / 摘除 | `core/index.rs:157` `reindex_file` / `:174` `remove_file` | 保存、新增、改名、删除都增量更新 |
| 双链解析 | `core/index.rs:225` `resolve_wikilink` | 按路径 → 文件名 → 标题三级匹配；多候选/未创建都能表达 |
| 预览里双链可点击 | `core/markdown.rs:702` `rewrite_wikilinks`（调用点 `ui/editor.rs:583`） | 解析成目标笔记链接；解析不到时原文原样保留 |
| `[[` 补全 | `ui/completion.rs:157` `open_wikilink_completion` | 自建（框架无触发链路）；候选排序用 `core::fuzzy` |
| 反向链接面板 | `ui/backlink.rs:57`、`core/index.rs:179` | 未解析双链按目标文本分桶，先写后建也能补上 |
| 标签面板 | `ui/tags.rs:109`、`core/index.rs:202` | 聚合 `#tag` 与计数；支持嵌套/frontmatter 标签（`core/markdown.rs:673`） |
| 大纲面板 | `ui/outline.rs:75`、`core/markdown.rs:399` `parse_facts` | 数据源是编辑器实时解析，不读索引；点击跳到标题所在行 |
| 右侧按钮条 | `ui/rail.rs:59` | 浮层不占布局宽度，展开时仍可点 |
| 右侧抽屉 | `ui/drawer.rs:100` `open` | 承载大纲 / 反链 / 标签；点遮罩或 `Esc` 收起 |
| 命令面板 `Cmd+P` | `ui/command.rs:393` | 文件名模糊搜索；命中已打开只激活；候选来自磁盘扫描，不落盘 |
| 全文搜索 `Cmd+Shift+F` | `ui/search.rs:172`、`core/search.rs:30` | 带行号与上下文，点击跳到命中行；后台检索、输入防抖 200ms |
| 模糊匹配与打分 | `core/fuzzy.rs:24` | 按字符匹配（中文/emoji 安全），返回命中下标供高亮 |
| 跳行打开 | `ui/tabs.rs:189` `open_note_at` | 搜索 / 大纲 / 反链 / 标签统一走它，不产生第二个标签 |
| 明暗主题切换 | `app.rs:695` `toggle_theme` | 跟随系统时按一次切到相反外观 |

## 6. AI 协助（可选，默认关闭）

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| 对话条 | `ui/ai.rs:467`、`Cmd+Shift+A` | 气泡布局，会话**按笔记落盘**（`ai_sessions.json`），换笔记自动切换、重启仍在 |
| 会话持久化 | `state/ai_sessions.rs:71` `save_note` | 分块存储（正文/工具调用/校验/diff），折叠状态可复现 |
| 快捷动作（内置 6 项 + 自定义） | `core/config.rs:729` `builtin_actions`、`ui/ai.rs:660` `run_action` | 润色/精简/翻译/改标题/摘要/标签；可增删改、停用；命令面板 `AI:` 前缀唤起 |
| 提示词变量 | `core/config.rs`（`{{known_tags}}`） | 注入库内既有标签 |
| 结果落法 | `core/config.rs:681` `AiActionApply` | 插入光标处 / 替换原文（需二次确认）/ 不采用 |
| 修订评审（逐块接受/拒绝） | `ui/editor.rs:1746` `start_review`、`core/ai/patch.rs:211` `review_layout` | AI 不直接改文件；按修改块决定，确认后才落盘 |
| 改前快照 | `core/ai/snapshot.rs:30` `write_snapshot` | 存 `ai-history/`，上限 200 份，超出删最旧 |
| Provider：Ollama | `core/ai/ollama.rs:22` | native `/api/chat`，NDJSON 流 |
| Provider：OpenAI 兼容 | `core/ai/openai.rs:30` | SSE 流；`base_url` 整体可配 |
| 流式解析 | `core/ai/sse.rs` / `core/ai/ndjson.rs` | 分帧、粘包、尾部 flush |
| 重试与备用模型 | `core/ai/mod.rs:298` `stream_with_failover` | 已吐过内容的请求不重发；可重试次数 1–5、退避可配 |
| 首字节超时 / 请求超时 | `core/config.rs:811` / `:837` | |
| 场景路由 | `core/config.rs:596` `AiRouteConfig` | 对话 / 快捷动作 / Skill / Agent 各指定模型；动作可再单独指定 |
| 上下文按模型裁剪 | `core/ai/context.rs:50` `assemble`、`core/config.rs:634` `context_limit` | 取全局上限与模型预算的较小值 |
| 密钥管理 | `core/ai/keychain.rs:100` `resolve_api_key` | macOS Keychain（界面可写入/清除）/ 环境变量 `XINK_AI_API_KEY` / 明文（显式选择） |
| 连接测试（探针） | `core/ai/probe.rs:26` `probe_model`、`core/ai/ollama.rs:61` | 测连通性、工具调用、耗时 |
| 从 Ollama 拉模型 | `core/ai/ollama.rs:34` `list_models`、`ui/settings.rs:2650` | 结果进模型缓存（`core/ai/models.rs:80`）并合并进模型列表 |
| 模型能力字典与推断 | `core/ai/capability.rs:41` `info` / `:87` `infer` | Ollama 有 `capabilities`，OpenAI 兼容端点无来源 → 给出默认猜测 |
| 能力 chip 行 + 手动编辑 | `ui/settings.rs:1556` `capability_row` | 悬停看说明；用户勾过的项不被推断覆盖（`capabilities_pinned`） |
| 逐模型参数档位 | `ui/settings.rs:1763` `model_param_control` | `num_ctx` / `num_predict` / `temperature` / `think` / `keep_alive` |
| Agent 工具调用 | `core/ai/tools.rs:65` `Tool`（7 个） | read_note / search_notes / list_tags / list_backlinks / outline / list_notes / read_skill_resource |
| Agent 循环 | `core/ai/agent_loop.rs:89` `run` | 多步调用 + 校验回灌 |
| 结果校验 | `core/ai/verify.rs:60` `check` | 双链目标、frontmatter、标签类型等问题的反馈 |
| Skills（`SKILL.md`） | `core/ai/skills.rs:73` `discover` | 库内 `.xink/skills` + 用户目录两级发现；可整体开关、逐项停用 |
| 库级批量任务（计划面板） | `ui/ai/plan.rs:69`、`core/ai/tasks.rs` | 改标签 / 加标签 / 删标签；批量执行、可中断恢复 |
| 生成预算与截断可见 | `core/ai/mod.rs:141` `Usage`、`ui/ai.rs` 状态行 | 协议层识别预算耗尽，状态行区分两类截断 |

## 7. 界面与外观

| 功能点 | 实现位置 | 说明 |
|---|---|---|
| 三栏布局（DockArea） | `app.rs:494` `Workspace` | 可拖拽、可停靠；默认左 260 / 右 420 |
| 布局持久化 | `app.rs:610` `save_layout`、`state/workspace.rs:37` | `layout.json` 带版本号，结构变更自动重建 |
| 主题（跟随系统 / 浅 / 深） | `app.rs:323` `apply_theme`、`core/theme.rs:40` | 6 套预置主题 JSON（3 主题 × 明暗） |
| 全局视觉规范 | `ui/tokens.rs` | 间距 / 圆角 / 语义色统一，取自 `cx.theme()` |
| 目录树 / 编辑区 / 预览区字体 | `core/config.rs:116` `FontConfig` | 族 / 字号 / 行高 / 字重分别可设 |
| 目录树图标尺寸 | `core/config.rs:142` | 12–48px |

## 8. 设置

独立设置窗口（单例，`ui/settings.rs:3868`），改动**即时生效并落盘**。

| 分类页 | 主要分组 | 实现位置 |
|---|---|---|
| 基本 | 外观（主题）、目录树（图标大小 / 字体 / 显示全部文件） | `ui/settings.rs:411` |
| 编辑器 | 编辑行为、字体与排版、数据文件 | `ui/settings.rs:550` |
| 预览 | 渲染（高亮主题 / 图片 / 链接打开方式）、排版、同步 | `ui/settings.rs:727` |
| AI | 连接、生成、模型与能力、模型密钥、场景路由、快捷动作、快照、Skills | `ui/settings.rs:935` 起 |
| 快捷键 | 每个动作一条，可覆盖 | `ui/settings.rs:1161` `keymap_fields` |

附加能力：设置项搜索（回车跳首个命中）、单项重置为默认、配置导入 / 导出 / 恢复全部默认（`core/config.rs:1318` / `:1327`）、
对已配置但未生效的项标注「⚠ 暂未生效」。

## 9. 持久化与容错

应用数据目录由 `dirs::data_dir()` 决定（macOS：`~/Library/Application Support/xink/`）。

| 文件 | 内容 | 实现位置 |
|---|---|---|
| `config.json` | 用户配置 | `state/workspace.rs:33` |
| `layout.json` | Dock 布局 | `state/workspace.rs:24` |
| `open_notes.json` | 标签顺序 + 活动标签 | `state/workspace.rs:155` |
| `vaults.json` | 多库注册表与当前选择 | `state/workspace.rs:56` |
| `ai_sessions.json` | AI 会话（按笔记分组） | `state/ai_sessions.rs:56` |
| `ai-history/` | 改动前快照（上限 200） | `core/ai/snapshot.rs` |
| `xink.pid` | 单实例锁 | `state/single_instance.rs` |

容错：笔记正文始终在库目录的 `.md` 里；配置文件缺失或 JSON 损坏一律回落默认值（`core/config.rs:1296`），
旧配置缺字段自动补齐，已移除的枚举值有兜底（`core/theme.rs:53`）。

## 10. 快捷键（默认）

| 快捷键 | 作用 | 可覆盖 |
|---|---|---|
| `Cmd+O` | 打开文件夹作为库 | ✅ |
| `Cmd+N` | 新建笔记 | ✅ |
| `Cmd+S` | 立即保存 | ✅ |
| `Ctrl+W` | 关闭当前标签 | ✅ |
| `F2` | 重命名（焦点在文件树） | ✅ |
| `Cmd+P` | 命令面板 | ✅ |
| `Cmd+Shift+F` | 全文搜索 | ✅ |
| `Cmd+Shift+A` | AI 对话条 | ✅ |
| `Cmd+Shift+L` | 明暗主题切换 | ✅ |
| `Cmd+,` | 设置 | ✅ |
| `Cmd+B` / `Cmd+I` / `Cmd+Shift+X` / `Cmd+E` / `Cmd+K` | 加粗 / 斜体 / 删除线 / 行内代码 / 链接 | ✅ |

键位表在 `core/keymap.rs:35`（纯数据，可单测），覆盖值非法即回落默认、冲突保留靠前的一项；**改动重启后生效**。
「新建文件夹」「删除」默认不绑键位。

## 11. 工程与质量

- 分层铁律：`ui → core` 单向，`core/` 不出现任何 UI 类型（AI 领域层同样如此）。
- 单元测试 662 个 / 51 个文件，物理位置在仓库根 `tests/`，由源文件末尾 `#[path]` 挂载（见 `docs/TESTS.md`）。
- 强制 `rust-embed` 的 `debug-embed`：debug 产物的 `.app` 拷到别的机器不会丢图标。
- 打包脚本 `scripts/build_macos.sh`：编译 → `.icns` → `.app` → ad-hoc 签名 → `.dmg` / `.pkg`，并校验图标确实内嵌。

## 12. 未实现 / 决定不做

| 项 | 状态 |
|---|---|
| 跨库检索 | 决定不做：搜索与双链只覆盖当前库 |
| 拼写检查 | 决定不做：配置项与设置项均已移除 |
| 界面语言（中/英切换） | 未实现：设置里已无该项，文案仅中文 |
| 编辑区独立配色 | 未实现：跟随主题 |
| 视频播放 | Spike 验证通过但接入失败，代码与依赖已清理（复盘在 `spike/video-playback` 分支） |
| PDF 预览 | 未立项（P2 非目标） |
| 目录树懒加载 | 未做：框架 `Tree` 需要完整树才能渲染展开箭头，当前全量扫描 |

## 13. 与 README 的差异（README 需同步）

核对代码后发现 README 有三处已过时：

1. 设置分类是 **5 个页签**（基本 / 编辑器 / 预览 / AI / 快捷键），README 写「四个分类」并把快捷键归在编辑器下。
2. 「拼写检查」「界面语言」两项在代码里已不存在（配置无字段、设置无入口），README 的设置表仍列着。
3. AI 模型能力清单（字典 / 推断 / chip 行 / 手动编辑 / pinned）——2026-10-29 落地，README 尚未记录。
