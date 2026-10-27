# 面板伸展（编辑右伸 / 预览左伸）执行计划

| 属性 | 内容 |
|---|---|
| slug | preview-maximize |
| 创建日期 | 2026-10-27 |
| 需求文档 | 用户对话提出并两轮澄清（2026-10-27），结论见 §2 |
| 设计文档 | [`docs/preview-extend-design.md`](../preview-extend-design.md) |
| 关联 commit | — |

> 需求于 2026-10-27 二次澄清：**不是"最大化"，而是"伸展遮蔽对侧"**。
> 本文档据此重写，前版"最大化/最小化"表述作废。

## 状态机

需求 ✅ → 设计 🔄（方案待用户在 G2 拍板）→ 计划 ⬜ → 实施 ⬜ → 测试 ⬜ → 质量 ⬜

## 1 现状查证（附证据）

### 1.1 那个 Collapse 按钮找到了——它是框架自带的

| 事实 | 证据 |
|---|---|
| 是 DockArea **面板标题栏自带的折叠按钮**，由 gpui-component 渲染，**不在项目源码里** | `gpui-component-0.8.0/src/dock/tab_panel.rs:218` `dock_toggle_button` |
| 显示在中间 dock（编辑区）标题栏**右上角**，图标 `IconName::PanelRight` | 同上 `:253-257` |
| tooltip 文案 `Dock.Collapse` / `Dock.Expand`（英文，项目未配中文 i18n） | 同上 `:270-273` |
| 点击 → 折叠**右侧 dock（预览）** → 编辑区变宽占据其位置 | 同上 `:275-279` `toggle_dock_by_user` |

**这完全对应用户描述的"编辑框向右伸展并遮蔽预览框"。**

### 1.2 框架的两个硬约束（决定方案选型）

| 约束 | 证据 | 影响 |
|---|---|---|
| **中间区域根本不是 dock**：`DockArea` 只有 `docks: HashMap<DockPlacement, DockRegion>`，Center 单独存 `self.center` | `dock_area.rs:105-107`、`:191` | — |
| **`toggle_dock(Center)` 是死路**：函数体第一行 `docks.get_mut(&placement) else { return }`，Center 不在表里 → 静默返回 | `dock_area.rs:307` | ❌ **方案 D 不成立**（第一版就是踩这个） |
| **`is_dock_open(Center)` 恒 false** | `dock_area.rs:293-297` | 图标会永久卡在"已伸展" |
| **框架有完整 Zoom 能力**：`Panel::zoomable()`、`TabGroup::toggle_zoom()`、`DockArea::set_zoomed_in/out()` | `panel.rs:40`；`tab_group.rs:267`；`dock_area.rs:727,736` | ✅ 预览左伸的实际路径 |
| **Zoom 链路闭合**：`toggle_zoom` → `set_zoomed` → `TabGroupEvent::ZoomIn/Out` → `DockArea::set_zoom` | `tab_group.rs:391-405`；`dock_area.rs:1224-1237` | 不用自己记账 |
| **Zoom 后整块区域换成该 group**：`render` 命中 `zoomed_view()` 就不再画三栏 | `dock_area.rs:1565-1593` | ⚠️ **左侧文件树一并被遮**（要拍板） |
| **Zoom 不持久化**：`load()` 开头 `self.zoomed = None` | `dock_area.rs:824` | ✅ 正好符合 Q2 |
| 面板能拿到自己的 group：`Panel::on_added_to(WeakEntity<TabGroup>)` | `panel.rs:68`；调用点 `tab_group.rs:340` | 按钮触发 zoom 的入口 |

### 1.3 由此产生的不对称（要拍板的点）

| 状态 | 机制 | 文件树是否保留 |
|---|---|---|
| **B 编辑右伸**（现有 Collapse） | 折叠右侧 dock | ✅ **保留** |
| **C 预览左伸**（Zoom 方案） | zoom 预览 group | ❌ **一并被遮** |

