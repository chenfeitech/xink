# xink — 技术设计文档

> 配套文档：`docs/REQUIREMENTS.md`（需求规格）

## 1. 技术选型（本项目最关键的一个决策）

### 1.1 背景：GPUI 生态已分裂为两条互不相通的线

调研（2026-09）发现，直接使用 `gpui` 会踩到版本陷阱：

| | `gpui` (crates.io) | `gpui-pre` (crates.io) |
|---|---|---|
| 版本 | **0.2.2** | **0.3.5** |
| 发布时间 | 2025-10-22（已停滞一年） | 2026-09-14（`zed@d89e9c2` 快照） |
| 发布方 | Zed Industries 官方 | huacnlee（gpui-kit 作者） |
| 生态配套 | 无 | `gpui-component` 0.6.4 依赖它 |

**关键事实**：活跃的组件库 `gpui-component` 0.6.4 的依赖是 `gpui-pre = "^0.3.5"`，
**不是** `gpui = "0.2.2"`。两者 API 不兼容，**绝不能混用**。

### 1.2 决策：使用 `gpui-kit` 单依赖

若按直觉直接依赖 `gpui = "0.2.2"`，能跑通 hello world，但**没有任何组件库可用**——
文件树、编辑器、Markdown 渲染全部要从 `div()` 手搓，对本项目是数月工作量。

```
gpui-kit 0.6.4            ← 应用只依赖这一个
├── gpui-base             无样式行为层：TextView / Markdown / HTML / 文本选择
└── gpui-component        60+ 成品组件：Tree / Editor / Textarea / DockArea / Input ...
     └── 内部 pin: gpui-pre 0.3.5
```

`gpui-kit` 会 pin 匹配的 GPUI 版本并 re-export，应用只列一个依赖，**从根上杜绝版本错配**。

**代价（已接受）**：
- `gpui-pre` 是 Zed main 分支快照，非语义化版本，升级必须跟随 `gpui-kit` 节奏
- 依赖树重（tree-sitter / blade-graphics / resvg），首次编译在分钟级

### 1.3 可直接复用的现成能力（省掉自研）

| 需求 | 现成能力 | 说明 |
|---|---|---|
| Markdown 预览 | `gpui_base::TextView::markdown()` | 标题/列表/表格/代码块/图片/链接/脚注、可选中复制、虚拟滚动、插件机制 |
| 预览增量更新 | `TextViewState` + `set_text()` | 只重算变动块，不重解析全文档 |
| Markdown 源码编辑 | `Textarea`（多行）/ `Editor`（tree-sitter 高亮） | 官方建议：普通多行用 `Textarea`，源码编辑用 `Editor` |
| 目录树 | `Tree` | 直接复用 |
| 面板布局 | `DockArea` | 拖拽/分屏/嵌套，**且可序列化持久化** |
| 文件变更监听 | `notify 7.0`（组件库已依赖） | 外部修改同步 |
| 图标 | Lucide（默认 `assets` feature） | 开箱即用 |
| Rope 文本结构 | `ropey`（组件库已依赖） | 编辑器底座 |

### 1.4 依赖清单

```toml
[package]
name = "xink"
edition = "2024"          # gpui-kit 要求 edition 2024
rust-version = "1.98"

[dependencies]
gpui-kit = "0.6"

# 以下版本必须与 gpui-component 内部保持一致，否则会出现多版本类型不兼容
ropey = "=2.0.0-beta.1"   # gpui-component 是精确锁定，必须对齐
notify = "7"
markdown = "1"            # 自研 AST 解析（大纲/双链提取）时使用

serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "2"
```

> **铁律**：`ropey` 必须与 `gpui-component` 精确同版本（`=2.0.0-beta.1`）。
> 写成 `"2"` 或 `"2.0.0-beta"` 会让 cargo 解析出第二份，导致类型不匹配、编译失败。

## 2. 架构设计

### 2.1 分层

