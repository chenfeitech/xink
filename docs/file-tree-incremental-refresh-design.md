# 文件目录树增量刷新设计

> 目标：操作（删除 / 重命名 / 移动 / 新建 / 导入 / 粘贴 / 拖拽）与监听事件（新增 / 删除 /
> 重命名）之后，目录树**只更新受影响的节点**，其余节点的展开折叠状态、选中态、滚动位置保持不变。

- 状态：待确认
- 涉及文件：`src/ui/file_tree.rs`、`src/core/watcher.rs`、`tests/ui/file_tree.rs`、`tests/core/watcher.rs`
- 不涉及：`Cargo.toml`（不新增依赖）、持久化格式

---

## 1. 现状与问题（含代码位置证据）

### 1.1 刷新链路：所有入口最终都重建整棵 `TreeItem`

9 处调用 `refresh()`（删除 `:199`、重命名 `:310`、移动 `:342`、恢复库 `:364`、监听 `:444`、
新建 `:478`、导入 `:557`、粘贴 `:621`、拖拽移动 `:671`，另有手动刷新 `:519` 与切库守卫 `:866`），
最终都汇入 `apply_tree`：

```677:690:src/ui/file_tree.rs
    fn apply_tree(&mut self, node: FileNode, cx: &mut Context<Self>) {
        let notes = count_notes(&node);
        // 目录类型随树一起缓存：空文件夹没有子节点，无法靠子节点数量判断
        let mut flags = HashMap::new();
        collect_dir_flags(&node, &mut flags);
        self.dir_flags = Rc::new(flags);
        // 库根默认展开
        let items = vec![to_tree_item(&node).expanded(true)];
        self.tree_state
            .update(cx, |state, cx| state.set_items(items, cx));
        println!("[xink] 文件树已更新: {notes} 篇笔记");
        self.status = format!("{notes} 篇笔记").into();
        cx.notify();
    }
```

`to_tree_item` 每次**从零**构造 `TreeItem`：

```778:785:src/ui/file_tree.rs
fn to_tree_item(node: &FileNode) -> TreeItem {
    let id: SharedString = node.path.to_string_lossy().to_string().into();
    let mut item = TreeItem::new(id, node.name.as_str());
    for child in &node.children {
        item = item.child(to_tree_item(child));
    }
    item
}
```

### 1.2 树整体收缩的根因：展开状态存在 `TreeItem` 自己身上

`gpui-base-0.6.4/src/tree.rs`：

```34:46:.cargo/registry/src/.../gpui-base-0.6.4/src/tree.rs
struct TreeItemState {
    expanded: bool,
    disabled: bool,
}

#[derive(Clone)]
pub struct TreeItem {
    pub id: SharedString,
    pub label: SharedString,
    pub children: Vec<TreeItem>,
    state: Rc<RefCell<TreeItemState>>,
}
```

- `TreeState` **没有**"已展开 id 集合"，唯一来源是每个 `TreeItem` 的 `state`。
- `TreeItem::new`（同文件 `:99-109`）总是 `expanded: false`。
- 因此 `to_tree_item` 造出的新树里，**除根节点被 `.expanded(true)` 强制展开外全部折叠**
  （`file_tree.rs:684`）→ 这就是用户看到的"目录树整体收缩"。

### 1.3 选中态丢失

`TreeState::set_items` 显式清空选中与右键索引（`tree.rs:214-219`）：

```214:219:.cargo/registry/src/.../gpui-base-0.6.4/src/tree.rs
    pub fn set_items(&mut self, items: impl Into<Vec<TreeItem>>, cx: &mut Context<Self>) {
        self.replace_items(items.into());
        self.selected_ix = None;
        self.right_clicked_ix = None;
        cx.notify();
    }
```

`apply_tree` 没有恢复逻辑 → 删除/新建后工具栏与右键菜单失去目标。

### 1.4 滚动位置跳回顶部

`replace_items`/`add_entry`（`tree.rs:284-313`）只把**展开**的节点摊平进 `entries`。
新树全折叠 ⇒ `entries` 从 N 条塌缩到 1 条，`uniform_list` 在 prepaint 时把越界的纵向偏移夹回 0：

```396:405:.cargo/registry/src/.../gpui-pre-0.3.5/src/elements/uniform_list.rs
                if self.item_count > 0 {
                    let content_height = item_height * self.item_count;

                    let is_scrolled_vertically = !scroll_offset.y.is_zero();
                    let max_scroll_offset = padded_bounds.size.height - content_height;

                    if is_scrolled_vertically && scroll_offset.y < max_scroll_offset {
                        shared_scroll_offset.borrow_mut().y = max_scroll_offset;
                        scroll_offset.y = max_scroll_offset;
                    }
```

