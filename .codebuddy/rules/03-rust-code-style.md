---
description: Rust 编码规范与错误处理约定。在编写或修改本项目任何 Rust 代码时遵守本规则。
globs: ["src/**/*.rs", "tests/**/*.rs"]
---

# Rust 编码规范

## 1. 版本与格式

- edition **2024**，`rust-version = "1.98"`
- 提交前必须 `cargo fmt` 与 `cargo clippy --all-targets` 无 warning
- 不要为了消除 clippy 告警而牺牲可读性；确需忽略时加 `#[allow(...)]` 并写清原因

## 2. 错误处理

| 场景 | 做法 |
|---|---|
| 应用代码（main / ui） | `anyhow::Result`，用 `?` 与 `.context("...")` |
| 库/领域层（`core/`） | 定义 `thiserror` 错误枚举，让调用方可匹配 |

```rust
// core/ 中：可匹配的错误类型
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("笔记不存在: {0}")]
    NotFound(PathBuf),
    #[error("IO 失败: {0}")]
    Io(#[from] std::io::Error),
}

// ui/ 中：用 anyhow 带上下文
let note = load(path).context("加载笔记失败")?;
```

**禁止**：
- 在 `core/` 里用 `.unwrap()` / `.expect()`（测试代码除外）
- 用 `panic!` 处理可预期的业务错误
- 吞掉错误（`let _ = ...`）而不加注释说明原因

## 3. 命名

- 类型 / Trait：`UpperCamelCase`
- 函数 / 变量 / 模块：`snake_case`
- 常量：`SCREAMING_SNAKE_CASE`
- 布尔字段用 `is_` / `has_` / `should_` 前缀（`dirty` 除外——领域惯用名保留）
- 路径相关变量统一用 `path`（`PathBuf`）与 `path_str`，不要用 `file`、`f` 这类含糊名

## 4. GPUI 特有约定

- `Render::render` 中不要做 IO 或重计算，只做读状态 + 构建元素树
- 需要重绘时用 `cx.notify()`，不要手动强制刷新
- 事件用 `EventEmitter<T>` 定义与发射，订阅方用 `cx.subscribe()`
- 需要后台任务用 `cx.background_spawn()`，结果回到主线程用 `cx.update()`
- 元素 id 用稳定的字符串（如文件路径），不要用会变化的值，否则状态会错乱

## 5. 注释与文档

- 公共 API（`pub fn` / `pub struct`）写 `///` 文档注释，说明**做什么**与**为何存在**
- 复杂算法/非直觉的取舍写 `//` 行内注释说明**为什么**，不要复述代码在做什么
- 代码本身不写解释性旁白注释

## 6. 测试

- `core/` 的每个模块都要有 `#[cfg(test)] mod tests`
- 测试名用 `should_xxx_when_xxx` 或在注释中写清场景
- UI 交互测试用 `gpui::test` 宏 + `TestAppContext`
- 涉及文件系统的测试用临时目录（`tempfile`），**绝不**污染真实用户目录
- 测试**写什么、测到什么程度**见 `09-testing-standard.md`
- **提交前必须跑的四条命令**（build / clippy / fmt / test）见 `07-quality-gates.md` G4

## 7. 依赖引入

- 每加一个新依赖前，先确认 `gpui-kit` / `gpui-component` 是否已间接引入（查 `cargo tree`）
- 能复用就复用，避免同一 crate 多版本共存
- 优先选纯 Rust、无 C 依赖的 crate（缩短编译时间）