```
ui/          视图层——只负责渲染与交互，不含业务规则
  ├── workspace.rs   DockArea 布局、标签栏
  ├── tabs.rs        标签注册表 + 打开笔记的唯一入口
  ├── file_tree.rs   目录树
  ├── editor.rs      编辑区
  ├── preview.rs     预览区
  └── command.rs     命令面板 / 搜索
core/        领域层——与 UI 完全解耦，可独立单元测试
  ├── vault.rs       库：根路径、文件枚举、增删改移动
  ├── note.rs        笔记：Rope 内容、脏标记、保存
  ├── session.rs     会话：标签标识归一化、保序去重、失效判定
  ├── markdown.rs    解析：AST → 预览数据 / 大纲 / 双链
  ├── index.rs       索引：标题、链接、标签 → 搜索
  └── watcher.rs     notify 文件监听 → 外部变更事件
state/       持久化：workspace.json、最近库、布局
```

**依赖方向严格单向**：`ui → core → 无`。`core` 不允许 `use` 任何 `gpui_kit` 的 UI 类型，
只允许使用 `gpui` 的 `Entity`/`App`/`Task` 等运行时类型（保证领域层可测试）。

### 2.2 状态模型（GPUI `Entity`）

GPUI 用 `Entity<T>` 持有跨组件共享状态，通过 `cx.notify()` 驱动重绘：

| Entity | 职责 |
|---|---|
| `Entity<Workspace>` | 顶层：当前库、打开的标签、布局状态 |
| `Entity<Vault>` | 当前库：根路径、文件树快照 |
| `Entity<Note>` | 单篇笔记：`Rope` 内容 + `dirty` 标记 + 磁盘 mtime |
| `Entity<NoteIndex>` | 全库索引：路径树 + 双链图 + 标签表，供搜索/补全 |
| `Entity<TextViewState>` | 预览文档状态（gpui-base 提供），`set_text()` 增量更新 |

### 2.3 数据流（单向）

```
用户编辑 → Note(Rope) 变更
              ├─ 防抖 300ms ─→ 原子写盘（tmp + rename）
              ├─ 防抖 300ms ─→ TextViewState.set_text() ─→ 预览刷新
              └─ 防抖 ──────→ NoteIndex 增量重建 ─→ 大纲/搜索/反链更新

notify 外部变更 ─→ Watcher 事件 ─→ 比对 mtime ─→ 提示"文件已被外部修改，重新加载?"
```

### 2.4 数据模型与持久化

```rust
struct Note {
    path: PathBuf,
    rope: Rope,          // 文本内容，避免大文件全量拷贝
    dirty: bool,
    disk_mtime: SystemTime,
}

struct WorkspaceState {           // 按职责拆成多个 JSON 落盘
    vault_root: Option<PathBuf>,  // vault.json
    open_tabs: Vec<PathBuf>,      // open_notes.json：顺序即标签顺序
    active_tab: Option<PathBuf>,  // open_notes.json：上次活动的标签
    layout: Option<String>,       // layout.json：DockArea 序列化结果
}
```

存储位置（macOS）：`~/Library/Application Support/xink/`
- `config.json` — 偏好设置
- `layout.json` — `DockArea` 布局（带 schema 版本）
- `vault.json` — 上次打开的库
- `open_notes.json` — **会话恢复的唯一来源**：已打开标签的顺序 + 活动标签
- `recent.json` — 最近文件（最多 10 条）
- `index/` — 索引缓存，**可随时删除重建**

> `open_notes.json` 只存**路径**，绝不存正文（见 §1 铁律 5）。
> 文件被删除后这条记录自然失效，恢复时剔除即可，无需额外清理机制。

> 索引是**可重建的派生数据**，不是真值。绝不把笔记正文写进索引库。

## 2.5 编辑器标签管理（去重 + 会话恢复）

编辑区的行为由两条规则主导：**同一文件只允许一个标签**，**启动时优先恢复而不是显示空白页**。
二者共用"已打开标签"这一份状态，因此放在一起设计。

