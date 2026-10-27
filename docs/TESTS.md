# 测试代码放在哪里

## 目录约定

测试代码**不**与被测源码同目录，统一放在仓库根的 `tests/`，目录结构镜像 `src/`：

```
src/core/datafile.rs        →  tests/core/datafile.rs
src/core/config.rs          →  tests/core/config.rs
src/ui/editor.rs            →  tests/ui/editor.rs
src/state/workspace.rs      →  tests/state/workspace.rs
```

新增测试时：`src/<层>/<模块>.rs` 的测试放 `tests/<层>/<模块>.rs`。

## 怎么挂上去：用 `#[path]`

在被测文件末尾这样声明：

```rust
// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/datafile.rs"]
mod tests;
```

两点必须知道：

1. **`#[path]` 的基准是本文件所在目录**（如 `src/core/`），不是"模块名目录"。
   因此所有声明都是 `../../tests/...` 两级回退。
2. **模块树位置没变**——`tests` 仍是 `datafile` 的子模块。所以测试文件里的
   `use super::*;` 照旧指向被测模块，**测试代码本身不需要因为搬迁而改动**。

## 为什么不用 Cargo 的集成测试目录

`tests/` 在 Cargo 里默认是**集成测试目录**：`tests/*.rs` 会被当作独立的测试 crate
去 `use xink::…`。但 xink 是 **bin crate**（只有 `src/main.rs`，没有 lib target），
集成测试根本无法引用它。何况集成测试只能访问 `pub` 项，而现有测试大量依赖
私有函数（`is_temp`、`tab_key`、`data_preview_source` …），真要搬过去就得把它们
全部提为 `pub`，白白破坏封装。

用 `#[path]` 是两全的做法：物理上集中到 `tests/`，逻辑上仍是 crate 内的单元测试，
能测私有项，且只编译**一个**测试二进制（不会每个测试文件各链接一遍 gpui）。

配套措施：`Cargo.toml` 里显式 `autotests = false`，防止有人在 `tests/` 根目录
落文件后被当成集成测试 target 而编译失败。

## 常用命令

```bash
cargo test                      # 跑全部
cargo test datafile             # 按名字过滤
cargo test core::               # 只跑 core 层
```

## 覆盖现状：UI 层能测什么

**结论：渲染与一切需要 `App` / `Window` / `Entity` 的东西都测不了。**
全仓从未用过 `TestAppContext` / `VisualTestContext`，所以 UI 层只有两类东西能测：

1. **查表类的静态数据**（设置的元数据表、抽屉的 id / 标签 / SVG）——写错了
   在界面上只表现为"图标不显示""某一行不见了"，很容易漏；
2. **纯函数与纯状态迁移**（路径换算、行→比例、高度算术、开合迁移）。

### 已覆盖（`src/ui/` 下 12 个模块有挂载）

`ai` `command` `completion` `drawer` `editor` `file_tree` `outline`
`preview` `rail` `settings` `theme` `vault_manager`

`completion` / `drawer` / `preview` / `rail` / `vault_manager` 这 5 个是 2026-10-27 补的，
测的都是上面两类；其中 `rail` 的 `next_opened`、以及顺手修正的
`completion::link_target` 注释（原文写"优先文件名"，实现其实是"库内一律相对路径"）
都是为了把逻辑从 `Context<Self>` 里掏出来才顺带做的。

### 未覆盖（4 个，都有原因，不要硬凑测试）

| 模块 | 为什么没测 |
|---|---|
| `backlink.rs` | 唯一的字符串逻辑（标题回落到 `file_stem`）在渲染闭包里；数据侧 `index.backlinks()` 已由 `tests/core/index.rs` 覆盖 |
| `search.rs` | 命中片段 / 截断在 `core/search.rs`（已有 `tests/core/search.rs`）；UI 侧只有 `Context<Self>` + `background_spawn` |
| `tabs.rs` | 标签去重 / 激活切换是纯状态机，但被 `panels: HashMap<_, WeakEntity<EditorPanel>>` 卡住——`insert` 是唯一入口且必须传 `&Entity`。要测就得先拆一个 `push_key`，属于为测试而重构，暂不做 |
| `tags.rs` | 状态迁移全在 `cx.listener` 闭包里；数据侧 `index.tags()` / `notes_with_tag()` 已由 `tests/core/index.rs` 覆盖 |

> 真要给后两个补，成本最低的抽法分别是 `tabs::push_key` 与
> `tags::toggle_selected(current: Option<&str>, tag: &str) -> Option<String>`，
> 但这两个都是"仅为可测性"的重构，等它们真出过 bug 再做更划算。

### 还有两类"测不了"，别硬写

1. **写真实 `app_data_dir()` 的**（`state/ai_sessions.rs`、`state/workspace.rs` 的
   `save_open_notes` / `load_open_notes`）：测试一旦跑起来就会**覆盖用户正打开着的
   标签与 AI 会话历史**。做法是**把其中的纯规则抽出来测**（已抽 `apply_note`、
   `normalize_open_notes` / `active_within`），函数本身留给人工验收。
2. **发真实 HTTP 的**（`core/ai/probe.rs` 的 `probe_model`）：同理，抽出
   `probe_outcome` / `probe_timeout` 覆盖判定与超时下限。

抽函数只做**原样搬移**（3~15 行、行为不变），不为测试改设计。

### 另有 12 节只能靠人眼

滚动手感、着色对不对、气泡靠哪边……这些自动化不了，统一收在
**[ACCEPTANCE.md](./ACCEPTANCE.md)**，别再散落到各设计文档里。
