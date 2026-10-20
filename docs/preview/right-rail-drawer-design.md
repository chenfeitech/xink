# 设计：右侧按钮条 + 半透明抽屉（大纲 / 反链 / 标签）

## 1. 现状与问题

大纲、反链、标签目前是**右侧 dock 里的三个 tab**，与预览共用一块区域，靠 tab 切换：

| 事实 | 代码位置 |
|---|---|
| 右侧 dock 组装了 4 个面板：预览 / 大纲 / 反链 / 标签 | `src/app.rs:643-647` |
| 右侧 dock 固定宽 420px | `src/app.rs:657` |
| 三个面板都是 dock 面板（`BasePanel` + `Panel` + `Render`） | `src/ui/outline.rs:103-134`、`src/ui/backlink.rs:15-55`、`src/ui/tags.rs:14-55` |
| 三个面板注册为可恢复面板（布局文件里有则重建） | `src/main.rs:30-38` |
| 布局版本 `MAIN_DOCK_VERSION = 5`，版本不符就重建默认布局 | `src/app.rs:410`、`src/app.rs:463` |
| 根布局是纵向 `flex_col`：dock_area + 三个 layer | `src/app.rs:668-677` |
| 大纲点击已能跳行：`open_note_at(path, Some(line))` → `goto_line` | `src/ui/tabs.rs:189`、`src/ui/editor.rs:289` |

问题：三个面板挤在 dock 里，与预览争空间；用户要的是"窗口最右侧一条竖排按钮，点开一个贴边的半透明抽屉"。

## 2. 目标 / 非目标

**目标**

1. 窗口**最右侧**固定一条竖直按钮区（下称 rail），上下依次排列：大纲、反链、标签；
2. 点击任一按钮 → 从右侧滑出**半透明抽屉**，宽度较小（约 300px），**上下撑满整个界面**；
3. 大纲 / 反链 / 标签的内容都在该抽屉里展示，不再占 dock；
4. 大纲项点击后**定位到笔记对应行**（复用现有点跳链路）；
5. 抽屉有**收回按钮图标**，点击收回；点遮罩与 Esc 也能收回。

**非目标**

- 不改三个面板的数据来源（仍读 `AppState.outline` / `NoteIndex`）；
- 不做抽屉宽度记忆、不新增持久化文件（抽屉是瞬时 UI）；
- 不做 rail 位置拖拽、不做多抽屉并排；
- 不做毛玻璃（框架无 `backdrop_blur`）。

## 3. 方案概述

**自绘 rail + 框架 `Sheet` 承载抽屉。**

| 结论 | 依据 |
|---|---|
| 抽屉用 `Root::open_sheet_at(Placement::Right, build, window, cx)` | `gpui-component-0.6.4/src/root.rs:379`；`Placement::Right` 定义于 `gpui-base-0.6.4/src/geometry.rs:19` |
| `Placement::Right` 渲染为 `.top(top).right_0().bottom_0()`，天然贴右、撑满下方 | `sheet.rs:180` |
| **全高**需把 `top` 置 0：`top` 取自 `cx.theme().sheet.margin_top`（默认 `TITLE_BAR_HEIGHT`） | `sheet.rs:144` |
| **半透明**：`refine_style` 在 `.bg(cx.theme().tokens.background)` 之后生效，传 `.bg(色.opacity(x))` 可覆盖 | `sheet.rs:170-172` |
| **收回按钮**：Sheet 自带右上角关闭按钮，点它 dispatch `Cancel` → `request_close` → `close_sheet` | `sheet.rs:197-205`、`sheet.rs:240-242` |
| 自带滑入动画 0.15s、自带遮罩与"点遮罩关闭" | `sheet.rs:229`、`sheet.rs:239-242` |
| 无毛玻璃：gpui-pre 0.3.5 没有 `backdrop_blur` / `backdrop_filter` | 源码检索无此 API |
| 同一时刻只有一个 active sheet → 三个按钮天然互斥，切换即替换 | `root.rs:397-403`（`active_sheet.take()`） |
| sheet 层已在 `Workspace::render` 渲染，不需要新增 layer 挂载点 | `src/app.rs:664` |

