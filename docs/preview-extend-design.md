# 设计：预览区向左伸展（遮蔽编辑区）

> 需求：预览区顶部**左侧**新增按钮，点击 → 预览向左伸展、遮蔽编辑区；再点恢复。
> 与现有「编辑区右上 Collapse 按钮 → 折叠右 dock → 编辑右伸遮蔽预览」形成对称。
> 执行计划：[`exec-plans/preview-maximize.md`](./exec-plans/preview-maximize.md)

## 1 现状与问题

| 事实 | 证据 |
|---|---|
| 编辑区已有伸展按钮，是 **gpui-component 框架自带的 dock toggle**，不在项目源码 | `gpui-component-0.8.0/src/dock/tab_panel.rs:218` `dock_toggle_button` |
| 它显示在中间 dock 标题栏右上角，图标 `PanelRight`，点击折叠**右侧 dock（预览）** | 同上 `:253-257`、`:275-279` |
| **中间区域不是 dock**：`DockArea` 只有 `docks: HashMap<DockPlacement, DockRegion>`，Center 单独存 `self.center` | `gpui-base-0.8.0/src/dock/dock_area.rs:105-107`、`:191` |
| **`toggle_dock` 开头就 `docks.get_mut(&placement) else { return }`——Center 不在表里，直接静默返回** | 同上 `:307` |
| **`is_dock_open(Center)` 恒为 `false`** | 同上 `:293-297`（`docks.get` 取不到 Center） |
| 框架有 Zoom：`Panel::zoomable()` / `TabGroup::toggle_zoom()`（天然 toggle） | `gpui-base-0.8.0/src/dock/panel.rs:40`；`tab_group.rs:267` |
| Zoom 链路完整：`toggle_zoom` → `set_zoomed` → `TabGroupEvent::ZoomIn/Out` → `DockArea::set_zoom` | `tab_group.rs:391-405`；`dock_area.rs:1224-1237` |
| **Zoom 后整块区域被换成该 group**：`render` 里 `zoomed_view()` 命中就不再画 Left/Center/Right | `dock_area.rs:1565-1593` |
| Zoom **不持久化**：`load()` 第一行就 `self.zoomed = None` | `dock_area.rs:824` |
| 面板能拿到自己所在 group：`Panel::on_added_to(WeakEntity<TabGroup>)` | `gpui-base-0.8.0/src/dock/panel.rs:68`；调用点 `tab_group.rs:340` |
| 预览面板另有图片模式（`image` 非空时显示图片而非 Markdown） | `src/ui/preview.rs` |
| 三栏布局在 `src/app.rs`，DockArea 只用 `add_panel` / `remove_panel` | `src/app.rs:429`；`src/ui/tabs.rs:154,243` |
| 折叠用 `set_open(false)` + `reconcile()`，**面板实体保留**（不像 `remove_panel` 会丢状态） | `dock_area.rs:314-318` |
| 框架没有**标题栏左侧**的自定义按钮入口：`toolbar_buttons` / `title_suffix` 都在 trailing（右侧） | `gpui-component-0.8.0/src/dock/panel.rs:92-107` |

**关键澄清 1**：`tab_panel.rs:246` 的 `Center => None` 卡的是**按钮放哪**（`designated` 位置判定）。

**关键澄清 2（第三轮调研，推翻方案 D）**：折叠能力**只对真实存在的 dock 生效**。
Center 不是 dock，所以 `toggle_dock(Center)` 是**死路**——不报错、不生效，
且 `is_dock_open(Center)` 恒 false，图标会永久卡在"已伸展"。
首版代码上线后用户反馈"点击无响应"，根因即在此。

## 2 目标 / 非目标

**目标**

- G1：预览区顶部左侧一个按钮，点击 → 预览向左伸展、遮蔽编辑区
- G2：**再点一次恢复**三栏原状（toggle）
- G3：与现有编辑区 Collapse 按钮在视觉与语义上对称

**非目标**