### 2.5.1 状态与落点

| 位置 | 内容 |
|---|---|
| `ui/tabs.rs::OpenTabs` | 内存态：`order: Vec<PathBuf>`（标签顺序）+ `panels: HashMap<PathBuf, WeakEntity<EditorPanel>>` + `active: Option<PathBuf>` |
| `ui/tabs.rs::open_note()` | **打开笔记的唯一入口**，所有入口都必须走它 |
| `core/session.rs` | 纯路径规则：标识归一化 `tab_key`、保序去重 `dedup_paths`、失效判定 `partition_existing`、活动标签回落 `resolve_active` |
| `state/workspace.rs` | `open_notes.json` 的读写；**只做序列化 + 路径级过滤，不做业务判断** |

为什么顺序与查找分开存：`HashMap` 的迭代顺序不确定，若直接拿它落盘，
每次保存得到的标签顺序都不同，恢复顺序就会随机漂移（实测会发生）。
`order` 是唯一的顺序来源，`panels` 只做查找，二者由 `OpenTabs` 的方法共同维护。

### 2.5.2 标签去重（同一文件只允许一个实例）

**匹配依据**：标签的唯一标识是**规范化后的绝对路径**

```rust
pub fn tab_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())  // 文件已被删除时回落原路径
}
```

- 用路径而非文件名：不同目录下允许存在同名笔记
- 用 canonicalize 而非原始字符串：软链接、`./a.md` 与 `/abs/a.md` 都指向同一文件，
  必须判为同一标签，否则会出现"两个标签编辑同一个文件，互相覆盖"
- 用 `WeakEntity` 持有面板：面板被关闭后引用自动失效，`len()/keys()` 只统计仍存活的标签，
  无需在关闭路径里手动清理多处状态

**触发条件**：以下任一入口请求打开 `path`，且 `tab_key(path)` 已在 `OpenTabs` 中：

| 入口 | 现状 |
|---|---|
| 文件树单击 `.md` | 已接入 |
| 最近文件列表点击 | 已接入 |
| 命令面板 / 全文搜索 / 双链跳转（P1） | 必须复用同一入口，禁止自建打开逻辑 |
| 启动恢复 | 恢复列表内部也要去重（历史记录可能被外部写坏） |
| 文件监听（外部改动） | 不触发打开，只对已打开标签生效 |

**切换逻辑**（命中已有标签时，按序执行且**不做**别的事）：

1. `dock_area.select_panel(PanelId)` —— 在 dock 层选中该标签
2. `focus_handle.focus()` —— 焦点移入编辑区
3. `OpenTabs.activate(key)` + 落盘 —— 更新活动标签

明确不做：不新建面板、不重新读盘（`Note::load` 会丢弃未保存的编辑）、
不重置光标与滚动位置、不改变标签顺序、不重复登记。

> **实现约束**：`select_panel` 会触发 dock 同步重渲染，标签栏此时会用 `read` 读每个面板的标题。
> 因此切换必须**在 panel 的写租约之外**执行（不能在 `panel.update(cx, |p, cx| p.activate())` 里做），
> 否则报 "cannot read EditorPanel while it is already being updated"。`reveal_panel()` 封装了这一约束。

### 2.5.3 空白页不变式

编辑区是一个 dock，不能被清空，因此用空白页占位：

- 一篇笔记都没打开 → 编辑区**恰好**存在一个空白页
- 只要打开了笔记 → 空白页不存在

`sync_placeholder()` 是维护该不变式的**唯一**函数，所有改变"已打开笔记集合"
的地方（打开、关闭、启动恢复）在其操作末尾调用它，幂等。
新建笔记时优先**认领**空白页（而不是一律新建），避免凭空多出空标签。

### 2.5.4 启动恢复历史打开的文件

**存储方式**（`open_notes.json`）：

```rust
struct OpenNotes {
    paths: Vec<PathBuf>,      // 标签顺序，即恢复顺序
    active: Option<PathBuf>,  // 上次活动的标签
}
```

