# CODEBUDDY.md

本项目是用 **GPUI** 开发的本地优先 Markdown 笔记桌面应用（类 Obsidian），名字为 **xink**。
完整需求见 `docs/REQUIREMENTS.md`，技术设计见 `docs/DESIGN.md`。

## 开工前必读

以下 5 条是本项目的技术铁律，**违反会导致编译失败或架构腐化**：

1. **只依赖 `gpui-kit = "0.6"`，禁止在 `Cargo.toml` 里直接写 `gpui = "0.2.2"`。**
   `gpui-kit` 内部已 pin `gpui-pre 0.3.5`。crates.io 上的 `gpui 0.2.2` 与 `gpui-pre 0.3.5`
   是两条互不相通的线，混用必然编译失败。
2. **`ropey` 必须写 `= "2.0.0-beta.1"`（精确版本）**，与 `gpui-component` 内部对齐，
   否则 cargo 会解析出第二份，导致类型不兼容。
3. **窗口的第一层必须是 `Root::new(view, window, cx)`**，且使用前先调用 `gpui_kit::init(cx)`。
4. **分层依赖单向：`ui → core`。** `core/` 不得引用任何 UI 组件类型，保证领域层可单测。
5. **笔记正文的唯一真值是磁盘上的 `.md` 文件。** 索引、缓存都是可重建的派生数据。
6. **先设计，后开发**：新功能（或影响现有行为的中等以上改动）必须先出设计文档、
   经用户确认后再写代码；确认前只允许调研与 Spike 原型。详见 `.codebuddy/rules/05-design-first.md`。

## 项目结构

```
src/
├── main.rs       启动入口：gpui_kit::application().run()
├── app.rs        根视图 + 全局初始化
├── core/         领域层（vault / note / markdown / index / watcher）—— 不含 UI
├── ui/           视图层（workspace / file_tree / editor / preview / command）
└── state/        持久化（workspace.json）
tests/            单元测试（目录结构镜像 src/，由 src 里的 #[path] 挂载；
                  不是 Cargo 集成测试目录，详见 docs/TESTS.md）
docs/             需求 + 设计文档（34 份，**入口见 docs/INDEX.md**）
.codebuddy/       rules（开发铁律） + skills（开发技能）
```

## 常用命令

```bash
cargo run                 # 运行（首次编译较慢，分钟级）
cargo build --release     # 发布构建
cargo test                # 领域层单元测试
cargo clippy --all-targets
cargo fmt
```

## 关键技术事实（避免重复调研）

| 项 | 值 |
|---|---|
| UI 框架 | `gpui-kit` 0.6.4（内含 `gpui-pre` 0.3.5 = `zed@d89e9c2` 快照） |
| Rust 版本 | 1.98.1，edition 2024 |
| 平台 | macOS，渲染走 Metal（需 Xcode） |
| Markdown 渲染 | `gpui_kit::base::TextView::markdown()` |
| Markdown 编辑 | `Textarea`（多行）/ `Editor`（tree-sitter 高亮） |
| 目录树 | `gpui_component::Tree` |
| 布局 | `gpui_component::DockArea`（可序列化持久化） |
| 文件监听 | `notify` 7 |

## 当前状态

**P0 已全部完成**：
打开文件夹作为库、浏览文件树、新建/重命名/移动/删除；
点击笔记以标签页打开并编辑，防抖 300ms 自动保存、`Cmd+S` 立即保存、原子写盘；
右侧 `TextView` 实时渲染 Markdown（代码块语法高亮跟随明暗主题、编辑区滚动大致同步、
本地相对路径图片自动补全为 file:// URL）；
重启恢复库与打开的标签页（顺序 + 活动标签）；`notify` 监听外部修改（未改动自动重载，有未保存改动则提示）；
最近文件列表。领域层有单元测试覆盖。

编辑区标签的两条硬规则（详见 `docs/DESIGN.md` §2.5）：
**同一文件只有一个标签**（重复打开只激活已有标签，入口唯一：`ui/tabs.rs::open_note`）；
**有历史打开记录就不显示空白页**（`open_notes.json`，恢复走同一个入口）。

新增模块：`core/highlight.rs`（syntect 语法高亮）、`core/markdown.rs`（图片路径解析）、
`core/watcher.rs`（外部变更监听）。

**下一个里程碑：P1** —— 双链 `[[wikilink]]`、反向链接、标签、全文搜索、大纲、命令面板。

已验证：
- `gpui-kit 0.6.4` + `gpui-pre 0.3.5` 编译通过，Metal 渲染跑通（无需完整 Xcode.app）
- `DockArea` 三栏布局 + 布局持久化（`~/Library/Application Support/xink/layout.json`）
- 原子写盘（临时文件 + rename）、外部修改检测均有测试覆盖

P0 已完成，下一步进入 **P1**：双链 `[[wikilink]]`、反向链接、标签、全文搜索、大纲、
命令面板（`Cmd+P`）、明暗主题。开始 P1 前建议先补两个 M4 遗留项：
代码块语法高亮、编辑区与预览区滚动同步。

## 参考资料

- GPUI Kit 文档：<https://gpui-kit.com>
- GPUI Kit 仓库：<https://github.com/longbridge/gpui-kit>
- 官方提供 AI 编码 skill（强烈建议安装）：`npx skills add longbridge/gpui-kit`
- 同类实现参考：<https://github.com/kongweiguang/gmark>（GPL-3.0，仅作功能参考，勿抄代码）