`FilesPanel` 全程没有保存/恢复偏移量。

### 1.5 外部重命名目前不刷新目录树

`notify` 把重命名折叠为 `Modify(Name(..))`（macOS FSEvents 后端对 `RenameMode::Both` 给出
`[旧路径, 新路径]`），而 `watcher.rs` 统一映射成 `Modified`：

```48:61:src/core/watcher.rs
            let kind = match event.kind {
                EventKind::Create(_) => WatchKind::Created,
                EventKind::Remove(_) => WatchKind::Removed,
                EventKind::Modify(_) => WatchKind::Modified,
                // 其余（访问、其它平台事件）与可见内容无关
                _ => return,
            };
```

`file_tree.rs:424` 的 `structure_changed` 只认 `Created | Removed`：

```424:425:src/ui/file_tree.rs
        let structure_changed = matches!(event.kind, WatchKind::Created | WatchKind::Removed);
```

→ 在访达 / 其他编辑器里重命名文件后，树里仍是旧名字，必须手动刷新。

---

## 2. 目标 / 非目标

### 目标

| # | 目标 |
|---|------|
| G1 | 删除 / 重命名 / 移动 / 新建 / 导入 / 粘贴 / 拖拽之后，树**不整体收缩**：未受影响的目录保持原展开 / 折叠状态 |
| G2 | 滚动位置不跳回顶部；变更节点在视口外时视口内容基本不动 |
| G3 | 选中项在路径仍存在时保持选中 |
| G4 | 外部（磁盘监听）新增 / 删除 / **重命名**同样走局部刷新 |
| G5 | 切换库、首次加载、手动刷新按钮的行为与现状一致（全新树，根展开） |

### 非目标

| # | 非目标 | 理由 |
|---|--------|------|
| N1 | 不做真正的"增量扫描"，仍是后台**全量扫描** + UI 层**增量合并** | 扫描在后台线程，`docs/DESIGN.md` 已认定万级文件量级可接受；真正的增量需要维护 watcher 与磁盘的最终一致性，正确性风险高 |
| N2 | 不做子目录懒加载 | 独立议题 |
| N3 | 不改排序规则、图标、右键菜单、拖拽行为 | 与本议题无关 |
| N4 | 不新增依赖、不改 `Cargo.toml` | 技术铁律 |
| N5 | 不做监听事件去抖 / 合并 | 独立议题，见 Q3 |

---

## 3. 方案概述

### 3.1 核心：保状态合并（reuse old `TreeItem`）

`FilesPanel` 自己留一份上次的树根（新增字段 `tree_items: Vec<TreeItem>`），
`apply_tree` 不再无条件 `to_tree_item`，而是走"旧树 + 新扫描结果 → 新树"的合并：

```rust
/// 旧 UI 树 + 新扫描结果 → 新 UI 树。
///
/// 路径即 id：路径没变 ⇒ 复用旧 `TreeItem`（`clone()` 共享 `state` 的 `Rc`，
/// 展开状态天然保留）；路径变了 ⇒ 新建。因此只有"受影响的那条路径"被重建，
/// 兄弟与祖先的视觉状态不受影响。
fn merge_tree_item(old: &TreeItem, new: &FileNode) -> TreeItem {
    let mut item = old.clone();                 // 共享 Rc<TreeItemState>
    item.label = new.name.as_str().into();      // 名称更新（大小写重命名）
    item.children = new
        .children
        .iter()
        .map(|child| {
            let id = child.path.to_string_lossy();
            match old.children.iter().find(|o| o.id.as_ref() == id.as_ref()) {
                Some(old_child) => merge_tree_item(old_child, child),
                None => to_tree_item(child),
            }
        })
        .collect();
    item
}
```

可行性依据（均已核对源码）：

1. `TreeItem` 是 `#[derive(Clone)]`（`tree.rs:40`），`clone()` 复制 `Vec` 但**共享**
   `Rc<RefCell<TreeItemState>>` → 覆盖 `children` 只影响副本，展开状态被继承。
2. `TreeItem.children` 是 `pub` 字段（`tree.rs:44`），可直接替换。
3. 路径即 id（`file_tree.rs:779`），重命名 / 移动 / 删除 / 新增都表现为"该层 id 集合变化"，
   天然等价于"局部变更"。
