# GPUI 开发坑位清单

## 生态与版本

### 1. `gpui` 与 `gpui-pre` 是两条线，不能混用
- crates.io 的 `gpui = "0.2.2"`（2025-10，Zed 官方，已停滞）
- crates.io 的 `gpui-pre = "0.3.5"`（2026-09，`zed@d89e9c2` 快照，生态在用）
- `gpui-component` 依赖的是 **gpui-pre**

**症状**：奇怪的类型不匹配、trait 未实现、方法不存在。
**对策**：应用只依赖 `gpui-kit`，让它 pin 版本。

### 2. 共享依赖版本没对齐 → 多版本共存
`gpui-component` 精确锁定 `ropey = "=2.0.0-beta.1"`。本项目写 `ropey = "2"` 会解析出另一份，
两边的 `Rope` 是不同类型。

**对策**：所有与组件库共享的依赖都对齐版本；改动依赖后跑 `cargo tree -d` 查重。

### 3. 随手 `cargo update` 导致框架漂移
`gpui-pre` 是 Zed main 的快照，不是语义化版本。

**对策**：`Cargo.lock` 必须入库；升级跟随 `gpui-kit` 发布节奏，按 minor 逐个升。

## 编译

### 4. macOS 报 Metal / shader 相关错误
**原因**：只装了 Command Line Tools，没装完整 Xcode。
**对策**：安装 Xcode 并 `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer`。

### 5. 首次编译极慢
blade-graphics / tree-sitter / resvg / naga 都要编译，分钟级属正常。

**对策**：
- 配 lld/mold 链接器（见 SKILL.md 二.2）
- `[profile.dev] debug = "limited"`、`split-debuginfo = "unpacked"`
- 对重依赖单独 `opt-level = 3`（gpui-kit 官方就这么做）

### 6. 二进制体积膨胀
**对策**：只启用需要的 `tree-sitter-<lang>` feature，不要开 `tree-sitter-languages` 全量。

## 运行时

### 7. 窗口白屏 / 样式全丢
**原因**：窗口根元素不是 `Root::new(view, window, cx)`。
**对策**：`open_window` 返回的必须是 `Root` 包裹的视图。

### 8. 组件无样式 / 主题未生效
**原因**：漏了 `gpui_kit::init(cx)`。
**对策**：在 `run()` 回调开头调用，且必须在任何组件使用之前。

### 9. 图标不显示
**对策**：`gpui_kit::application().with_assets(gpui_kit::assets::Assets)`。

### 10. 界面卡死 / 掉帧
**原因**：在 `render()` 或主线程做了 IO、全库扫描、索引构建。
**对策**：这些一律放 `cx.background_spawn()`；`render()` 只读状态 + 建元素树。

### 11. 状态改了但界面不更新
**对策**：改完 `Entity` 状态要 `cx.notify()`。

### 12. 元素状态错乱（滚动位置、展开态丢失）
**原因**：元素 id 用了会变化的值（如索引、临时 id）。
**对策**：用稳定 id，本项目统一用**文件路径**。

### 13. `cx.spawn` 的任务没执行
**原因**：忘了 `.detach()`。
**对策**：`cx.spawn(...).detach();`（除非显式持有 `Task`）。

### 14. 后台任务里访问 UI 类型 panic
**对策**：后台只做纯计算/IO，结果通过 `cx.update()` 回到主线程再操作 Entity。

## 文本与编辑器

### 15. CJK / IME 输入异常
**对策**：不要自己处理按键序列，用 gpui 的 `EntityInputHandler` 或组件库 `Textarea`/`Editor`。

### 16. 大文件编辑卡顿
**原因**：用 `String` 存全文反复拷贝。
**对策**：用 `Rope`（ropey）。

### 17. Markdown 预览每次编辑都整体闪烁
**对策**：用 `TextViewState::set_text()` 增量更新，不要重建 `TextView` 或换 id。

### 18. `prompt_for_paths` 的 await 结果是两层 Result
写成 `if let Ok(Some(path)) = prompt.await` 会报类型不匹配。
oneshot 外层是通道取消，内层才是平台结果。

**对策**：匹配 `Ok(Ok(Some(path)))`，并单独处理 `Ok(Err(err))` 与 `Err(_)`。

### 19. `ListItem` 不在 `gpui_kit::component` 根下
**对策**：用 `gpui_kit::component::list::ListItem`。

### 20. 渲染回调是 `Fn`，不能 move 捕获的 `Entity`
`tree()` / `uniform_list()` 的 render 回调是 `Fn`（会被多次调用），
闭包内若 `move` 一个 `Entity` 到 `on_click` 里会报 E0507。

**对策**：在回调内部先 `let state = entity.clone();`，再让 `on_click` move 这份克隆。

### 21. `PathPromptOptions` 的字段是 `prompt`
不是 `prompt_label`，类型是 `Option<SharedString>`。