**否决方案**

| 方案 | 否决理由 |
|---|---|
| 继续用 dock tab | 与需求相悖（需求就是要从 dock 移出来） |
| 完全自绘抽屉浮层 | 遮罩、滑入动画、Esc、焦点都要自己管，框架 `Sheet` 已具备 |
| 用 `Dialog` 承载 | Dialog 是居中/偏上浮层，不贴边、不能全高 |

**与既有坑的关联（必须遵守）**

- `open_sheet_at` 的 build 闭包与 dialog 一样**每次重绘都重放**（`root.rs:221-223`）：内容实体必须在 `open_sheet_at` **之外**创建一次，否则每帧重建、状态丢失；
- `open_sheet_at` 内部也会 `focus_handle.focus(window, cx)`（`root.rs:394`）：**不要**在它之后再调一次 focus，否则同帧出现两个"已聚焦"节点，命中框架的 a11y 断言（见 `src/ui/command.rs:293` 的注释）。抽屉内容若需要聚焦，focus 调用放在 build 闭包内（每帧幂等）。

## 4. 数据模型与持久化

- 新增 `Entity<RailState>`（`src/ui/rail.rs`）：只有一个字段 `opened: Option<DrawerKind>`，`DrawerKind` 为 `Outline | Backlink | Tags`；用于按钮选中态与"再点一次即收起"的 toggle。
- 抽屉内容实体：打开时 `cx.new(...)` 建一次，关闭即释放。三个面板实体本身不变（`OutlinePanel` / `BacklinkPanel` / `TagsPanel` 都已有 `Render`，可直接作为 sheet 的 child）。
- 布局：`MAIN_DOCK_VERSION` 5 → **6**，默认布局的右侧 dock **只放预览**（`src/app.rs:643-647` 删掉三行 `panel_view`）。
- 兼容：`src/main.rs:30-38` 的 `register_panel` **保留**——老布局文件里若残留这三个面板仍能恢复，不会变成空白面板。
- **不新增任何持久化文件**，抽屉状态不落盘（瞬时 UI）。

## 5. 交互流程

```
点击 rail「大纲」
  → toggle：若当前已是大纲 → Root::close_sheet（收起并清 RailState）
  → 否则 open_drawer(Outline, window, cx)
      ① 内容实体在 open_sheet_at 之外创建一次
      ② Root::open_sheet_at(Placement::Right, build, window, cx)
      ③ build: Sheet::new
              .size(px(300.))                 // 窄抽屉
              .bg(theme.background.opacity(0.88))  // 半透明
              .overlay(true).overlay_closable(true)
              .title("大纲")                  // 自带右上收回按钮
              .child(内容实体)
      ④ 打开前把 theme.sheet.margin_top 置 0 → 抽屉上下撑满

抽屉内点击大纲某一项
  → tabs::open_note_at(path, Some(line), window, cx)   // 复用现有点跳
  → 编辑区定位到该标题行并聚焦；抽屉保持打开，便于连续跳转

收回：点右上关闭按钮 / 点遮罩 / Esc → close_sheet → 清 RailState（按钮回到未选中）
```

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 没有打开任何笔记 | 大纲沿用既有空态文案「没有打开的笔记」（`src/ui/outline.rs:143-153`）；反链/标签按各自空态 |
| 抽屉开着时切换库 / 索引重建 | 反链、标签在构造时已订阅 `IndexChanged`（`backlink.rs`、`tags.rs` 的 `new`），自动刷新 |
| 抽屉开着时编辑笔记 | 大纲由 `AppState.outline` 驱动，编辑后实时刷新（既有机制） |
| 重复点同一个按钮 | 收起抽屉（toggle） |
| 点另一个按钮 | 框架自动替换 active sheet，内容切换，不需要先关 |
| 窗口很窄 | 抽屉 300px 固定；若窗口宽度不足 300+内容区，抽屉仍贴右显示（不挤压到 0） |
| 主题切换 | 抽屉背景取自 `cx.theme()`，跟随明暗 |
| 焦点 | 抽屉内不主动抢焦点；若将来加搜索框，focus 放 build 闭包内（见 §3 的 a11y 约束） |