想让 C 也保留文件树，只剩「撑大右 dock 尺寸」一条路，代价：
① 尺寸会写进 `layout.json`，与 Q2「不持久化」冲突，需在保存前还原；
② 窗口缩放要重算尺寸。**当前按 Zoom 交付，等人工验收拍板。**

## 2 需求定义与拍板记录

### 已拍板

| # | 问题 | 结论 |
|---|---|---|
| Q1 | 编辑区已有按钮（框架 Collapse），是否新增？ | **不新增**——已存在。本次**只做预览左伸这个按钮** |
| Q2 | 状态是否持久化 | **不持久化**——每次启动回到默认三栏 |
| Q3 | 入口形式 | **只要按钮**——不加快捷键、不加双击分隔条 |
| Q4 | B/C 能否直接互切 | **接受限制**：先回三栏再切另一侧 |
| Q5 | 文件树是否参与伸展 | **不参与**（前提：方案能保留文件树） |

### 待拍板（👤 交回用户）

| # | 问题 | 出处 |
|---|---|---|
| Q6 | 预览左伸时**文件树也被遮住，能否接受**？不能接受则改「撑大右 dock 尺寸」方案（代价见 §1.3） | `ACCEPTANCE.md` §14 / 设计文档 §9-1 |

## 3 边界条件（九项逐条过）

| # | 边界项 | 归属 | 说明 |
|---|---|---|---|
| 1 | 空输入 | **适用** | 无打开笔记时按钮是否禁用 |
| 2 | 超大输入 | 不适用 | — |
| 3 | 非法路径 | 不适用 | — |
| 4 | 并发写 | 不适用 | 不触碰文件 |
| 5 | 外部程序改文件 | 不适用 | — |
| 6 | 崩溃后半截文件 | 不适用 | — |
| 7 | 多库切换 | **适用** | 切 Vault 时回到三栏（Q2 不持久化） |
| 8 | 隐藏与忽略文件 | 不适用 | — |
| 9 | 非 UTF-8 | 不适用 | — |

**本项目特有的两项**：

1. **图片预览模式**：`PreviewPanel.image` 非空时面板显示图片而非 Markdown
   （`src/ui/preview.rs`），左伸按钮在该模式下仍可用。
2. **编辑区下方挂件**：AI 对话条 / 修订评审条在编辑区被遮蔽时如何处理。

## 4 任务与门禁

| T-n | 内容 | 依赖 | 涉及文件 | 验收方式 | 状态 |
|---|---|---|---|---|---|
| T-1 | ~~按钮调 `toggle_dock(Center)`~~ | G2 | `src/ui/preview.rs` | — | ❌ **证伪** |
| T-2 | 改走 Zoom：`PreviewPanel.group` + `on_added_to` + 按钮调 `group.toggle_zoom()`，状态读 `dock_area.zoomed_group()` | T-1 证伪 | `src/ui/preview.rs` | 编译 + 人工 | ✅ |
| T-3 | G4 四连自校验 | T-2 | — | build / clippy / fmt / test 全绿 | ✅ |
| T-4 | 人工验收（进 `ACCEPTANCE.md` §14），含 Q6 拍板 | T-2 | — | 用户亲自跑 | ⬜ 人工 |

| 门禁 | 结果 | 证据 |
|---|---|---|
| G1 需求 | ✅ | §2 五个问题全部拍板 |
| G2 设计 | ✅ | 用户确认（2026-10-27「好，开工」），方案后由 D 改 A（见偏差记录） |
| G3 计划 | ✅ | 本文件 |
| G4 实施 | ✅ | 四连见下（改 Zoom 后重跑） |
| G5 测试 | ⬜ | 无新增可单测逻辑（状态由框架持有），以人工验收为准 |
| G6 质量 | ⬜ | 待 T-4；Q6 仍需 👤 拍板 |

### G4 四连输出（改 Zoom 后重跑，2026-10-27）

