# 设计：目录树跟随活动笔记（P1-R14）

> 需求见 `docs/file-tree/file-tree-reveal-active-note-requirements.md`。

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 文件树的选中项只在用户点击时改变，没有任何"按活动笔记定位"的逻辑 | `src/ui/file_tree.rs:1246`（`set_selected_index` 只在 `on_click` 里出现） |
| **激活路径有两条**，只挂一条会漏 | `src/ui/tabs.rs:189`（`open_note_at`：树/命令面板/搜索/大纲/反链/标签/双链/启动恢复）；`src/ui/editor.rs:1867`（`set_active`：dock 上点已存在的标签页只走这里） |
| 树的 item id 就是路径字符串 | `src/ui/file_tree.rs:869` |
| 刷新走"保状态合并"，展开状态靠 `TreeItem::clone()` 共享 `Rc<RefCell<TreeItemState>>` 继承 | `src/ui/file_tree.rs:879-884`、`907-915` |
| `apply_tree` 每次扫描完都 `set_items`（**清空选中**）再按 id 恢复，并写回滚动偏移 | `src/ui/file_tree.rs:726-770` |
| 树在后台扫描，冷启动恢复标签时树可能还没就绪 | `src/ui/file_tree.rs:546`（`scan_and_apply`） |
| **框架已提供 `reveal_item`**：展开所有折叠祖先 + 滚动到可见 | `gpui-base-0.8.0/src/tree.rs:275-284`，内部 `expand_ancestors` 在 `:298`；框架自己的用例在 `:666` |
| `reveal_item` 的 `ScrollStrategy` 参数可命名：gpui-kit 顶层 `pub use ::gpui::*` | `gpui-kit-0.8.0/src/lib.rs:101`；`src/ui/file_tree.rs:17` 已 `use gpui_kit::*` |
| `AppState` 里**没有** FilesPanel 字段（outline / preview / rail 有） | `src/app.rs:80-115` |
| FilesPanel 由两处创建 | `src/app.rs:671`（默认布局）、`src/main.rs:24-26`（布局恢复的面板工厂） |

## 2. 目标 / 非目标

**目标**：实现 FR1–FR7（见需求文档）。

**非目标**：

- 不改文件树的刷新与展开状态持久化机制
- 不改"点树即打开笔记"的既有行为
- 不引入事件总线（见 §3.3 方案 C）

## 3. 方案概述

钩子挂在 `EditorPanel::set_active`，因为需求定义的触发场景（点已存在的标签页）
只走这一条路径；其余打开入口最终也都会经过标签切换，因此一处覆盖全部。

```
EditorPanel::set_active(active=true)
  → crate::ui::file_tree::reveal_note(path, cx)
      → FilesPanel::reveal(path, cx)
          ├─ 不在当前库 → 放弃（FR6）
          ├─ 记 pending_reveal，立即 try_reveal
          │     ├─ 命中：reveal_item（展开 + 滚动）→ index_of → set_selected_index
          │     └─ 未命中（树还没扫到）→ 保留 pending，等 apply_tree 补（FR7）
          └─ apply_tree 末尾：恢复选中/滚动之后，再补一次 pending 揭示
```

### 3.1 拿到 FilesPanel 实体

`AppState` 新增 `files: Option<WeakEntity<FilesPanel>>`。`FilesPanel::new` 的签名
从 `&mut App` 改为 `&mut Context<Self>`，以便用 `cx.weak_entity()` 自注册——
与 `EditorPanel::create` 登记 `empty_panels` 的做法同源（`src/ui/editor.rs:219`）。
两处调用点（`src/app.rs:671`、`src/main.rs:25`）的闭包参数本就是
`&mut Context<FilesPanel>`，无需改动调用侧。

注册用 `try_global` 兜底：当前两条创建路径都在 `AppState` 的 `set_global`
（`src/app.rs:478`）之后，但保持不 panic 更安全。

### 3.2 顺序很重要

`reveal_item` 内部会 `expand_ancestors` → `rebuild_entries`，**展开前后条目下标会变**。
因此必须"先 `reveal_item`，再 `index_of`，再 `set_selected_index`"；
预先算好的下标在展开后是无效的。

同理，`apply_tree` 里要先按 id 恢复选中、写回滚动偏移，**最后**再补 pending 揭示，
否则揭示的滚动会被写回的旧偏移覆盖。

### 3.3 被否决的方案

| 方案 | 否决理由 |
|---|---|
| A. 钩子挂在 `tabs::open_note_at` | 漏掉需求定义的触发场景：dock 上点已存在的标签页不经过它（`src/ui/tabs.rs:206-216` 的"已打开"分支只 `reveal_panel` + `tabs.activate`，随后由 dock 回调 `set_active`） |
| B. 在 `Render::render` 里读 `tabs.active` 并揭示 | 违反规则 02 §6（render 只做读状态 + 构建元素树）；且展开会 `rebuild_entries` + `cx.notify`，有重入与循环风险 |
| C. 引入事件总线或新增一个 Entity 做中转 | 与既有 `ui::outline::update_outline` 的直连模式不一致；为这一个功能引入新机制属于独立重构，收益不匹配 |

## 4. 数据模型与持久化

```rust
// AppState（src/app.rs）
/// 文件树面板的弱引用，供"活动笔记变化时定位树节点"使用。
/// 与 EditorPanel 登记 empty_panels 同源：面板创建时自注册。
pub files: Option<WeakEntity<FilesPanel>>,

// FilesPanel（src/ui/file_tree.rs）
/// 待揭示的笔记路径：树还没扫到它时先记下，扫描完成后补一次（FR7）
pending_reveal: Option<PathBuf>,
```

