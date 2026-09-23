---
description: GPUI 技术栈与依赖管理的硬约束。在新建/修改 Cargo.toml、引入依赖、编写 GPUI 应用启动代码、使用任何 gpui 组件时，必须先遵守本规则。违反会直接导致编译失败。
---

# GPUI 技术栈铁律

## 1. 只依赖 `gpui-kit`，绝不直接依赖 `gpui`

```toml
[dependencies]
gpui-kit = "0.6"
```

**禁止**在 `Cargo.toml` 中出现：

```toml
gpui = "0.2.2"          # 禁止
gpui_platform = "*"     # 禁止
```

### 原因

GPUI 生态已分裂为两条互不相通的线：

| crate | 版本 | 状态 |
|---|---|---|
| `gpui` | 0.2.2（2025-10） | Zed 官方发布，已停滞一年，**生态组件库不用它** |
| `gpui-pre` | 0.3.5（2026-09） | `zed@d89e9c2` 快照，是活跃组件库实际使用的版本 |

`gpui-component` 0.6.4 依赖的是 `gpui-pre ^0.3.5`。两者 API 不兼容。
`gpui-kit` 会 pin 匹配的 GPUI 版本并 re-export，应用只列一个依赖即可杜绝版本错配。

代码里通过 `gpui_kit::*` 使用 GPUI 类型（`div()`、`Render`、`Window`、`Context` 等）。

## 2. 共享依赖必须精确对齐版本

`gpui-component` 内部**精确锁定**了 `ropey = "=2.0.0-beta.1"`。
本项目若使用 ropey，必须同样精确锁定：

```toml
ropey = "=2.0.0-beta.1"    # 正确
ropey = "2"                # 错误：会解析出第二份，类型不兼容，编译失败
```

同理，其他与组件库共享的依赖（`notify`、`markdown`、`serde` 等）先查
`gpui-component` 的版本要求再定，避免同一 crate 多版本共存。

判断方法：改动依赖后运行 `cargo tree -d`，检查是否有重复版本。

## 3. 应用启动骨架（固定写法）

```rust
use gpui_kit::component::*;
use gpui_kit::*;

fn main() {
    gpui_kit::application()
        // 使用默认 Lucide 图标集
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            // 必须在使用任何组件前调用
            gpui_kit::init(cx);

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|_| AppView::new());
                    // 窗口第一层必须是 Root
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("failed to open window");
            })
            .detach();
        });
}
```

三条不可省略：
- `gpui_kit::init(cx)` 必须在用组件之前
- 窗口根元素必须是 `Root::new(view, window, cx)`
- `Render` 的实现签名是 `fn render(&mut self, window: &mut Window, cx: &mut Context<Self>)`

## 4. 升级策略

- `gpui-pre` 是 Zed main 分支的**快照**，不是语义化版本，升级必须跟随 `gpui-kit` 的发布节奏
- **不要**随手 `cargo update`；`Cargo.lock` 必须提交进版本库
- 升级 `gpui-kit` 时按 minor 版本逐个升，每次升完跑通编译再继续

## 5. 组件选择

优先复用现成组件，不要从 `div()` 手搓：

| 需求 | 组件 |
|---|---|
| 目录树 | `Tree` |
| 源码编辑（带高亮） | `Editor` |
| 普通多行文本 | `Textarea` |
| 单行输入 | `Input` |
| 面板/分屏布局 | `DockArea`（可序列化） |
| Markdown / HTML 渲染 | `gpui_base::TextView` |
| 按钮 | `Button` |

不确定某个能力是否存在时，先查 <https://gpui-kit.com> 再决定。
