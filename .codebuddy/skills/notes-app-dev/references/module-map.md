# xink 模块地图

## 依赖方向

```
ui/  ──→  core/  ──→  (无 UI 依赖)
  │          │
  └──────────┴──→  state/
```

`core/` 可以出现：GPUI 运行时类型（`Entity`、`App`、`Context`、`Task`、`EventEmitter`）
`core/` 不得出现：`gpui_kit::component::*` 组件、布局样式、窗口/视图概念

## core/ 领域层

### `vault.rs`
```rust
struct Vault {
    root: PathBuf,
    ignore: IgnoreRules,      // .git / node_modules / 隐藏文件
}
```
职责：打开/关闭库、枚举文件（返回路径树）、新建/删除/重命名/移动（含目录）。
不做：树展开状态、渲染。

### `note.rs`
```rust
struct Note {
    path: PathBuf,
    rope: Rope,               // 禁止用 String 存全文
    dirty: bool,
    disk_mtime: SystemTime,
}
```
职责：加载/保存（**原子写**：tmp + rename）、脏标记、外部变更比对。
不做：光标、选区、编辑器状态。

### `markdown.rs`
职责：解析 Markdown AST，产出三类派生数据：
- 预览数据（交给 `TextView`）
- 大纲（标题层级）
- 双链 `[[...]]` 与标签 `#tag`（**GFM 不含这些，需自行扩展解析**）

### `index.rs`
职责：全库索引（路径 → 标题 / 链接 / 标签），支撑搜索与补全。
约束：后台构建；只存派生数据，**绝不存正文**；可随时重建。

### `watcher.rs`
职责：`notify` 监听 → 转成领域事件（如 `ExternalChanged(PathBuf)`）。
约束：忽略应用自身写盘事件；忽略临时文件与忽略目录。
不做：弹窗（交给 `ui/`）。

## ui/ 视图层

| 文件 | 内容 | 主要组件 |
|---|---|---|
| `workspace.rs` | DockArea 布局、标签栏、窗口级状态 | `DockArea` |
| `file_tree.rs` | 目录树、懒加载、右键菜单 | `Tree` |
| `editor.rs` | 编辑区、防抖触发 | `Textarea`（MVP）→ `Editor`（P1） |
| `preview.rs` | 预览、滚动同步 | `gpui_base::TextView` + `TextViewState` |
| `command.rs` | 命令面板、搜索 | `Input` + 列表 |

## state/ 持久化

```rust
struct WorkspaceState {          // serde → workspace.json
    vault_root: Option<PathBuf>,
    recent_vaults: Vec<PathBuf>,
    open_tabs: Vec<PathBuf>,
    active_tab: usize,
    layout: Option<String>,      // DockArea 序列化结果
}
```

存储位置（macOS）：`~/Library/Application Support/ink/`
- `config.json` — 偏好设置
- `workspace.json` — 工作区状态
- `index/` — 索引缓存（可删）

## Entity 清单

| Entity | 持有者 | 作用 |
|---|---|---|
| `Workspace` | App | 顶层：当前库、打开的标签、布局 |
| `Vault` | Workspace | 库根路径与文件树快照 |
| `Note` | Workspace | 单篇笔记内容与状态 |
| `NoteIndex` | Vault | 全库索引 |
| `TextViewState` | 预览视图 | gpui-base 提供的预览文档状态 |

## 防抖基线

| 场景 | 时长 |
|---|---|
| 编辑 → 预览刷新 | 300ms |
| 编辑 → 自动保存 | 300ms |
| 编辑 → 索引重建 | 500ms |
| 搜索输入 → 查询 | 200ms |