- 不做 B/C 直接互切（已拍板 Q4：先回三栏）
- 不做状态持久化（已拍板 Q2）
- 不加快捷键、不加双击分隔条（已拍板 Q3）
- 不改文件树那一栏的**布局与尺寸**（已拍板 Q5）
  —— 但采用 Zoom 后，伸展期间文件树会随整块区域一起被预览**覆盖**；
  这只是"被遮住"，文件树的尺寸 / 状态 / 内容都不变，恢复后原样回来
- 不改现有 Collapse 按钮的行为

## 3 方案对比

### 方案 A：复用框架 Zoom —— **采用**（第三轮调研后改选）

`PreviewPanel` 在 `on_added_to` 里记住自己的 `TabGroup`，按钮点击调
`group.toggle_zoom(window, cx)`；状态直接读 `dock_area.zoomed_group()`。

| 维度 | 评价 |
|---|---|
| 改动面 | **小**——一个字段 + 一个 `on_added_to` + 一个按钮，不碰布局逻辑 |
| toggle 语义 | ✅ 框架原生：`toggle_zoom` = `set_zoomed(!zoomed)`，再点即恢复 |
| 状态准确性 | ✅ 状态由**容器**持有，按钮每帧从 `zoomed_group()` 读，不存在本地标志与框架不同步 |
| 持久化 | ✅ **不持久化**（`load()` 清 zoom），正好符合已拍板的 Q2 |
| 窗口缩放 | ✅ zoom 恒填满整块区域，窗口变大变小都不用重算尺寸 |
| 数据风险 | ✅ 无——不动笔记文件，也不动面板实体 |
| 升级风险 | 低——走公开 API，不与框架内部布局对抗 |
| **代价** | ⚠️ zoom 填的是**整个 area**，左栏文件树一并被遮（见 §2 非目标 Q5 的重新界定） |

### ~~方案 D：复用框架 `toggle_dock(Center)`~~ —— ❌ **已否决（实现后证伪）**

| 维度 | 评价 |
|---|---|
| 改动面 | 小（曾以为） |
| **致命缺陷** | ❌ `docks` 里没有 `Center` 键，`toggle_dock` 开头 `get_mut` 直接 return；`is_dock_open(Center)` 恒 false。**上线后实测：点击无任何反应** |
| 否决理由 | 未读到 `toggle_dock` 函数体第一行就下结论（详见 `exec-plans/preview-maximize.md` 偏差记录） |

### 方案 B：自实现「折叠中间 dock」

自己维护伸展状态，把编辑面板从布局中移除或压到 0 宽，保留文件树。

| 维度 | 评价 |
|---|---|
| 改动面 | **大**——框架明确不支持折叠 Center（`tab_panel.rs:246`），要绕过 |
| 可测性 | 状态机可抽成纯函数单测（沿用 `rail.rs::next_opened` 的做法） |
| 数据风险 | 若用 `remove_panel` 会丢面板状态（编辑器光标、未保存内容） |
| 扩展性 | 完全可控 |
| **缺点** | 与框架布局逻辑对抗，后续升级 gpui-kit 易碎；`remove_panel` 有丢数据风险 |

### 方案 C：以编程方式调整分隔条（把编辑区压到最小）

| 维度 | 评价 |
|---|---|
| 改动面 | 中等 |
| 可测性 | 一般 |
| 数据风险 | 无 |
| **缺点** | ① 需先验证框架是否暴露「设置面板尺寸」的 API（**未调研，不得凭经验判断**）；② 「遮蔽」不彻底——编辑区仍占一条，不是真的被预览取代 |

### 被否决的方案

| 方案 | 否决理由 |
|---|---|
| 直接 `remove_panel` 编辑面板 | 会丢编辑器状态（光标、未保存内容），`tabs.rs:141` 已有注释警告「一个面板都不剩会 panic」 |
| 用右侧 rail 抽屉（现有 `drawer.rs`）覆盖 | 抽屉是浮层，不是「伸展」，预览区本身没变宽，语义不符 |