4. 递归深度 = 目录深度，复杂度 O(n)，与现有 `to_tree_item` 同量级，不引入额外开销。

根节点的特殊处理：

- 旧根 id（库根路径）与新扫描根 id **一致** → `merge_tree_item(&old_root, &new_root)`，
  **不再强制 `.expanded(true)`**（用户若手动折叠了根，也应当保留）。
- 不一致（首次加载 / 切换库 / 库本身被改名）→ `to_tree_item(&new_root).expanded(true)`，
  与现状完全一致。

### 3.2 选中态恢复

`set_items` 之前记下 `self.selected_path(cx)`，之后按 id 复位：

```rust
self.tree_state.update(cx, |state, cx| {
    state.set_items(items, cx);
    if let Some(id) = selected_id {
        state.set_selected_index(state.index_of(&id), cx);
    }
});
```

- `index_of`（`tree.rs:264-266`）只在已摊平的 `entries` 里查找；目标因祖先折叠而不可见、
  或已被删除时返回 `None` → 选中清空，**与现状一致**。
- `set_selected_index`（`tree.rs:225-228`）会 `cx.notify()`，无需额外通知。

### 3.3 滚动位置保持

```rust
// 保存
let offset = state.scroll_handle().0.borrow().base_handle.offset();
// 恢复
state.scroll_handle().0.borrow().base_handle.set_offset(offset);
```

- `TreeState::scroll_handle()` 是公开方法（`tree.rs:256-258`）。
- `UniformListScrollHandle` 是 pub 元组结构、内含 pub `base_handle`
  （`uniform_list.rs:80` + `:115-117`）；`ScrollHandle::offset / set_offset`
  （`div.rs:4260` / `:4396`）可直接读写。
- 合并后 `entries` 数量不再塌缩到 1，§1.4 的夹取基本不会被触发；这里只是兜底。

### 3.4 感知外部重命名

`src/core/watcher.rs`：

- `WatchKind` 增加 `Renamed` 变体；
- 把 `EventKind::Modify(ModifyKind::Name(_))` 映射为 `WatchKind::Renamed`；
  `Modify(Data | Any | Metadata | Other)` 仍为 `Modified`（自动保存走这条，不重扫）；
- 抽出纯函数 `fn classify_kind(kind: &EventKind) -> Option<WatchKind>` 便于单测，
  闭包里只调用它。

`src/ui/file_tree.rs`：

- `handle_watch_event` 的 `structure_changed` 纳入 `Renamed`（`:424`）。

`paths` 保持 `notify` 原样（`Both` 时为 `[旧, 新]`）。现有过滤器
（`watcher.rs:60`）会滤掉已不存在的旧目录路径，新路径通过即可触发刷新；
具体"删哪个、加哪个"交给随后的全量扫描 + 合并来纠正，**不依赖 from/to 配对**——
避免不同平台（FSEvents / inotify）事件语义差异带来的复杂度。

### 3.5 被否决的方案

| 方案 | 否决理由 |
|------|----------|
| A. 自己维护 `HashSet<PathBuf>` 展开集合，刷新后逐个 `expanded(true)` | 需要在重建 `TreeItem` 时设置，本质与合并同一件事，但多一份可能与 UI 不同步的状态；合并复用 `TreeItem` 就不需要第二份状态 |
| B. 升级 / 替换 `gpui-component` 的 Tree | 违反"只依赖 gpui-kit 0.6"铁律，代价高 |
| C. 改第三方源码让 `set_items` 不清选中 | 不允许改依赖源码；改为上层恢复 |
| D. 只按 watcher 事件做增量、完全不重扫 | 见 N1：需要与磁盘保持最终一致，正确性风险高 |

---

## 4. 数据模型与持久化

- **新增（仅内存）**：`FilesPanel.tree_items: Vec<TreeItem>`，保存上一次交给
  `set_items` 的树根。当前只有一个根，仍用 `Vec` 以匹配 `set_items(impl Into<Vec<TreeItem>>)`
  的签名。
- **不变**：`dir_flags` 仍由新扫描结果整体重建（`file_tree.rs:680-682`）；排序仍是
  "目录优先 + 名称小写序"（`core/vault.rs:154-158`）。
- **持久化**：无新增文件；`layout.json` / `workspace.json` / `open_notes.json` 均不受影响。
- **旧数据迁移**：不涉及。

---

