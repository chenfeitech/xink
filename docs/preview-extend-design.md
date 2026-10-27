# 设计：预览区向左伸展（遮蔽编辑区）

> 需求：预览区顶部**左侧**新增按钮，点击 → 预览向左伸展、遮蔽编辑区；再点恢复。
> 与现有「编辑区右上 Collapse 按钮 → 折叠右 dock → 编辑右伸遮蔽预览」形成对称。
> 执行计划：[`exec-plans/preview-maximize.md`](./exec-plans/preview-maximize.md)

## 1 现状与问题

| 事实 | 证据 |
|---|---|
| 编辑区已有伸展按钮，是 **gpui-component 框架自带的 dock toggle**，不在项目源码 | `gpui-component-0.8.0/src/dock/tab_panel.rs:218` `dock_toggle_button` |
| 它显示在中间 dock 标题栏右上角，图标 `PanelRight`，点击折叠**右侧 dock（预览）** | 同上 `:253-257`、`:275-279` |
| **中间 dock 不可折叠**：toggle 只支持 `Left`/`Right`/`Bottom`，`Center` → `return None` | 同上 `:246`、`:260` |
| 框架有 Zoom：`Panel::zoomable()` / `set_zoomed()` / `TabGroup::toggle_zoom()`（天然 toggle） | `gpui-base-0.8.0/src/dock/panel.rs:102,105`；`tab_group.rs:267` |
| Zoom 后该 group **填满整个 area** | `dock_area.rs:800` 注释：*the container that fills the area when something is zoomed* |
| 预览面板另有图片模式（`image` 非空时显示图片而非 Markdown） | `src/ui/preview.rs` |
| 三栏布局在 `src/app.rs`，DockArea 只用 `add_panel` / `remove_panel` | `src/app.rs:429`；`src/ui/tabs.rs:154,243` |
| **`toggle_dock(placement)` 对任何 placement 都有效**，只要求该 dock `is_collapsible()`，未排除 Center | `gpui-base-0.8.0/src/dock/dock_area.rs:301-320` |
| **`DockPlacement::collapsible` 默认 `true`** | `gpui-base-0.8.0/src/dock/dock_placement.rs:125` |
| 折叠用 `set_open(false)` + `reconcile()`，**面板实体保留**（不像 `remove_panel` 会丢状态） | `dock_area.rs:314-318` |
| 面板可自定义工具栏按钮：`Panel::toolbar_buttons()` | `gpui-component-0.8.0/src/dock/panel.rs:101,158` |
| 项目当前**未实现** `toolbar_buttons` | 全仓 grep 无结果 |

**关键澄清**：`tab_panel.rs:246` 的 `Center => None` 卡的是**按钮放哪**（`designated` 位置判定），
**不是能不能折叠**。折叠能力本身对 Center 完全可用（见上表第 6–7 行）。

**核心矛盾（已消解）**：原以为只能靠 Zoom（会遮蔽文件树）；
调研后发现可直接复用与 Collapse **完全相同**的 `toggle_dock` 机制，见方案 D。

## 2 目标 / 非目标

**目标**

- G1：预览区顶部左侧一个按钮，点击 → 预览向左伸展、遮蔽编辑区
- G2：**再点一次恢复**三栏原状（toggle）
- G3：与现有编辑区 Collapse 按钮在视觉与语义上对称

**非目标**

- 不做 B/C 直接互切（已拍板 Q4：先回三栏）
- 不做状态持久化（已拍板 Q2）
- 不加快捷键、不加双击分隔条（已拍板 Q3）
- 不改文件树那一栏（已拍板 Q5）
- 不改现有 Collapse 按钮的行为

## 3 方案对比

### 方案 D：复用框架 `toggle_dock(Center)` —— **推荐**（第二轮调研后新增）

`PreviewPanel` 实现 `toolbar_buttons()` 返回一个按钮，点击调
`dock_area.toggle_dock(DockPlacement::Center, window, cx)`。

| 维度 | 评价 |
|---|---|
| 改动面 | **小**——只加一个按钮 + 一次 `toggle_dock` 调用，不碰布局逻辑 |
| 对称性 | ✅ **与现有 Collapse 完全同一套机制**：右上按钮 `toggle_dock(Right)`，左上按钮 `toggle_dock(Center)` |
| toggle 语义 | ✅ 框架原生：`set_open(!open)`，再点即恢复 |
| 文件树 | ✅ **保留**（折叠的只是 Center dock，Left dock 不受影响） |
| 数据风险 | ✅ 无——`set_open(false)` 保留面板实体，光标 / 未保存内容不丢（对比 `remove_panel`） |
| 可测性 | 状态由框架持有；可测部分为按钮 id / tooltip 的查表一致性 |
| 升级风险 | 低——走的是公开 API，不与框架内部布局对抗 |

**为什么框架自己不画这个按钮**：`dock_toggle_button` 的 `designated` 要求按钮挂在
Center dock 的左上/右上 group 上；而「折叠 Center」的按钮**没有天然归属位置**
（该挂在谁身上？），故 `Center => None`。我们把它挂在**预览面板自己**的
`toolbar_buttons()` 上，正好补上这个空缺。

### 方案 A：复用框架 Zoom