```text
cargo build                : Finished dev profile，0 warning（项目代码）
cargo clippy --all-targets : 0 warning
cargo fmt --check          : clean
cargo test                 : 603 passed / 0 failed / 3 ignored
```

## 5 实施记录

### 第一版（已作废）—— `toggle_dock(Center)`

- `toolbar_buttons()` 加按钮，点击 `dock_area.toggle_dock(DockPlacement::Center)`
- 结果：用户实测**点击无任何反应**；图标因 `is_dock_open(Center)` 恒 false 卡死

### 当前实现 —— Zoom（2026-10-27）

改动仍只落在 `src/ui/preview.rs` 一处：

- import：`DockPlacement` → `TabGroup`
- `PreviewPanel` 增字段 `group: Option<WeakEntity<TabGroup>>`
- `impl BasePanel` 增 `on_added_to()` 存下 group 句柄
- `extend_toggle_button(group, cx)`：
  状态判据 `dock_area.zoomed_group() == Some(group.node())`（**每帧问容器，
  不维护本地标志**，杜绝不同步）；点击 `group.toggle_zoom(window, cx)`；
  `group` 为 `None` 时按钮不渲染，避免出现死按钮
- 按钮仍挂在 `title()` 最左端（见 §7）

## 6 下一步

1. **人工验收**（`ACCEPTANCE.md` §14）——本功能正确性只能靠跑起来看，
   尤其要确认「点击真的有反应」
2. 👤 **Q6 拍板**：文件树被遮能否接受
3. 按验收结果决定是否换成「撑大右 dock 尺寸」方案

## 7 按钮位置（已定：标题**最左侧**）

**第一版**用了 `toolbar_buttons()`，按钮落在标题栏**右侧**——因为
`gpui-component` 的 `Panel` trait 只有 `title()` / `title_suffix()` /
`toolbar_buttons()` 三个扩展点，后两个都固定在 trailing，**左侧没有入口**
（`gpui-component-0.8.0/src/dock/panel.rs:93-108`）。

用户确认要左侧后**改为挂在 `title()` 的最左端**（在「预览」两字之前）：

- 与现有 `density_menu`（密度菜单）同一区域——标题区放交互控件是本项目已有做法
- 必须包 `.occlude()`：否则点击会顺带触发标签选中（`density_menu` 已有此注释）
- **代价**：按钮会随 `title()` 一起出现在 tab 标签上（若面板被拖成 tab）。
  与密度菜单的既有取舍一致，接受。

被否决的备选：在预览**内容区**顶部自绘一行工具栏放左侧——会永久多占约 28px
高度且图片模式要一并处理，不如 `title()` 干净。

## 偏差记录

- 2026-10-27：需求由"最大化"**更正为**"伸展遮蔽对侧"，前版表述作废。
- 2026-10-27：首轮查证称"未找到该按钮"——**是我的失误**：grep 用了
  `head -15`，被 `ai.rs` 的 `collapsed` 字段刷屏截断，真正的框架按钮又不在
  项目源码里。用户提供关键词 `Collapse` 后在**依赖源码**中定位。
  教训：查证"某能力是否存在"时，① 不要过早 `head` 截断；
  ② 现象在 UI 上但代码里搜不到时，要想到**框架渲染**这一层。
- 2026-10-27（第二次，同类错误）：断言「Center 可以折叠，只要 `is_collapsible()`」
  ——只读了 `toggle_dock` 的**注释和前几行约束**，没往下读函数体第一行
  `docks.get_mut(&placement) else { return }`。而 Center **不在 `docks` 表里**
  （它是 `self.center` 字段）。结果第一版上线即死按钮，由用户实测发现。
  教训：**判断"某 API 对某个入参是否生效"时，必须读完函数体**，
  尤其是 `else { return }` 这类静默失败；"文档说支持"≠"这个入参走得到"。
  同类第三处：`is_dock_open` 同理恒 false。