- `active` 存路径而不是下标：下标会随某条记录失效而错位，路径不会
- `#[serde(default)]`：旧版本只写了 `paths`，新版本仍能读；读不到即视为"无历史记录"
- 保存时按 `OpenTabs.order` 输出并去重，保证顺序稳定、无重复项

**写入时机**（不依赖退出流程，避免崩溃丢失）：

| 时机 | 说明 |
|---|---|
| 打开笔记后 | 立即写 |
| 关闭标签 / 清空为空白页后 | 立即写，否则重启会恢复出已关闭的标签 |
| 切换标签 | 立即写（更新 `active`），用户操作频率低，开销可忽略 |
| 布局变更、应用退出 | 兜底再写一次 |

**恢复时机**：`Workspace::new` 中，位于 **布局恢复之后、`sync_placeholder` 之前**。
顺序不能颠倒：布局恢复会按 `layout.json` 重建出一批空白 `EditorPanel`，
必须先让它们存在，恢复流程才能"认领"；认领完再由 `sync_placeholder` 决定空白页的去留。

```
load_layout → 重建 DockArea（产出一堆空白 EditorPanel）
    ↓
restore_open_notes：逐条 open_note（认领空白页 → 载入 → 登记）
    ↓
activate(active)：激活上次活动的标签
    ↓
sync_placeholder：有笔记则清空白页，无笔记则保留一个
```

**恢复顺序**：严格按 `paths` 顺序逐个恢复；全部恢复完成后**再**激活 `active`，
避免中间态（边建边切会让预览反复重建）。

**异常处理**：

| 情况 | 处理 |
|---|---|
| 文件已被删除 / 路径失效 | 加载前 `is_file()` 过滤掉，**静默跳过**并打印日志；不弹窗打断启动 |
| 存在但读不了（权限、非 UTF-8） | `Note::load` 失败 → 不登记进 `OpenTabs`、归还认领的空白页；该笔记视为未打开 |
| 记录里有重复路径 | 恢复前 `dedup_paths` 保序去重，只恢复一次 |
| `active` 指向的标签恢复失败 | `resolve_active` 回落到"最后一个成功恢复的标签"；全失败则为 None |
| 全部记录都失效 | 等价于没有历史记录 → `sync_placeholder` 展示空白编辑页 |
| JSON 损坏 / 解析失败 | 视为空记录，回落到空白页；**不因历史文件损坏阻止启动** |
| 恢复后记录与实际情况不一致 | 恢复结束立即写回一次，失效条目被自动剔除（自愈） |

> 恢复走的是与手动打开**完全相同**的 `open_note` 入口，
> 因此"恢复"不会绕过去重规则，也不会绕过登记/落盘——这是本设计的关键约束。

## 3. 关键技术方案（难点与对策）

| 难点 | 对策 |
|---|---|
| 编辑时预览实时但不卡顿 | 停止输入 300ms 防抖后解析；用 `TextViewState::set_text()` 增量更新（gpui-base 只重算变动块，不重解析全文档） |
| 大库首次加载慢 | 索引构建放 `cx.background_spawn()` 后台线程；文件树子目录懒加载 |
| 外部修改冲突 | `notify` 监听 + mtime 比对，冲突时弹提示，**绝不静默覆盖** |
| 双链 `[[...]]` 解析 | GFM 不含此语法；用 `markdown` crate 解析后遍历 AST 提取，配合自定义扫描 |
| 重复标签（同一文件两个实例） | 统一入口 `tabs::open_note` + 规范化路径键 `tab_key`，命中即激活，不新建 |
| 恢复顺序随机漂移 | 顺序单独由 `OpenTabs.order` 维护并落盘，不取 `HashMap` 迭代顺序 |
| 切标签时的重入 panic | 只在 dock 层 `select_panel`，绝不在 panel 写租约内切换（封装为 `reveal_panel`） |
| 写盘安全 | 写临时文件再 `rename`，保证原子性，崩溃不产生半截文件 |
| 二进制体积 / 编译时长 | 只启用需要的 `tree-sitter-<lang>` feature，不开全量；配 `.cargo/config.toml` 换 lld |
| CJK 与 IME 输入 | 用 gpui 的 `EntityInputHandler` / 组件库 `Textarea`，不自行处理按键序列 |