## 5. 交互流程（以删除为例）

1. 用户在树里选中 `项目/B/笔记.md`，右键删除 → `delete_item`（`:181-205`）；
2. `vault.remove()` 完成后调用 `refresh(window, cx)`（`:199`）；
3. `refresh` 在后台线程 `scan_root` 得到新 `FileNode`（`:488-490`）；
4. 回到主线程 `apply_tree`：
   - 记下 `selected_path` 与滚动偏移；
   - `merge_tree_item(&self.tree_items[0], &new_root)`：
     `项目/A/`（路径未变）复用旧项 → **保持展开**；`项目/B/` 的 children 里少了
     `笔记.md` → 该层重建，其余兄弟不变；
   - `set_items` + 复位选中（目标已删除 ⇒ `None`）+ 复位滚动偏移；
   - 更新 `dir_flags` 与 `status`；
5. 用户视角：树不收缩，滚动条不动，只有那一行消失。

---

## 6. 异常与边界

| 场景 | 处理 |
|------|------|
| 首次加载 / 切换库 | 旧根 id 与新根不同 → 整树重建、根展开，与现状一致 |
| 库根目录本身被改名 | watcher 报 `Renamed`，`active_root` 守卫会重新 `Vault::open`；根 id 变化 → 整树重建（此时整体刷新是合理的） |
| 删除当前选中项 | 选中清空（同现状），见 Q1 |
| 同层仅大小写不同的重名 | id 不同 → 该节点重建，其余不变 |
| 扫描失败 | 只更新 status，**保留旧 `tree_items` 不动** → 树不会消失（现状：同样只更新 status，行为一致） |
| `show_all_files` 配置切换 | 仍走 `refresh` → 合并后可见性变化体现为节点增删，展开状态保留 |
| 外部批量写入（构建工具等） | 每次事件触发一次后台扫描，与现状一致；去抖见 Q3 |
| 目录被整体删除后再重建同名 | id 相同但已被移除过 → 走"未找到旧项"分支，新建（展开状态重置，符合"新目录"语义） |

---

## 7. 测试计划

### 7.1 自动化（`tests/ui/file_tree.rs`，纯逻辑，无需 GPUI 运行时）

> 注意沿用该文件既有约定：**不能** `use super::*`（`use gpui_kit::*` 会遮蔽 `#[test]`），
> 需显式 `use super::{FileNode, TreeItem, merge_tree_item, to_tree_item, ...}`。

| # | 用例 | 断言 |
|---|------|------|
| t1 | 新增子节点 | 其他兄弟的 `is_expanded()` 保持；新节点存在且默认折叠 |
| t2 | 删除子节点 | 其他兄弟的展开状态保持；被删节点不在新树中 |
| t3 | 重命名（id 变化） | 该节点被重建（展开状态重置），兄弟与祖先状态不变；新 id 出现、旧 id 消失 |
| t4 | 深层（≥3 层）兄弟变更 | 深层目录的展开状态保持 |
| t5 | 根 id 不同 | 返回全新 item，且根 `is_expanded() == true` |
| t6 | 子节点顺序 | 与 `FileNode` 的顺序一致（目录优先 + 名称序） |
| t7 | 展开状态继承的底层机制 | `old.clone()` 后改 `children`，`old` 的展开状态不受影响（锁定 `Rc` 共享语义） |
| t8 | label 更新 | 大小写重命名时新树 label 与新 `FileNode.name` 一致 |

### 7.2 自动化（`tests/core/watcher.rs`）

| # | 用例 | 断言 |
|---|------|------|
| t9 | `Modify(Name(Both | From | To | Any))` | → `Some(WatchKind::Renamed)` |
| t10 | `Modify(Data(Any))` / `Modify(Metadata(Any))` | → `Some(WatchKind::Modified)`（不触发重扫） |
| t11 | `Create(Any)` / `Remove(Any)` | → `Created` / `Removed`（回归） |
| t12 | `Access(Any)` / `Other` | → `None` |

### 7.3 不自动化（依赖真实窗口与滚动）

滚动偏移的保持、`set_items` 后的选中复位涉及 `uniform_list` 的布局时机，
用 GPUI 测试 harness 成本高于收益 → 走人工验收（§8）。

---

## 8. 人工验收清单

准备：一个含 ≥3 层目录、每层 ≥3 项的库；展开到 4 层；滚动到树中部。