### 建议

**采用方案 A（Zoom）**。方案 D 已被源码证伪；方案 B/C 要么与框架布局对抗，
要么要自己承担"尺寸 + 持久化"两套账（右 dock 尺寸会被 `layout.json` 持久化，
与"不持久化"的拍板冲突）。Zoom 是框架里唯一一条**现成、可逆、不落盘、
且窗口缩放自适应**的路。

已知代价只有一条：zoom 会连左栏文件树一起遮住。这条需**人工验收确认**，
见 §9 开放问题 1。

> 按 `05-design-first.md` §4，框架行为**必须查源码/实测**，不得凭注释推断
> （历史教训：曾误判"框架没有链接点击回调""没有缩进设置"，
> 以及本轮误判"Center 可以折叠"，源码核实后都是错的）。

## 4 数据模型与持久化

- **不持久化**（已拍板 Q2）：zoom 状态由框架的 `TabGroup.zoomed` / `DockArea.zoomed`
  持有，`DockArea::load()` 会清零（`dock_area.rs:824`），重启必回三栏
- 本项目新增字段只有一个：`PreviewPanel.group: Option<WeakEntity<TabGroup>>`，
  **纯运行时句柄，不入 `dump()`**，不写 `layout.json`
- 旧数据兼容：不涉及（无新持久化字段，`layout.json` 格式不变）

## 5 交互流程

```
[默认三栏]
   │
   ├─ 点预览区左上 [←] ──→ [预览左伸，编辑被遮蔽]
   │                            │
   │                            └─ 再点 [←] ──→ 回默认三栏
   │
   └─ 点编辑区右上 [→] ──→ [编辑右伸，预览被遮蔽]（现有能力）
                                │
                                └─ 再点 [→] ──→ 回默认三栏
```

- B/C 之间**不直接互切**（已拍板 Q4）
- 按钮图标：沿用框架 dock toggle 同款的 `PanelLeft` / `PanelLeftOpen`
  （`tab_panel.rs:254-255` 已在用），与编辑区 Collapse 视觉一致

**伸展态的实际形态（Zoom 语义）**

```
默认三栏               伸展态（预览 zoom）
┌────┬───────┬─────┐   ┌──────────────────┐
│ 树 │ 编辑  │ 预览 │   │ 预览（含自己的    │
│    │       │ [←] │ → │ 标题栏，按钮还在）│
└────┴───────┴─────┘   └──────────────────┘
                          ↑ 编辑区被覆盖
                          ↑ 文件树亦被覆盖（Zoom 填满整块 area）
```

即：**不是"预览变宽"，而是"预览所在的 group 整块顶掉了整个 dock 区域"**；
被遮住的两栏只是不渲染，实体、尺寸、内容都还在，取消 zoom 后原样回来。

## 6 异常与边界

| 场景 | 处理 |
|---|---|
| 没有打开笔记 | 预览是空的，但仍可伸展（zoom 不看有没有内容）；不做禁用，避免按钮时有时无 |
| 预览处于图片模式 | 左伸照常可用 |
| 编辑区有未保存内容时左伸 | 不保存、不丢弃，仅被覆盖；恢复后内容、光标、滚动位置都还在 |
| 编辑区挂着 AI 对话条 / 修订评审条 | 随编辑区一起被覆盖，恢复后保留 |
| 切 Vault / 重载布局 | 回到默认三栏（`load()` 清 zoom） |
| 伸展中把预览面板拖走 / 关闭 | group 消失 → `zoom_survives` 判定失败 → 自动回到三栏（`dock_area.rs:1092-1098`） |
| 伸展中缩放窗口 | zoom 恒填满整块 area，无需重算尺寸 |
| 还没加入 group（`on_added_to` 未触发） | `group` 为 `None` → **按钮不渲染**，不会出现"点了没反应的死按钮" |

## 7 测试计划

