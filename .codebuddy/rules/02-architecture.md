---
description: 项目分层架构与模块边界规则。在新增模块、放置代码文件、设计 Entity、决定某段逻辑该放哪一层时遵守本规则。
globs: ["src/**/*.rs"]
---

# 分层架构与模块边界

## 1. 三层结构，依赖严格单向

```
ui/      视图层   渲染 + 交互，不含业务规则
   ↓
core/    领域层   纯业务逻辑，与 UI 解耦，可独立测试
   ↓
state/   持久化   序列化/反序列化
```

**铁律：`ui → core`。反向依赖、循环依赖一律禁止。**

## 2. `core/` 的边界（最重要）

`core/` 中**不得**出现：

- 任何 `gpui_kit::component::*` 组件类型（`Tree`、`Editor`、`Button` …）
- 任何布局/样式代码（`.flex()`、`.p_4()`、颜色）
- 任何窗口/视图概念（`Window`、`Render` 实现）

`core/` 中**可以**出现：

- GPUI 运行时类型：`Entity`、`App`、`Context`、`Task`、`EventEmitter`（用于状态与通信）
- 纯业务数据结构与算法

**判据**：如果这段逻辑能在一个 `#[test]` 里不创建窗口就跑通，它就该在 `core/`。

> 这条约束是为了让路径处理、索引构建、双链解析、原子写盘这些真正有 bug 风险的逻辑
> 可以用普通单元测试覆盖，不依赖 GUI。

## 3. 模块职责

| 模块 | 职责 | 不得承担 |
|---|---|---|
| `core/vault.rs` | 库根路径、文件枚举、增删改移动 | 渲染、树展开状态 |
| `core/note.rs` | Rope 内容、脏标记、保存、mtime | 编辑器光标/选区 |
| `core/markdown.rs` | 解析 AST → 大纲/双链/预览数据 | 具体渲染样式 |
| `core/index.rs` | 全库索引、搜索 | 磁盘写入 |
| `core/watcher.rs` | `notify` 事件 → 领域事件 | 弹窗 UI |
| `ui/*` | 渲染与交互 | 业务规则 |
| `state/*` | 序列化/反序列化 | 业务逻辑 |

## 4. 状态用 `Entity`，不要散落在视图里

跨组件共享的状态必须是 `Entity<T>`，通过 `cx.notify()` 驱动重绘：

```rust
// 正确：共享状态是 Entity
struct Workspace {
    vault: Entity<Vault>,
    open_tabs: Vec<Entity<Note>>,
}

// 错误：把业务状态塞进视图结构体再用 Rc<RefCell> 传来传去
```

主要 Entity：`Workspace`、`Vault`、`Note`、`NoteIndex`。

## 5. 数据流单向，禁止视图间互相直接调用

```
用户操作 → 修改 Entity 状态 → cx.notify() → 各视图重新 render
```

视图之间不直接调用彼此的方法；通过 `Entity` 读写 + `EventEmitter` 事件通信。

## 6. 异步规则

- 耗时操作（索引构建、全库扫描、文件 IO）放 `cx.background_spawn()`，
  **绝不**在 `render()` 或主线程中做
- `render()` 只做"读状态 + 构建元素树"，保持纯粹，不做 IO、不做重计算
