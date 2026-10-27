---
name: notes-app-dev
description: xink 笔记应用（类 Obsidian，GPUI 实现）的项目开发技能。当需求涉及 Vault 目录管理、笔记增删改移动、Markdown 编辑、实时预览、多标签、索引与搜索、外部文件变更同步、工作区持久化时使用。触发关键词：笔记、Vault、文件树、Markdown 预览、双链、标签、索引、自动保存、会话恢复、里程碑。
---

# xink 笔记应用开发

> **流程位置**：流水线阶段 4 的**技术底座**，由 `implement-task` 按需加载
> （编排见 `../rd-orchestrator/SKILL.md`，阶段定义见 `../../rules/00-pipeline.md`）。
> 本技能只回答"笔记应用怎么写"，不负责流程与门禁。

## 项目背景

基于 GPUI 的本地优先 Markdown 笔记桌面应用，对标 Obsidian 基础能力。
**一句话定位**：笔记就是磁盘上普通的 `.md` 文件，应用只负责浏览、编辑、渲染。

- 需求：[`docs/REQUIREMENTS.md`](../../../docs/REQUIREMENTS.md)
- 设计：[`docs/DESIGN.md`](../../../docs/DESIGN.md)
- 铁律：[`.codebuddy/rules/`](../../rules/)

**动手前必读**：`rules/01-gpui-stack.md`（依赖铁律）、`rules/04-notes-domain.md`（领域规则）。

## 不可违背的五条

1. 依赖只用 `gpui-kit`，禁止直接依赖 `gpui 0.2.2`
2. `ropey` 必须 `= "2.0.0-beta.1"`
3. 窗口根元素是 `Root`，且先 `gpui_kit::init(cx)`
4. 分层单向 `ui → core`，`core/` 不含 UI 组件
5. 磁盘文件是唯一真值，索引可重建

## 模块地图

```
src/main.rs          启动入口
src/app.rs           根视图 + 全局初始化
src/core/
  vault.rs           库：根路径、文件枚举、增删改移动
  note.rs            笔记：Rope 内容、dirty、原子保存
  markdown.rs        解析：AST → 预览数据 / 大纲 / 双链
  index.rs           索引：标题、链接、标签 → 搜索
  watcher.rs         notify → 外部变更领域事件
src/ui/
  workspace.rs       DockArea 布局 + 标签栏
  file_tree.rs       目录树（Tree）
  editor.rs          编辑区（Textarea / Editor）
  preview.rs         预览区（TextView）
  command.rs         命令面板 / 搜索
src/state/
  workspace.rs       workspace.json 序列化
```

完整职责与边界见 `references/module-map.md`。

## 开发 SOP（按里程碑推进，不要跳步）

| # | 目标 | 验收 |
|---|---|---|
| **M0** | 环境验证：建 Cargo 项目，跑通 hello world | 窗口弹出，暴露 Xcode/Metal 问题 |
| **M1** | 骨架：DockArea 三栏（树 / 编辑 / 预览） | 三栏可拖拽，布局可序列化 |
| **M2** | 目录管理：打开文件夹、树展示、增删改移 | 树操作与磁盘同步一致 |
| **M3** | 编辑保存：Textarea、防抖保存、原子写、多标签 | 大文件不卡，崩溃不产生半截文件 |
| **M4** | 预览：TextView 实时渲染、滚动同步、主题 | 300ms 内刷新，代码块高亮 |
| **M5** | 持久化：布局 + 标签 + 最近库 | 重启恢复到上次状态 |

**每个里程碑结束都必须 `cargo build` + 手工验收通过，再进入下一个。**

## 关键实现要点

### 编辑 → 预览 / 保存 / 索引
三条链路都走防抖，且时长不同：

```
编辑 → 300ms → TextViewState.set_text()   预览刷新
     → 300ms → atomic_write()  + 更新 disk_mtime
     → 500ms → NoteIndex 增量重建
```

用 `TextViewState::set_text()`（增量，只重算变动块），不要重建 `TextView`，
也不要换元素 id，否则选区和滚动位置会丢、且整篇闪烁。

### 原子写盘（硬性要求）
```rust
let tmp = path.with_extension("md.tmp");
fs::write(&tmp, content)?;
fs::rename(&tmp, path)?;   // 同目录 rename 是原子的
```
写失败必须提示用户，绝不静默丢弃。

### 外部变更处理
`notify` 事件 → 比对 mtime：
- mtime 变了 + `dirty == false` → 自动重载
- mtime 变了 + `dirty == true` → **弹提示让用户选**（重载 / 保留我的修改 / 另存为）
- 绝不静默覆盖未保存的编辑；忽略自己写盘触发的事件

### 索引
- 放 `cx.background_spawn()` 构建，不阻塞 UI
- 只存路径、标题、链接、标签等派生数据，**绝不存正文**
- 可随时删除重建

### 元素 id
统一用**文件路径**做元素 id，保证标签、滚动位置、树展开态稳定。

## 验收清单（每个里程碑自查）

- [ ] `cargo build` 无 warning，`cargo clippy` 干净
- [ ] `core/` 新增逻辑有 `#[test]` 覆盖（用临时目录，不碰真实用户目录）
- [ ] 大文件（10MB 级）操作不卡死
- [ ] 写盘走临时文件 + rename
- [ ] 外部修改笔记后应用行为符合上述策略
- [ ] 忽略 `.git` / `node_modules` / 隐藏文件 / `*.md.tmp`

## 常见误判（避免走弯路）

- **不要一开始就做图谱 / 插件系统** —— 明确在 P2，不在本期
- **不要做 WYSIWYG** —— 本期是源码编辑 + 分屏预览
- **不要引入数据库** —— 索引是派生数据，文件系统即真值
- **不要为了 UI 方便把业务规则写进 `ui/`** —— 那样就没法单测了
