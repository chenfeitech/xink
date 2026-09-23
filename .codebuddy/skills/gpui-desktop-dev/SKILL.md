---
name: gpui-desktop-dev
description: 使用 GPUI / gpui-kit 开发 Rust 桌面应用的专家技能。当需要搭建 GPUI 项目骨架、初始化窗口、使用组件（Tree/Editor/Textarea/DockArea/TextView）、绑定快捷键与 Action、处理异步任务与后台线程、排查 GPUI 编译或运行时错误时使用。触发关键词：GPUI、gpui、gpui-kit、gpui-component、Rust 桌面应用、窗口初始化、组件使用、编译报错。
---

# GPUI 桌面应用开发

## 角色

GPUI / gpui-kit 桌面开发专家。GPUI 是 Zed 编辑器的 UI 框架，**pre-1.0、文档不全、版本间有 breaking change**，
因此本技能强调：**先对齐真实 API，再动手写代码**；不凭记忆猜 API。

## 核心原则

1. **依赖只用 `gpui-kit`**，不直接依赖 `gpui`（详见 `rules/01-gpui-stack.md`）
2. **不猜 API** —— 遇到不确定的组件/方法，先查 <https://gpui-kit.com> 或 `docs.rs/gpui-kit`
3. **先跑通再扩展** —— GPUI 编译重，任何大改动前先确保当前代码能编译
4. **编译慢是常态** —— 首次全量编译在分钟级，配好链接器加速（见下）

## 一、环境准备（macOS）

GPUI 用 Metal 渲染，需要**完整 Xcode**（不只是 Command Line Tools）：

```bash
# 1. App Store 安装 Xcode，首次启动安装 macOS 组件
xcode-select --install
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer

# 2. 验证
xcode-select -p     # 应指向 /Applications/Xcode.app/Contents/Developer
```

Linux 需要 `wayland` 或 `x11` feature；Windows 无需额外 feature。

## 二、新建项目 SOP

### 1. Cargo.toml

```toml
[package]
name = "ink"
edition = "2024"
rust-version = "1.98"

[dependencies]
gpui-kit = "0.6"
```

### 2. 编译加速（建议一开始就配）

`.cargo/config.toml`：

```toml
[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=/opt/homebrew/opt/llvm/bin/ld64.lld"]

[target.aarch64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=/opt/homebrew/opt/llvm/bin/ld64.lld"]
```

需先 `brew install llvm`。Linux 用 `-fuse-ld=lld` 或 `mold`。

### 3. 最小可运行骨架

```rust
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;

pub struct HelloWorld;

impl Render for HelloWorld {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .gap_2()
            .size_full()
            .items_center()
            .justify_center()
            .child("Hello, World!")
            .child(
                Button::new("ok")
                    .primary()
                    .label("Let's Go!")
                    .on_click(|_, _, _| println!("Clicked!")),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)   // Lucide 图标集
        .run(move |cx| {
            gpui_kit::init(cx);                  // 必须在用组件前调用

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|_| HelloWorld);
                    cx.new(|cx| Root::new(view, window, cx))   // 第一层必须是 Root
                })
                .expect("failed to open window");
            })
            .detach();
        });
}
```

**三个易错点**：漏 `gpui_kit::init(cx)`、窗口根不是 `Root`、忘记 `.detach()`。

## 三、核心机制速查

| 机制 | 用法 |
|---|---|
| 状态容器 | `Entity<T>`，通过 `cx.new()` 创建，`entity.read(cx)` / `entity.update(cx, ...)` |
| 渲染 | `impl Render for T { fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) }` |
| 触发重绘 | `cx.notify()` |
| 事件 | `impl EventEmitter<MyEvent> for T`，发射 `cx.emit(event)`，订阅 `cx.subscribe(&entity, ...)` |
| 全局状态 | `impl Global for T` |
| 快捷键 | `gpui::actions!(name, [ActionName])` + `cx.bind_keys([...])` |
| 后台任务 | `cx.background_spawn(async move { ... })`，回主线程用 `cx.update()` |
| 定时器 | `cx.spawn(async move |cx| { cx.background_executor().timer(Duration).await; ... })` |
| 文件对话框 | `cx.prompt_for_paths(PathPromptOptions { ... })` |
| 测试 | `#[gpui::test] fn t(cx: &mut TestAppContext)` |

## 四、组件速查

| 需求 | 组件 / API |
|---|---|
| 目录树 | `Tree` |
| 源码编辑（tree-sitter 高亮） | `Editor` |
| 普通多行文本 | `Textarea` |
| 单行输入 | `Input` |
| 面板 / 分屏 / 标签布局 | `DockArea`（可序列化持久化） |
| Markdown / HTML 渲染 | `gpui_base::TextView::markdown()` |
| 按钮 | `Button` |
| 图标 | `Icon`（Lucide，需 `with_assets`） |
| 虚拟列表 | `uniform_list` / `list` |

详细 API 片段见 `references/gpui-kit-api.md`。

## 五、常见错误与排错

完整清单见 `references/pitfalls.md`。高频三条：

1. **编译报类型不匹配，且涉及 `Rope` / `ropey`** → 版本没对齐，改成 `= "2.0.0-beta.1"`
2. **窗口白屏 / 组件样式全丢** → 窗口根元素不是 `Root`，或漏了 `gpui_kit::init(cx)`
3. **图标不显示** → 没调 `.with_assets(gpui_kit::assets::Assets)`

## 六、提速建议

安装官方 AI 技能包，可直接获得组件目录与编码规范：

```bash
npx skills add longbridge/gpui-kit
```

## 参考

- 文档 <https://gpui-kit.com>（中文：`/zh-CN/` 前缀）
- 仓库 <https://github.com/longbridge/gpui-kit>（`examples/` 下有 dock / markdown / editor 等可运行示例）
- API <https://docs.rs/gpui-kit>