- [ ] **删除**：选中某文件删除 → 树不收缩，滚动位置不动，该行消失
- [ ] **删除**：选中某目录删除 → 其余目录展开状态不变
- [ ] **新建笔记 / 新建目录** → 新节点出现在正确位置，其余展开状态不变，不回顶
- [ ] **重命名（工具栏 / F2）** → 名称就地更新，其余展开状态与滚动不变
- [ ] **移动到其他目录** → 源目录少一项、目标目录多一项，两边其余状态不变
- [ ] **拖拽移动** → 同上
- [ ] **粘贴截图** → 新文件出现在目标目录，树不收缩
- [ ] **从访达拖入目录** → 导入后局部更新，其余不变
- [ ] **选中保持**：展开 `A/`，选中 `A/x.md`，在 `B/` 下新建文件 → `A/x.md` 仍为选中项
- [ ] **滚动保持**：滚动到中部后执行删除 → 视口不跳回顶部
- [ ] **手动刷新按钮** → 行为与现状一致（全量重建，根展开）
- [ ] **切换库** → 全新树、根展开、旧库标签关闭（现状不变）
- [ ] **外部新增**：在访达里新建 `a.md` → 树里自动出现，其余不变
- [ ] **外部删除**：在访达里删掉 `a.md` → 树里自动消失，其余不变
- [ ] **外部重命名**：在访达里把 `a.md` 改为 `b.md` → 树里自动更新（本设计前不刷新）
- [ ] **外部重命名目录**：在访达里给目录改名 → 树里自动更新，其展开状态重置，兄弟不变
- [ ] **编辑不抖动**：连续输入并自动保存 30 秒 → 树无重扫（`Modified` 不触发刷新）
- [ ] **扫描失败兜底**：把库目录权限改成只读后刷新 → 树不消失，status 报错

---

## 9. 风险与开放问题

### 风险

| # | 风险 | 缓解 |
|---|------|------|
| R1 | 直接读写 `TreeItem.children`（pub 字段，`tree.rs:44`）而非 API | 加注释说明依据并锁定 gpui-kit 0.6；用 t1–t8 单测覆盖，升级时第一时间暴露 |
| R2 | 通过 `UniformListScrollHandle.0.borrow().base_handle` 读写偏移（pub 元组字段，`uniform_list.rs:80`） | 同上；且合并后条目数稳定，兜底路径极少触发 |
| R3 | `set_offset` 写的是共享 handle，下一帧 prepaint 仍可能夹取 | 合并后 `entries` 不再塌缩；人工验收"滚动保持"逐条确认 |
| R4 | 复用旧 `TreeItem` 后，若磁盘内容与 id 不一致（如路径规范化差异）可能匹配失败 | id 由 `node.path` 生成，两端同源；匹配失败退化为"重建该节点"，最坏情况等同现状 |

### 需用户拍板

- **Q1**：删除当前选中项后，是否自动选中"相邻节点"（上一个兄弟 / 父目录）？
  默认 **不做**（保持现状：清空选中）。做的话工具栏与右键菜单在连续删除时更顺手。
- **Q2**：是否把 watcher 的 kind 映射抽成纯函数 `classify_kind`（`&EventKind -> Option<WatchKind>`）
  以便单测？建议 **做**，成本极低。
- **Q3**：是否在本期一并给监听事件加 100–200ms 去抖（外部批量写入时只重扫一次）？
  默认 **不做**（N5），另立议题。
- **Q4**：根节点是否仍强制展开？本设计改为**保留用户状态**（首次 / 切库才强制）。
  若希望"根永远展开"，请指出。

---

## 10. 实施步骤

| 期 | 内容 | 产出 | 可独立验证 |
|----|------|------|-----------|
| **P1** | `FilesPanel.tree_items` 字段 + `merge_tree_item` + `apply_tree` 改为合并、选中复位、滚动偏移保存/恢复 | `src/ui/file_tree.rs`、`tests/ui/file_tree.rs`（t1–t8） | 是：G1 / G2 / G3，人工清单前 12 项 |
| **P2** | `WatchKind::Renamed` + `classify_kind` + `structure_changed` 纳入重命名 | `src/core/watcher.rs`、`src/ui/file_tree.rs`、`tests/core/watcher.rs`（t9–t12） | 是：G4，人工清单"外部重命名"2 项 |
| **P3**（可选，待 Q3） | 监听事件去抖 | `src/ui/file_tree.rs` | 是 |

每期结束各自跑：`cargo test`、`cargo clippy --all-targets`、`cargo fmt`。
