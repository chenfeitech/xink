# 设计：索引派生表（反向链接 / 标签 / 标题解析）

> 本文是对 `docs/navigation/note-index-design.md` §9 开放问题 2 的**结论修订**。
> 该文档当时决定"派生表不预建，查询时实时反查"，本文用实测数据说明该判断的
> 两条前提都不成立，并给出增量维护派生表的设计。

## 1. 现状与问题

### 1.1 代码事实

| 事实 | 证据 |
|---|---|
| `backlinks()` 查询时遍历**全部笔记 × 全部双链**，对每个链接做一次 `to_lowercase()` 分配 | `src/core/index.rs:169-193` |
| `tags()` 查询时遍历全部笔记，为每个标签维护 `Vec<PathBuf>` 并用 `contains` 去重 | `src/core/index.rs:196-213`（去重在 201-204） |
| `notes_with_tag()` 同样全表扫，且对每个标签再 `to_lowercase()` | `src/core/index.rs:216-226` |
| `resolve_wikilink()` 第 3 步（按标题匹配）全表扫，每篇笔记 `title.to_lowercase()` | `src/core/index.rs:250-256` |
| 三个查询都在 **render** 里被调用 | `src/ui/backlink.rs:88`、`src/ui/tags.rs:77`、`src/ui/tags.rs:124` |
| 面板靠 `IndexChanged` 触发重绘 | `src/ui/backlink.rs:25`、`src/ui/tags.rs:25` |
| `IndexChanged` 在**每次保存**时发射，而自动保存防抖为 300ms | `src/app.rs:211-220`、`src/ui/editor.rs:1508`、`src/core/config.rs:420` |
| 预览源文每次重算都会为每个 `[[双链]]` 调一次双链解析 | `src/ui/editor.rs:469`、`src/ui/editor.rs:1960` |

### 1.2 实测（release 构建，3000 篇笔记，每篇 5 个双链 + 3 个标签）

| 操作 | 单次耗时 | 位置 |
|---|---|---|
| `NoteIndex::build` | 68ms | 后台线程，不阻塞 UI |
| `backlinks()` | 0.82ms | **UI 线程** |
| `tags()` | **345ms** | **UI 线程** |
| `notes_with_tag()` | 1.9ms | **UI 线程** |
| `rewrite_wikilinks`（20 个按标题解析的双链） | 2.2ms | **UI 线程** |

`tags()` 是 O(n²)：每个标签的 `Vec<PathBuf>` 越长，`contains` 越慢。
**标签越少、笔记越多就越慢**——而真实库恰好是"几十个标签 + 几千篇笔记"。

### 1.3 结论

`note-index-design.md` §3 拒绝派生表的两条理由，实测都不成立：

1. "万级笔记 × 平均个位数链接 ≈ 亚毫秒" —— `tags()` 是 345ms，差三个数量级；
2. "只在切换笔记 / 打开面板时调用" —— 每次自动保存都发 `IndexChanged`，
   打字期间每 300ms 触发一次全量重算。

结果：标签抽屉打开时打字，每 300ms 卡一次 345ms。这是用户可感知的卡顿，不是理论瓶颈。

## 2. 目标 / 非目标

**目标**

- `backlinks` / `tags` / `notes_with_tag` / `resolve_wikilink` 从 O(n) 降到 O(1) ~ O(命中数)
- 四个查询的**公开签名与返回语义完全不变**，调用方（`ui/backlink.rs`、`ui/tags.rs`、`ui/editor.rs`）零改动
- 派生表与 `notes` 主表的一致性可被测试证明（见 §7）

**非目标**

- 不改索引覆盖范围（仍只收 `.md`）、不落盘、不做跨库检索
- 不引入新依赖
- 不改 UI 层的刷新时机（`IndexChanged` 是否降频是独立问题，见 §9）
- 不做正文级倒排索引：全文搜索仍走 `core/search.rs` 按需读盘

## 3. 方案概述

**主表 + 增量维护的派生表**。在 `NoteIndex` 上新增三张表：