| 能自动化 | 要人工 |
|---|---|
| `cargo build` / `clippy --all-targets` / `fmt --check` / `test`（G4 四连） | **点击是否真的伸展**（上一版就是死按钮，必须人眼确认） |
| 无新增纯函数可测：状态判据 `zoomed_group() == Some(group.node())` 一行，逻辑全在框架侧 | **伸展时文件树是否也被遮**（开放问题 1，需拍板） |
| — | 再点是否准确回到三栏、各栏比例是否与伸展前一致 |
| — | 恢复后编辑区光标、滚动位置、未保存内容是否还在 |
| — | 图片模式下、窄窗 / 宽屏下的表现 |
| — | 切换 / 关闭 / 拖走预览面板后伸展态是否自动解除 |

## 8 人工验收清单

> 交付时进 `docs/ACCEPTANCE.md`，此处只放索引（`10-acceptance.md`）。

- [ ] 预览区标题**最左侧**出现按钮（在「预览」两字之前）
- [ ] 点击 → 预览铺满、编辑区被遮蔽（**不再是"点了没反应"**）
- [ ] 伸展状态下**文件树是否也该被遮** 👤（开放问题 1，需拍板）
- [ ] **再点一次 → 恢复三栏**，比例与伸展前一致
- [ ] 按钮图标在两种状态下正确切换（伸展态显示"收起"）
- [ ] 伸展状态下编辑区的未保存内容、光标位置不丢
- [ ] 与编辑区右上 Collapse 按钮视觉对称
- [ ] 图片预览模式下左伸正常
- [ ] 不同窗口尺寸下（窄窗 / 宽屏）伸展表现正常

> 完整清单见 `docs/ACCEPTANCE.md` §14。

## 9 风险与开放问题

| # | 类型 | 内容 |
|---|---|---|
| 1 | **需用户拍板** 👤 | Zoom 会连左栏文件树一起遮住。若要求"伸展后文件树仍在"，只能改用「撑大右 dock 尺寸」方案，代价：① 尺寸会被 `layout.json` 持久化，与 Q2「不持久化」冲突，需在保存前还原 ② 窗口缩放时要重算尺寸。**当前按 Zoom 交付，等验收结论** |
| 2 | 已核实 | Zoom 链路与渲染替换点（`dock_area.rs:1224` / `:1565`）——源码级确认，行为仍进人工验收 |
| 3 | 已解决 | 图标沿用框架 dock toggle 同款的 `PanelLeft` / `PanelLeftOpen`（`tab_panel.rs:254-255` 已在用），与编辑区 Collapse 视觉一致 |
| 4 | 已知限制 | **无过渡动画**：框架的 dock toggle / zoom 都是即时布局替换，没有暴露动画钩子。要平滑动画需自绘过渡层，成本另议 |
| 5 | 已消解 | 方案 B/C 不再采用，其风险（对抗框架布局、尺寸持久化）不再进入本需求 |

## 10 实施步骤

| 步 | 内容 | 状态 |
|---|---|---|
| S0 | 查证框架 Collapse 按钮的实现位置 | ✅ `tab_panel.rs:218` |
| S1 | 第一版实现：`toggle_dock(Center)` | ❌ **证伪**：Center 不在 `docks` 里，点击无响应（用户实测反馈） |
| S2 | 第三轮源码查证：`docks` / `center` / `toggle_dock` / zoom 链路 | ✅ 本文 §1、§3 |
| S3 | 改走 Zoom：`on_added_to` 存 group + 按钮调 `toggle_zoom` + 状态读 `zoomed_group()` | ✅ 已实现（`src/ui/preview.rs`），G4 四连通过 |
| S4 | 按钮位置：从 `toolbar_buttons()`（右）移到 `title()` 最左 | ✅ 已实现并交付 |
| S5 | 人工验收（含开放问题 1：文件树是否可接受被遮） | ⬜ 待用户跑 `docs/ACCEPTANCE.md` §14 |