- **不落盘**：这是纯 UI 瞬时状态，与布局/标签恢复无关
- 不新增配置项的开关（需求未要求可关闭）

## 5. 交互流程

正常路径（点已存在的标签页）：

1. dock 切换标签 → `EditorPanel::set_active(true)`
2. 取 `self.note` 的路径 → `file_tree::reveal_note(path, cx)`
3. FilesPanel 校验路径在当前库内 → 置 `pending_reveal`
4. `try_reveal`：`reveal_item(id, ScrollStrategy::Center)` 展开祖先并滚动
5. 展开后 `index_of(id)` → `set_selected_index(Some(ix))`
6. 命中则清空 `pending_reveal`

冷启动路径：

1. 恢复标签 → `set_active` → `reveal_note`（此时树还在后台扫描）
2. `try_reveal` 未命中 → `pending_reveal` 保留
3. 扫描完成 → `apply_tree`：恢复选中与滚动偏移 → 补一次 `try_reveal` → 清空

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 路径不在当前库根之下 | `reveal` 直接放弃，不置 `pending_reveal`（否则会一直留着、每次刷新都抢一次选中） |
| 笔记在 `.git` / `node_modules` / `target` | 树里本就没有该节点 → `try_reveal` 未命中 → 静默（FR6） |
| 未打开库 | `self.vault` 为 `None` → 放弃 |
| 扫描中途用户又切了标签 | `pending_reveal` 被覆盖为最新路径，只揭示最后一篇 |
| `AppState` 里 `files` 为 `None`（面板尚未创建/已释放） | `reveal_note` 静默返回 |
| 重复触发同一篇笔记 | `try_reveal` 幂等（同 id 展开+选中+滚动结果不变）；命中后清空 pending，不会每次刷新都重抢 |
| 与 `apply_tree` 的竞争 | 见 §3.2：揭示放在恢复之后 |

## 7. 测试计划

本功能几乎全是 UI 行为，`core/` 层没有可拆的纯逻辑，因此：

1. **自动化**：以"不回归"为主——现有 341 项测试必须全绿，
   `cargo fmt --check` 与 `cargo clippy --all-targets` 无输出。
2. **可自动化的单点**：`reveal` 的守卫（库外路径不置 `pending_reveal`）需要
   `Context<FilesPanel>`，当前测试体系没有窗口环境（规则 03 §6 提到 UI 交互测试
   需要 `gpui::test` + `TestAppContext`），**不在本次范围**，留作开放问题。
3. **行为验证**：靠 §8 人工验收清单。

## 8. 人工验收清单

- [ ] 打开一篇藏在两层未展开目录里的笔记，切到它的标签 → 两级目录自动展开，节点选中且可见
- [ ] 目录树很长、目标在视口外时切标签 → 树滚到目标可见
- [ ] 在树里先选中另一个文件，再切标签 → 选中被切到活动笔记（FR4）
- [ ] 反复点同一篇已打开的笔记的标签 → 不抖动、不重复滚动
- [ ] 冷启动：退出时有 3 个打开的标签且分布在不同目录 → 启动后树扫完自动定位到活动标签那篇
- [ ] 从命令面板（`Cmd+P`）打开一篇深目录里的笔记 → 树同步定位
- [ ] 从搜索结果 / 反链 / 标签面板跳转 → 树同步定位
- [ ] 打开一篇库外文件（如通过"打开库"之外的路径）→ 无异常，树不动（FR6）
- [ ] 关闭一个标签后焦点落到邻居 → 树跟随到邻居
- [ ] 切标签后立刻在树里做新建/重命名 → 目标目录仍是用户点的那个（不被揭示逻辑干扰）

## 9. 风险与开放问题

| # | 类型 | 内容 | 处置 |
|---|---|---|---|
| 1 | 风险 | `reveal_note` 从 `EditorPanel` 直连 `FilesPanel`，形式上不符合规则 02 §5「视图之间不直接调用」 | 与既有 `set_active` → `ui::outline::update_outline` 完全同一模式；统一改成事件机制是独立重构，见 §3.3 方案 C。**需用户确认是否接受沿用现有模式** |
| 2 | 风险 | `FilesPanel::new` 改签名（&mut App → &mut Context<Self>） | 两处调用点的闭包参数本就是 `Context<FilesPanel>`，无需改调用侧；编译期即可验证 |
| 3 | 风险 | 面板自注册依赖 `AppState` 已存在 | 用 `try_global` 兜底；当前两条创建路径都在 `set_global` 之后（已核实） |
| 4 | 开放 | 是否需要可配置开关（"关闭目录树跟随"） | 需求未要求，本次不做；若有需要另立配置项 |
| 5 | 开放 | 揭示的滚动策略用 `Center` 还是 `Top` | 本次取 `Center`（目标落在视口中部，上下文更完整）；验收时若觉得跳动明显可换 `Top` |

## 10. 实施步骤

| 步 | 内容 | 可独立验证 |
|---|---|---|
| S1 | `AppState` 加 `files` 字段；`FilesPanel::new` 改签名并自注册 | 编译通过，现有 341 项测试全绿 |
| S2 | `FilesPanel` 加 `pending_reveal` 与 `reveal` / `try_reveal`；`apply_tree` 末尾补揭示 | 编译通过；单独打开库不触发时不改变任何既有行为 |
| S3 | `EditorPanel::set_active` 里调 `reveal_note` | 编译通过；§8 清单逐条验收 |
| S4 | 按需补 `docs/INDEX.md` 与 `REQUIREMENTS.md` 的 R14 登记 | 文档自洽 |