```
by_title:   HashMap<String, Vec<PathBuf>>        // 标题小写 → 路径
by_tag:     HashMap<String, HashSet<PathBuf>>    // 标签小写 → 路径集合
link_refs:  HashMap<String, Vec<LinkRef>>        // 归一化双链目标 → 写下的引用
```

`by_tag` 用 `HashSet` 而不是 `Vec`：旧实现用 `Vec` + `contains` 去重，
是 O(该标签下笔记数²)，正是 `tags()` 345ms 的来源。

**关键取舍：`link_refs` 存"写了什么"，不存"指向谁"。**

`backlinks(P)` 在查询时才用 `link_targets_of(P)`（`src/core/index.rs:309-320`；
一篇笔记能被 stem / 相对路径 / 相对路径去扩展名三种写法命中）取出对应的桶并合并，
再滤掉自引。反过来——登记时就把双链解析成具体路径——是错的，见 §3.2 方案 D。

### 3.1 单一写出口

所有写操作收敛到三个私有方法，`build` / `reindex_file` / `remove_file` 只调它们，
且三种写操作走**完全相同**的路径：

```
detach(path)        // 用旧事实把该路径从全部表里摘干净
attach_keys(note)   // 登记主表 + by_title + by_tag
attach_links(path)  // 登记 link_refs
```

因为 `attach_links` 不解析目标是否在库里，新增、重解析、删除无需区分，
`build` 也回到单趟。这直接回应原设计拒绝派生表的理由（"增量更新最容易写错"）：
错只可能错在这一组方法里，而 §7 的差异测试会把它钉死。

### 3.2 被否决的方案

| 方案 | 否决理由 |
|---|---|
| A. 查询加缓存 + 失效标记 | 失效判定要覆盖路径 / 标题 / 标签三类变化，比维护派生表更容易漏；且首次查询仍要付一次 O(n²)，卡顿只是被推迟 |
| B. 只把 `tags()` 的 `Vec` 换成 `HashSet` | 只治 `tags`，`backlinks` 与 `resolve_wikilink` 仍是 O(n)；改动量相近却留一半问题 |
| C. 查询挪到后台线程 | UI 要处理"数据还没到"的空态，打字期间每 300ms 一次线程往返；复杂度高于维护派生表，且不减少总工作量 |
| **D. 登记双链时就解析成具体路径**（本文初版：`link_keys` + `backlinks` + `out_links`） | **实施中被差异测试否掉**。解析结果取决于 `attach` 那一刻库里有哪些笔记：先写的笔记引用了一篇后建的笔记，那份引用当时记不下、之后也没人补。补救只能是"每次出现新笔记就全库重解析一遍"，批量导入 100 个文件会退化成 100 × O(n·m)。按目标文本分桶后这个问题根本不存在 |

## 4. 数据模型与持久化

```rust
pub struct NoteIndex {
    root: Option<PathBuf>,
    notes: HashMap<PathBuf, IndexedNote>,           // 唯一真值（不变）

    by_name: HashMap<String, Vec<PathBuf>>,         // 已有
    by_rel_path: HashMap<String, PathBuf>,          // 已有

    // 新增
    by_title: HashMap<String, Vec<PathBuf>>,
    by_tag: HashMap<String, HashSet<PathBuf>>,
    link_refs: HashMap<String, Vec<LinkRef>>,
}
```

- **不落盘**：索引是磁盘的派生数据（规则 04），无格式兼容与旧数据迁移问题
- **内存增量**：`LinkRef` 总量等于全库双链总数，与拆除前的 `backlinks` 表同量级——
  没有多占，只是换了个组织方式；`by_title` / `by_tag` 各是每篇一两个条目
- **公开 API 不变**：`IndexedNote` / `LinkRef` / `Resolution` 结构不变，
  四个查询方法签名与排序规则不变（`tags()` 仍是"数量降序、同数量字典序"）

## 5. 交互流程

打字 → 保存 → 面板重绘的正常路径（改动前后**完全一致**，只有内部耗时变化）：

```
用户输入
  → EditorPanel::on_change → schedule_save（防抖 300ms）
      ├─ refresh_doc → rewrite_wikilinks → resolve_wikilink   [O(n) → O(1)/次]
      └─ save_now → Note::save → index_note_saved
              → reindex_file → detach + attach
              → emit IndexChanged
                  → BacklinkPanel::render → backlinks()        [O(n·m) → O(命中数)]
                  → TagsPanel::render     → tags()             [O(n²) → O(标签数)]
```

