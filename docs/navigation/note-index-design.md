# 设计：全库索引 `core/index.rs`（P1-T6）

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 没有任何全库索引：R5 双链、R6 反链、R7 标签、R8 搜索都缺同一份"库里有什么" | `src/core/mod.rs` 无 `index` 模块 |
| 已经能拿到单篇笔记的事实（标题 / 链接 / 双链 / 标签，带行号） | `src/core/markdown.rs::parse_facts`（T5 已完成） |
| 命令面板（T7）**不需要**索引：它直接扫 `Vault::scan` 拿文件名 | `src/ui/command.rs::collect_candidates` |
| 库根由 `AppState.vaults.active_root()` 派生，切库要重建索引 | `src/app.rs:339`、`src/core/vault.rs:339` |
| 写盘只有一处出口 `Note::save`（原子写）；外部变更由 `VaultWatcher` 播报 | `src/core/note.rs:79`、`src/ui/editor.rs:491`、目录树已订阅 `handle_watch_event` |

不做索引的后果：R5/R6/R7/R8 各自扫盘、各自解析，四个功能的"什么是笔记、什么是标签"必然不一致。

## 2. 目标 / 非目标

**目标**

- 全库笔记的标题、外链、双链、标签一次解析、多处复用
- 构建放后台，不阻塞 UI；单文件变更可增量更新
- 只依赖磁盘：不落盘、不做缓存失效的心智负担

**非目标**

- 不存正文（正文永远读磁盘，T12 搜索按需读文件）
- 不做跨库检索 —— **已决定不做**（2026-10-20）：索引只覆盖当前库，切库即重建，
  不维护跨库的合并索引（见 `docs/vault/multi-vault-design.md` 非目标）
- 不解析非笔记文件（数据文件不参与双链 / 标签）

## 3. 方案概述

**主表 + 实时派生查询**，而不是维护多张派生表：

```
notes: HashMap<PathBuf, IndexedNote>        // 唯一真值（磁盘的派生）
by_name / by_rel_path: 文件名 / 相对路径 → 路径   // 双链解析用
```

`backlinks()`、`tags()` 在查询时从 `notes` 反查（万级笔记 × 平均个位数链接 ≈ 亚毫秒，
且只在切换笔记 / 打开面板时调用）。理由：派生表在增量更新时最容易写错，
而本轮没有性能瓶颈证明需要它；真到瓶颈再加缓存并补测试。

**构建流程**

```
启动 / 切库 → background_spawn(NoteIndex::build(root))
            → 回到实体整体替换（构建期间旧索引继续可用）
保存 / 外部变更 → reindex_file(path) / remove_file(path)（幂等，~1ms）
```

**覆盖范围**：只收 `.md`。数据文件（json/yaml/toml）不参与双链与标签。

## 4. 数据模型

```rust
pub struct NoteIndex {
    root: Option<PathBuf>,
    notes: HashMap<PathBuf, IndexedNote>,
    by_name: HashMap<String, Vec<PathBuf>>,      // 文件名小写（不含扩展名）
    by_rel_path: HashMap<String, PathBuf>,       // 相对库根路径小写
}

pub struct IndexedNote {
    pub path: PathBuf,
    /// 标题：第一个 H1 → 否则第一个标题 → 否则文件名（去扩展名）
    pub title: String,
    pub facts: DocFacts,
}

pub struct LinkRef { pub from: PathBuf, pub line: usize, pub target: String }

pub enum Resolution { Hit(PathBuf), Ambiguous(Vec<PathBuf>), Missing }
```

- **不落盘**：索引是磁盘的派生数据（规则 04），启动重建即可
- `IndexedNote` 只在内存，随库切换整体丢弃

## 5. 关键规则

**双链目标解析**（`resolve_wikilink`，优先级从高到低，全部大小写不敏感）：

1. 相对库根路径（`notes/a.md`、`notes/a`）；
2. 文件名（不含扩展名）；
3. 笔记标题。

多个命中返回 `Ambiguous`，由 UI 决定如何提示（T9 处理）。

**标题**：第一个 H1；没有则第一个任意层级标题；都没有则用文件名去扩展名。

**增量更新的时机**：

| 事件 | 动作 | 触发点 |
|---|---|---|
| 启动 / 切库 | 后台全量构建 | `AppState` 初始化后、切库成功后 |
| 笔记保存 | `reindex_file(该路径)` | `EditorPanel::save_now` 成功后 |
| 监听：Created / Renamed | `reindex_file`（**旧路径若已不存在则摘除该条目**，改名事件会给 `[旧, 新]` 两个路径） | 目录树 `handle_watch_event` |
| 监听：Removed | `remove_file` | 同上 |

应用内的新建 / 删除 / 重命名最终都会落到 watcher 事件上（写的是真盘），
因此不需要在每一处操作里再补一次；只有"保存内容"必须显式挂钩
（内容变更只产生 `Modified`，不重扫结构）。

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 没有打开库 | `root = None`，`notes` 为空，查询返回空（不报错） |
| 构建中查询 | 旧索引继续服务（构建在后台完成才整体替换） |
| 单个文件读失败 / 非 UTF-8 | 跳过该文件，不中断整库构建 |
| 文件在构建期间被删除 | 下次增量更新时剔除；查询到已不存在路径时按需过滤 |
| 同名的两篇笔记 | 都进 `by_name`，解析时返回 `Ambiguous` |
| 超大库 | 构建在后台线程；`Vault::scan` 已经忽略 `.git` / `node_modules` |

## 7. 测试计划（`tests/core/index.rs`，临时目录）

- 构建：多目录、嵌套、只收 `.md`、忽略数据文件与噪声目录
- 标题：H1 优先、退化到首个标题、退化到文件名
- 反链：引用者、行号、别名；删除引用者后反链消失
- 标签：聚合与计数、同标签多篇、大小写归一
- 双链解析：文件名 / 相对路径 / 标题 / 歧义 / 缺失
- 增量：保存后反链与标签更新；删除后反链消失；幂等（重复 reindex 结果一致）

## 8. 人工验收清单

本任务无界面，验收等同于"下游可用 + 无性能退化"：

- [ ] 万文件库启动：索引在后台完成，期间 UI 不卡（状态栏有提示）
- [ ] 编辑并保存一篇含 `[[双链]]` 与 `#标签` 的笔记后，反链 / 标签立即更新（T10 / T11 落地后验证）
- [ ] 在访达里删除 / 新增笔记后，索引同步更新

## 9. 开放问题

| # | 问题 | 决定 |
|---|---|---|
| 1 | 是否索引数据文件（json/yaml） | 否：不参与双链与标签 |
| 2 | 派生表（backlinks/tags 反向表）是否预建 | 原决定"否：查询时实时反查，见 §3"。**2026-10-23 已修订**：实测 `tags()` 在 3000 篇库上单次 345ms，且每次自动保存都会触发，两条前提均不成立。改为增量维护派生表，见 [index-derived-tables-design.md](./index-derived-tables-design.md) |
| 3 | 索引是否需要落盘加速启动 | 否：派生数据，启动后台重建 |

## 10. 实施步骤

| 步 | 内容 |
|---|---|
| S1 | `core/index.rs`：`NoteIndex` / `IndexedNote` / `build` / 查询 API + 单测 |
| S2 | `AppState.notes` 挂载 + 启动 / 切库触发后台构建 |
| S3 | 增量挂钩：保存后重解析、监听事件增删 |