`PreviewPanel` 实现 `zoomable() -> true`，按钮调 `TabGroup::toggle_zoom()`。

| 维度 | 评价 |
|---|---|
| 改动面 | **最小**——框架原生，toggle 语义免费（`toggle_zoom` = `set_zoomed(!zoomed)`） |
| 可测性 | 状态由框架持有，本项目可测部分少 |
| 数据风险 | 无（不触碰笔记文件） |
| 扩展性 | 编辑区若将来也要「右伸」，同样用 zoom |
| **缺点** | ⚠️ zoom 后**可能连左侧文件树一起遮蔽**，与 Collapse（保留文件树）**不对称** |

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

**采用方案 D**。它与现有 Collapse **同源、对称、不丢状态、改动最小**，
也不需要 Zoom 那种"填满整个 area"的语义——方案 A / B / C 均已无优势。

仍需**一次实测**确认框架的实际布局行为（§10 S0）：
折叠 Center dock 后，Right dock（预览）是否真的向左扩展填补、文件树是否保留。

> 按 `05-design-first.md` §4，框架行为**必须实测**，不得凭注释推断
> （历史教训：曾误判"框架没有链接点击回调""没有缩进设置"，源码核实后都是错的）。

## 4 数据模型与持久化

- **不持久化**（已拍板 Q2）
- 方案 A：状态由框架的 `TabGroup.zoomed` / `DockArea.zoomed` 持有，本项目不新增字段
- 方案 B：需在 `EditorPanel` 或 `AppState` 增一个枚举
  `enum Extent { Default, EditorExtended, PreviewExtended }`
- 旧数据兼容：不涉及（无新持久化字段）

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
- 按钮图标：待定，需确认 gpui-kit 内置图标清单里有能表达「向左伸展」的
  （当前全仓只用过 10 种图标，无此类）

## 6 异常与边界

| 场景 | 处理 |
|---|---|
| 没有打开笔记 | 按钮禁用或点击无效（待定，倾向禁用） |
| 预览处于图片模式 | 左伸照常可用 |
| 编辑区有未保存内容时左伸 | 不保存、不丢弃，仅遮蔽；恢复后内容还在 |
| 编辑区挂着 AI 对话条 / 修订评审条 | 随编辑区一起被遮蔽，恢复后保留 |
| 切 Vault | 回到默认三栏（不持久化） |
| 左伸状态下关闭预览面板 | 伸展状态随之失效，回到默认 |

## 7 测试计划

| 能自动化 | 要人工 |
|---|---|
| 方案 B 的状态机（若采用）：抽成纯函数，如 `next_extent(current, action)` | **zoom 后文件树是否保留**（方案 A 的前提，必须实测） |
| 按钮 id / tooltip 的查表一致性（沿用 `settings.rs` 描述表测试的做法） | 伸展动画是否平滑、有无闪烁 |
| — | 恢复后编辑区光标、滚动位置、未保存内容是否还在 |
| — | 图片模式下左伸的表现 |

## 8 人工验收清单

> 交付时进 `docs/ACCEPTANCE.md`，此处只放索引（`10-acceptance.md`）。

- [ ] 预览区顶部左侧出现按钮，图标能表达「向左伸展」
- [ ] 点击 → 预览向左伸展、编辑区被遮蔽
- [ ] **再点一次 → 恢复三栏**，比例与伸展前一致
- [ ] 伸展状态下编辑区的未保存内容、光标位置不丢
- [ ] 与编辑区右上 Collapse 按钮视觉对称、行为对称
- [ ] 图片预览模式下左伸正常
- [ ] 不同窗口尺寸下（窄窗 / 宽屏）伸展表现正常

## 9 风险与开放问题

| # | 类型 | 内容 |
|---|---|---|
| 1 | ~~需用户拍板 Q6~~ ✅ **已消解** | 方案 D 折叠的是 Center dock，文件树（Left dock）**天然保留**，无需在「对称」与「改动量」之间取舍 |
| 2 | 需实测 | 折叠 Center 后预览是否真的左伸填补（§10 S0 四点） |
| 3 | 需调研 | gpui-kit 内置图标清单里有没有「向左伸展」语义的图标 |
| 4 | 风险 | 方案 B 与框架布局对抗，gpui-kit 升级时易碎 |
| 5 | 风险 | 方案 B 若误用 `remove_panel` 会丢编辑状态 |

## 10 实施步骤

| 步 | 内容 | 可独立验证 |
|---|---|---|
| **S0** | **Spike 验证**：给 `PreviewPanel` 临时加一个按钮调 `toggle_dock(Center)`，实测四点——① 预览是否真的左伸填补 ② 文件树是否保留 ③ 再点是否恢复 ④ 编辑区光标 / 未保存内容是否还在 | 跑起来看四点（**Spike 原型，不是交付**；验证后按结果调整） |
| S1 | 根据 S0 结果锁定方案（预期方案 D 通过），更新本文档 §3 | 本文档定稿 |
| S2 | 实现按钮 + 伸展逻辑（预览面板侧） | 编译通过 |
| S3 | 边界处理：无笔记禁用、图片模式、切库重置 | 人工验收 |
| S4 | 文档：INDEX 登记、验收项进 `ACCEPTANCE.md` | 文档自洽 |