外部变更（访达里增删改名）走 `handle_watch_event` → `index_note_saved` /
`index_note_removed`，同样是 `detach` + `attach`，与保存路径共用一份代码。

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 单篇笔记解析失败 / 非 UTF-8 | `reindex_file` 直接 return，不登记；该篇的旧条目已在 `detach` 中摘除，不会留下半截派生数据 |
| 同名笔记（两个路径同一 stem） | 都进 `by_name` 与 `link_keys`，解析仍返回 `Ambiguous`；一条双链会同时进两个 `backlinks` 桶，与现状一致 |
| 标题重复 | 都进 `by_title`，`resolve_wikilink` 仍返回 `Ambiguous` |
| 自引用 `[[自己]]` | 跳过 `p == note.path`，与现状一致（`src/core/index.rs:177-179`） |
| 标签大小写不同（`#Rust` / `#rust`） | 统一小写归一后进 `by_tag`，与现状一致 |
| 一篇笔记内重复写同一个标签 | `HashSet` 天然去重，`tags()` 计数不重复，与现状一致（现状靠 `contains` 去重） |
| 笔记被删除 | `remove_file` → `detach`：同时摘掉它作为**引用来源**的贡献（`out_links`）和作为**被引用目标**的桶（`backlinks`） |
| 笔记被改名 / 移到库外 | 监听给的是 `[旧路径, 新路径]`，两个都走 `reindex_file`：旧路径已不存在，走"文件不在了"分支被摘除（连同它的标签与写下的引用）；新路径正常登记。**只按"文件不存在"判定**——权限不足、非 UTF-8 等错误下文件仍在，此时保留旧条目比摘掉稳妥 |
| 构建期间文件被删 | 构建在后台线程完成后整体替换；下一次增量更新剔除，与现状一致 |
| 未打开库 | `root = None`，全部表为空，查询返回空 |

## 7. 测试计划

在 `tests/core/index.rs` 增加（临时目录，沿用现有 `tmp()` / `note()` / `index_of()` 辅助）：

1. **差异测试（核心）**：同一批笔记，分别用 `NoteIndex::build()` 全量构建、
   和用"空索引 + 逐篇 `reindex_file()`"构建，断言两者的 `backlinks()` / `tags()` /
   `notes_with_tag()` / `resolve_wikilink()` 结果**逐项相等**。
   这条直接保证"增量维护不会写错派生表"。
2. **不变量测试**：随机序列的增 / 改 / 删之后，从 `notes` 用**慢速全表重算**
   得到一份派生表快照，与维护出来的逐项比对相等。
3. **幂等测试**：同一路径连续 `reindex_file` 两次，派生表与一次的结果一致
   （防 `attach` 重复 push）。
4. **语义回归**：现有 `tests/core/index.rs` 全部用例保持通过、不修改断言，
   作为"公开语义未变"的证明。
5. **规模测试**：3000 篇量级下 `tags()` 单次耗时应从 345ms 降到毫秒以下。
   建议**只写成打印不写成断言**——时间断言在无 CI 的本地环境不稳定；
   阈值由 §8 人工验收把关。

`cargo test` 现有 333 项必须全绿；`cargo fmt --check` 与 `cargo clippy --all-targets` 无输出。

## 8. 人工验收清单

- [ ] 打开标签抽屉，在 3000 篇量级的库里连续打字 30 秒，界面不卡（改动前每 300ms 卡一次）
- [ ] 打字期间反链面板与标签面板的数字随保存实时更新
- [ ] 修改一篇笔记的 `#标签` 并保存后，标签面板的计数与列表立即变化
- [ ] 在访达里删除一篇被引用的笔记，反链面板里该条消失
- [ ] 在访达里新增一篇带 `[[双链]]` 的笔记，源笔记的反链面板立即多出一条
- [ ] 两篇同名笔记：`[[名字]]` 仍提示歧义，反链仍双向正确
- [ ] 冷启动 3000 篇库：索引构建耗时与改动前相当（构建在后台，UI 不卡）