## 4. 目录结构

```
rustgpui/
├── CODEBUDDY.md              项目入口 guidance（AI 首先读取）
├── README.md                 项目说明 + 构建前置条件
├── docs/
│   ├── REQUIREMENTS.md       需求规格
│   └── DESIGN.md             本文档
├── .codebuddy/
│   ├── rules/                开发铁律（AI 必须遵守）
│   └── skills/               项目开发技能
├── Cargo.toml
└── src/
    ├── main.rs               启动入口
    ├── app.rs                根视图 + 全局初始化
    ├── core/                 领域层
    ├── ui/                   视图层
    └── state/                持久化
```

## 5. 里程碑

| # | 目标 | 验收 |
|---|---|---|
| **M0** | 环境验证 | `gpui-kit` 编译通过并弹出 hello world 窗口（先做，暴露 Xcode/Metal 问题） |
| **M1** | 骨架 | `DockArea` 三栏布局：左文件树 / 中编辑 / 右预览 |
| **M2** | 目录管理 | 打开文件夹、树展示、新建/删除/重命名/移动（R1） |
| **M3** | 编辑与保存 | `Textarea` 编辑、防抖自动保存、原子写、多标签（R2） |
| **M4** | 预览 | `TextView` 实时渲染、滚动同步、主题适配（R3） |
| **M5** | 持久化 | 布局 + 标签 + 最近库，重启可恢复（R4） |
| **M6** | P1 公共底座 | `core/fuzzy` 模糊匹配、`core/markdown` 事实解析、`core/index` 全库索引 |
| **M7** | P1 导航与结构 | 命令面板（R10）、大纲（R9）、双链（R5）、反链（R6）、标签（R7）、全文搜索（R8） |
| **M8** | P1 收尾 | 明暗主题入口（R11）、Markdown 快捷键（R12）、数据文件（R13）、目录树增量刷新 |

M6–M8 的拆解与进度见 `docs/P1-PLAN.md`；各功能的取舍记录在设计文档
（`wikilink-design.md` / `outline-design.md` / `index-panels-design.md` /
`command-palette-design.md` / `note-index-design.md` /
`file-tree-incremental-refresh-design.md`）。

## 6. 风险登记表

| 风险 | 影响 | 对策 |
|---|---|---|
| **未安装完整 Xcode.app** | 高：Metal 相关编译可能失败 | M0 第一步就验证；必要时安装 Xcode（App Store） |
| gpui 是 pre-1.0，版本间 breaking change | 中 | 锁 `Cargo.lock`，不随手升级 `gpui-kit` |
| `gpui-pre` 快照随 Zed main 漂移 | 中 | 跟随 `gpui-kit` 官方升级节奏，不自行 bump |
| 组件库未覆盖的能力需自研 | 中 | 先查 `gpui-kit.com` 文档；确实没有再用 `gpui-base` 自建 |
| 首次编译耗时长 | 低 | 配 lld/mold；`cargo build` 预热 |

## 7. 测试策略

- **领域层**：`core/` 纯 Rust 逻辑（路径处理、索引、双链解析、原子写）用 `#[test]` 覆盖，
  这是投入产出比最高的部分，且不依赖 GUI
- **标签与会话**：`core/session.rs` 的标识归一化、保序去重、失效判定、活动标签回落
  全部是无 GUI 的纯函数，必须逐个覆盖（标签重复与恢复顺序错乱都源于此）
- **UI 层**：GPUI 提供 `gpui::test` 宏与 `TestAppContext`，对关键交互（打开文件、
  保存、切换标签）写组件级测试
- **手工冒烟**：每个里程碑按上表"验收"列逐条人工验证
