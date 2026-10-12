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