## 9. 风险与开放问题

| # | 类型 | 问题 | 处置 |
|---|---|---|---|
| 1 | 风险 | `link_refs` 的分桶键必须与 `backlinks` 查询侧 `link_targets_of` 的归一化口径一致（`trim` + 去 `.md` + 小写） | 两侧共用同一组函数，不各写一份；由 §7 的差异测试与随机一致性测试覆盖 |
| 2 | 风险 | `detach` 漏摘某张表会导致查询逐渐失真，且不会报错 | §7 不变量测试；三个 `attach*` / `detach` 函数相邻放置，便于逐表对照 |
| 5 | 已知 | 笔记被删除后，指向它的 `link_refs` 条目会留下（查询侧按 `link_targets_of(P)` 取桶，P 已不在库里就不会被问到） | 无害：该路径被重新创建时这些引用恰好仍然正确，正是"全删全加"用例期望的行为 |
| 3 | 开放 | `IndexChanged` 每 300ms 一次是否本身过密 | **不在本设计范围**。本设计让每次重算变便宜，但不改变发射频率。若验收仍觉频繁，另立议题考虑把索引更新降频到 500ms（与规则 04 §6 的"编辑 → 索引重建 500ms"一致） |
| 4 | 已消解 | ~~`out_links` 是否值得多占一份表~~ | 方案 D 被否掉后不再需要：`detach` 用旧事实算出分桶键即可直接摘除 |

## 10. 实施步骤

| 步 | 内容 | 可独立验证 |
|---|---|---|
| S1 | 在 `NoteIndex` 加三张派生表字段；实现 `detach` / `attach_keys` / `attach_links`；`build` / `reindex_file` / `remove_file` 改为调用它们 | 现有 333 项测试全绿（语义未变） |
| S2 | 四个查询改为读派生表 | 差异测试 + 随机一致性测试通过 |
| S3 | 补规模验证：打印 3000 篇下各查询耗时，与本文 §1.2 对照 | 数字落在毫秒级 |
| S4 | 人工验收（§8） | 清单逐条打勾 |
| S5 | 修复改名残留：`reindex_file` 在文件已不存在时改为摘除旧条目（S1–S3 之后发现的既有缺陷） | 改名三条用例通过；全量 340 项 |

S1 与 S2 可以合为一个 commit，但 S1 结束时必须能独立跑通全部现有测试——
那是"公开语义未变"的第一道证明。

## 11. 实施结果与实测对比

2026-10-23 完成 S1–S3。release 构建，3000 篇笔记（每篇 5 个双链 + 3 个标签），与 §1.2 同口径：

| 操作 | 改动前 | 改动后 | 变化 |
|---|---|---|---|
| `NoteIndex::build` | 68ms | 70ms | 持平（仍是后台线程） |
| `backlinks()` | 0.82ms | **1.95µs** | ~420x |
| `tags()` | 345ms | **0.22µs** | ~1.5e6 x |
| `notes_with_tag()` | 1.9ms | 1.51ms | 1.26x，见下 |
| `rewrite_wikilinks`（20 个按标题解析的双链） | 2.2ms | **27µs** | ~81x |
| `reindex_file()`（单次自动保存走的路径） | — | 17.8µs | — |

`notes_with_tag("标签1")` 只快了 1.26x，因为它要**返回 3000 条路径并排序**——耗时由产出结果的大小决定，不是由查找决定。这条本来就不是瓶颈：旧实现是全表扫 O(n) 加上同样的 O(k log k) 排序，k≈n 时两者同量级。

`tags()` 从"每 300ms 卡 345ms"变成 0.22µs，是本次的主要收益。

**与本文初版的偏差**：初版 §3 / §4 设计的是五张表（`link_keys` + `backlinks` + `out_links` + `by_title` + `by_tag`），实施中被差异测试否掉，改为三张表（见 §3.2 方案 D）。偏差理由是**正确性而非性能**：初版在"引用先出现、目标后出现"时会永久丢引用。最终实现更简单（少两张表、`build` 回到单趟），查询复杂度相同。
