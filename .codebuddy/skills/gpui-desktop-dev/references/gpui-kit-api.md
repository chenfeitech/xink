# gpui-kit API 速查

> 版本基线：`gpui-kit` 0.6.4（内含 `gpui-pre` 0.3.5 = `zed@d89e9c2`）。
> API 可能随版本变动，**使用前以 <https://gpui-kit.com> 与 `docs.rs/gpui-kit` 为准**。

## 导入约定

```rust
use gpui_kit::*;                 // div(), Render, Window, Context, Entity ...
use gpui_kit::component::*;      // Root, Button, Tree, Editor, Textarea, DockArea ...
use gpui_kit::component::button::*;
use gpui_kit::base::{TextView, TextViewState};
```

## 应用启动

```rust
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|_| AppView::new());
                    cx.new(|cx| Root::new(view, window, cx))
                }).expect("open window");
            }).detach();
        });
}
```

分层：`gpui-kit`（门面）→ `gpui-base`（无样式行为层）+ `gpui-component`（成品组件）。
只想要行为层可关掉默认 feature，只依赖 `gpui-base`。

## Markdown / HTML 渲染（gpui-base）

```rust
// 一次性渲染
TextView::markdown("preview-id", markdown_source)
    .scrollable(true)
    .style(TextViewStyle::default());

// 需要持续更新（本项目预览区用这个）
let state = cx.new(|cx| TextViewState::markdown(initial_source, cx));
TextView::new(&state).scrollable(true);
state.update(cx, |s, cx| s.set_text(updated_source, cx));
```

要点：
- `set_text()` 是**增量更新**：只重算变动的块，不重解析整个文档，也保留选区
- `scrollable(true)` 填满容器并垂直滚动；未设置则随内容增长
- 语法高亮默认关闭，需自行提供：
  ```rust
  TextView::markdown("id", src).code_block_highlighter(|block| {
      my_highlighter(block.lang(), block.code())
          .into_iter()
          .map(|(range, color)| (range, HighlightStyle { color: Some(color), ..Default::default() }))
          .collect()
  })
  ```
- YAML frontmatter 默认关闭，需 `MarkdownExtensions::default().frontmatter()`
- 自定义语法用插件：实现 `MarkdownPlugin`，通过 `.plugin(...)` 注册（双链/公式走这条）
- 数学公式 `$\dots$` 默认会解析为节点；无插件认领时按原始文本渲染

## 编辑

- `Textarea` —— 普通多行文本（Markdown 源码编辑的 MVP 选择）
- `Editor` —— 源代码编辑器，tree-sitter 高亮 + LSP 诊断/补全/hover，200K 行稳定
- `Input` —— 单行输入，支持校验、输入掩码、前后缀

语法高亮需要启用 feature：`gpui-component/tree-sitter` 及具体语言
（如 `tree-sitter-md`、`tree-sitter-rust`）。**只启用需要的语言**，别开全量。

## 布局（DockArea）

可拖拽面板、可拖标签、嵌套分屏、边缘停靠，且**布局可序列化**——
正好用于本项目"重启恢复布局"需求。

```bash
cargo run -p example-dock     # 官方示例（在 gpui-kit 仓库内）
```

## 目录树（Tree）

`gpui_component::Tree` 提供层级数据展示，配合懒加载可用于大目录。

## 文件对话框

字段是 `prompt`，不是 `prompt_label`：

```rust
let prompt = cx.prompt_for_paths(PathPromptOptions {
    files: false,
    directories: true,
    multiple: false,
    prompt: Some("选择笔记库".into()),
});

// 新建/另存为：
let prompt = cx.prompt_for_new_path(&dir, Some("未命名.md"));
```

两者都返回 oneshot channel，**结果是双层 Result**（外层通道取消，内层平台调用结果）：

```rust
match prompt.await {
    Ok(Ok(Some(paths))) => { /* 用户选中 */ }
    Ok(Ok(None))        => { /* 用户取消 */ }
    Ok(Err(err))        => { /* 平台调用失败 */ }
    Err(_)              => { /* 通道被取消 */ }
}
```

## 目录树（Tree）

```rust
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tree::{tree, TreeItem, TreeState};

// 状态：Entity<TreeState>
let tree_state = cx.new(|cx| TreeState::new(cx));

// 数据：TreeItem::new(id, label)，可 .child(..) / .children(..) / .expanded(bool)
// 约定：id 用文件路径，保证稳定
let items = vec![TreeItem::new("/a/b.md", "b.md")];
tree_state.update(cx, |state, cx| state.set_items(items, cx));

// 渲染
tree(&tree_state, move |ix, entry, selected, window, cx| {
    let item = entry.item();          // &TreeItem，字段 id/label/children 均为 pub
    let state = tree_state.clone();   // Fn 闭包不能 move 捕获，需在内部 clone
    ListItem::new(("item", ix))
        .child(item.label.to_string())
        .on_click(move |_, _, cx| {
            state.update(cx, |s, cx| s.set_selected_index(Some(ix), cx));
        })
})
```

`TreeState` 还提供 `selected_item()`、`set_selected_item()`、`entry(ix)`、`scroll_handle()`，
并 emit `TreeEvent::{Expanded, Collapsed}`。
注意：`TreeState` 需要完整树数据才能渲染展开箭头，因此**不支持子目录懒加载**。

## 图标

默认 `assets` feature 内置 Lucide 图标集，必须传给 application：

```rust
gpui_kit::application().with_assets(gpui_kit::assets::Assets)
```

自定义图标则关掉该 feature，并按 `IconName` 的命名提供 SVG。

## 官方示例（在 gpui-kit 仓库内）

| 命令 | 内容 |
|---|---|
| `cargo run -p example-dock` | 停靠布局 |
| `cargo run -p example-markdown` | Markdown 渲染 |
| `cargo run -p example-html` | HTML 渲染 |
| `cargo run -p example-editor` | 编辑器 + LSP |
| `cargo run -p hello_world` | 最小骨架 |