## 7. 测试计划

自动化（`cargo test`）：

- `DrawerKind` 与按钮文案/图标的映射（纯函数）；
- `RailState::toggle` 语义：同项收起、异项替换（纯逻辑）；
- 大纲项 → `open_note_at(path, Some(line))` 的行号换算已有覆盖（`tests/ui/outline.rs`），本次不改。

需人工：见 §8（抽屉是 GUI 行为，无法在单元测试里断言）。

## 8. 人工验收清单

- [ ] 窗口最右侧有一条竖直按钮区，上下依次是「大纲 / 反链 / 标签」
- [ ] 点「大纲」→ 右侧滑出抽屉；宽度较窄（约 300px），**上下撑满整个界面**
- [ ] 抽屉是半透明的，能隐约看到下面的内容
- [ ] 点大纲某一项 → 编辑区跳到对应标题行（**索引定位**），抽屉保持打开
- [ ] 点「反链」→ 内容切换为反链列表；点某条 → 跳到来源笔记的对应行
- [ ] 点「标签」→ 显示标签列表；点标签展开其下笔记；点笔记跳转
- [ ] 抽屉右上角有收回按钮图标，点击即收起
- [ ] 点遮罩 / 按 Esc 也能收起
- [ ] 再点同一个按钮 → 收起（toggle）；点另一个按钮 → 内容替换
- [ ] 右侧 dock 现在只剩预览，不再有那三个 tab
- [ ] 切换明暗主题后抽屉配色正确，无明显重影/看不清
- [ ] 重启后布局正确：右侧 dock 只有预览，rail 仍在最右侧

## 9. 决策记录（已拍板）

| # | 问题 | 结论 |
|---|---|---|
| 1 | 预览区 | **保留右侧 dock，只放预览**；三个面板移到 rail + 抽屉 |
| 2 | rail 布局 | **浮在内容之上，不占宽度**（半透明浮层贴最右，内容区保持全宽） |
| 3 | 点大纲跳转后 | **抽屉保持打开**，手动点收回按钮关闭 |
| 4 | 抽屉尺寸 | **宽 260px**，不透明度 0.88（框架无毛玻璃，纯色半透明） |
| 5 | 毛玻璃 | 框架无 `backdrop_blur`，接受纯色半透明 |
| 6 | 抽屉状态 | **不持久化**，重启后不自动展开 |

补充实现细节（决策落地）：

- rail 渲染顺序放在 sheet 层**之后**、dialog 层**之前**：抽屉展开时 rail 仍可点（切换/收起），命令面板等模态打开时 rail 被遮住（符合模态语义）；
- 三个按钮用**内联 SVG 图标**（与 `ui/file_tree.rs:798-809` 同一做法）：默认图标集（`default-icons.txt`，101 个）里没有 list / tag / link，语义对不上。

## 10. 实施步骤

| 步骤 | 内容 | 可独立验证 |
|---|---|---|
| 1 | 新增 `src/ui/rail.rs`：`RailState`（Entity）+ 竖直按钮条渲染 | 编译通过，按钮出现在最右侧 |
| 2 | 新增 `src/ui/drawer.rs`：`open_drawer` / `close_drawer`（Sheet 承载，实体建在外面，全高 + 半透明） | 点按钮能弹出/收起抽屉 |
| 3 | `src/app.rs`：根布局改 `h_flex`（dock `flex_1` + rail 固定宽）；`reset_default_layout` 右侧只放预览；`MAIN_DOCK_VERSION` 5→6 | 布局正确、旧布局被重建 |
| 4 | 主题 `sheet.margin_top` 置 0（保证全高） | 抽屉上下撑满 |
| 5 | `cargo build` + `cargo clippy` + `cargo test` + 按 §8 人工验收 | 全绿 |
