# 编辑区 ↔ 预览区滚动同步重构

> 状态：**已确认并实施完毕**（2026-10-20）· **M1–M5 全部完成，待人工验收**
> 范围：`ui/editor.rs`、`ui/preview.rs`、`core/sync.rs`（新增锚点与方向锁）、`core/note.rs`
> 相关：`docs/DESIGN.md` §2.4（M4 预览）、`docs/navigation/outline-design.md`（大纲跳转同源）
>
> ⚠️ **版本提示**：本文引用的框架源码路径与行号基于 `gpui-base 0.6.4` /
> `gpui-pre 0.3.5`。项目已于 2026-10-21 升级到 `gpui-kit 0.8.0` /
> `gpui-pre 0.3.9`（见 `docs/DESIGN.md` §1.3），涉及内核行为（deferred 滚动、
> 虚拟列表测量、`logical_scroll_top`）的结论需**重新核对**。

**已确认的决策**

| 决策 | 结论 |
|---|---|
| D1 软换行 | 做成配置项 `editor.soft_wrap`，**默认开启**；关闭时走 O(1) 精确路径 |
| D2 锚点解析依赖 | `markdown` 1.0 从 dev-dependency **升为正式依赖**（与 gpui-base 同版本，Cargo 合并为一份） |
| D3 调试 HUD | 要：配置项 `preview.debug_scroll_sync`（默认关），预览面板标题显示 `E / P / Δ` |
| 开工方式 | 一次性完成 M1→M5（S0 的三个事实在实现过程中确认，见 §5） |

## 1. 现状

双向同步目前是**每帧轮询 + 线性比例**：

| 环节 | 位置 | 做法 |
|---|---|---|
| 编辑区轮询 | `ui/editor.rs:488-539`（`schedule_scroll_tick` :567 用 `on_next_frame` 自递归） | `ratio = -scroll_offset / (line_h × 逻辑行数 - viewport)` |
| 编辑区应用 | `ui/editor.rs:542-564` | `set_scroll_offset(-ratio × max)` |
| 预览区轮询 | `ui/preview.rs:122-164` | `ratio = -offset / (content_size - viewport)` |
| 预览区应用 | `ui/preview.rs:93-119` | `list_state().set_offset()` |
| 防循环 | 两侧各自的 `last_emitted_ratio` / `last_received_ratio` + `EPS = 0.002`（editor.rs:489 / preview.rs:123） | 单值比较吸收回显 |

两端**没有任何内容对齐**：只是把"滚动条百分比"互相拷贝。

## 2. 根因分析

按对准确性的影响从大到小：

### R1 比例 ≠ 内容位置（首要根因）

两端的"百分比"尺度完全不同：

- 编辑区是 Markdown **源码行**（一个代码块 = 1 行），预览区是**渲染块**（同一个代码块可能占 300px）。
- 一个 20 行的 ```python 块：编辑区占 20 行高度，预览区可能占 500px。比例相同时两端看到的内容相差甚远，且**误差随内容结构非线性放大**。
- 图片、表格、多级列表同理；编辑区的 `#` 标记在预览里变成大号标题。

表现："滚到文档后半段时两端完全对不上"，且前 1~2 屏看起来是准的（误差还没积累）。

### R2 编辑区内容高度估算偏低

`ui/editor.rs:514` 用 `line_h × note.line_count()` 当内容高度。但框架 `soft_wrap` **默认开启**
（`gpui-base-0.6.4/src/input/base/state.rs:689`，项目未覆盖，全仓 grep `soft_wrap` 无结果），
软换行下一个逻辑行占多个显示行 → 分母偏小 → 编辑区算出的 ratio **偏大**（跑得比实际快）。
长行越多的笔记，编辑区一侧越"超前"。

### R3 预览区 `content_size` 只统计已测量 item

`ListState::content_size` = `viewport + max_offset_for_scrollbar`，后者基于
**已测量的 item**（gpui-pre `list.rs:770-772`）。虚拟列表在远端未测量区域会低估内容高度，
于是滚动过程中分母在变 → 同一个物理位置对应的 ratio 漂移，表现为"越往下滚偏差越大 / 到底部跳一下"。

### R4 回显吸收用单值 EPS，会 ping-pong 也会丢帧

现在靠 `|ratio - last_received_ratio| <= 0.002` 判断"这是我刚应用的同步结果"：

- **抖动**：对端 `set_offset` / `set_scroll_offset` 会被 clamp、取整、按 item 对齐，
  回读到的 ratio 与目标值之差可能 > EPS（短文档尤其明显，1px 就超过 0.002）→
  被当成"用户滚动"反向推回 → 对端再回显 → **来回抖动**。
- **丢帧 / 卡住**：快速连续滚动时，用户滚到的位置若正好落在 `last_received ± EPS` 内，
  这一次滚动被误吸收，表现为"滚了一下又弹回去"。

### R5 非活动编辑器也在推送（`is_active` 只判了接收侧）

`apply_preview_ratio` 前有活动判断（`editor.rs:200-203`），但 `tick_scroll_sync`
（`editor.rs:488`）**没有** → 后台标签的编辑器面板同样每帧计算并 `emit(EditorScrolled)`，
把预览带跑。多标签场景下是硬 bug。

### R6 切换文档 / 重新解析后状态残留

`last_emitted_ratio` / `last_received_ratio` 只在"空白页"分支被清零（`editor.rs:517-521`、
`preview.rs:143-147`）。切换标签（`ActiveDocChanged`，`preview.rs:52-62`）、
`activate_preview`（`editor.rs:339`）、`clear()`（`editor.rs:448`）都不重置 →
新文档沿用旧比例，第一帧就跳到错误位置。

### R7 异步生效：写入滚动位置不是立即的

- 编辑区 `set_scroll_offset` 只是写入 `deferred_scroll_offset`，下一次 layout 才生效，
  且会被按 `scroll_size` clamp（`gpui-base input/base/element.rs:671-677`）。
- 预览 `set_offset` 在 `last_layout_bounds` 为 None 时**直接 return**（未布局时无效），
  随后仍会被 `rebase_pending_scroll` 重新 clamp。
- 预览 `set_text()` 超过 4KiB 走后台解析（`text/state.rs:472`），块数要几帧后才稳定；
  图片加载完成后 item 高度也会变。

→ 所以在"内容刚变 / 刚切换"的窗口内同步，用的是过期几何，必然错位。

## 3. 目标与非目标

> **同步时机（2026-10-21 修订）**：采用**停止后对齐**——滚动过程中不读不写对端，
> 停下 250ms 后对齐一次。原因是滚动中写对端必然产生跳变（块内渲染非线性、
> 虚拟列表高度在变），用户实测抖动后确认取"滚动全程不抖"。
> 详见 `docs/preview/preview-scroll-jitter.md` §3.0。

**目标**

1. 两端对齐到**同一份内容坐标**：Markdown **源码行号 + 行内比例**（下称 `LineAnchor`），
   而不是滚动条百分比。标题、段落、代码块、列表都能对上。
2. 抖动 / 跳变 / 滞后消失：防循环改用**方向锁 + 时间窗**，不再靠单值 EPS。
3. 覆盖边界：快速连续滚动、双向交替滚动、内容长度差异（图片/长代码块）、
   文档切换、编辑后重解析、图片模式、数据文件（json/yaml/toml）、多标签。

**非目标**

- 不做"逐字符 / 光标级"对齐（编辑区是源码、预览是渲染，做不到）。
- 不改变软换行默认行为（见 §8 待决策 D1）。
- 不引入新的滚动事件机制：框架两侧都没有可订阅的滚动事件，仍用 `on_next_frame` 轮询
  （`gpui-base` 的 `on_scroll_wheel` 是内部挂载的，不对外 emit）。

## 4. 方案：源行锚点同步

### 4.1 中间坐标

```rust
/// 两端共用的内容坐标：源码逻辑行 + 行内比例
struct LineAnchor {
    /// 0-based 源码行号
    line: usize,
    /// 行内 0.0–1.0（软换行下一行可能占多个显示行，用它细分）
    frac: f32,
}
```

两端各自实现 `LineAnchor ↔ 自己的滚动位置`，互相只传 `LineAnchor`。

### 4.2 预览区：源行 ↔ 列表 item（精确）

预览的列表项 = Markdown **顶层块**（`gpui-base text/document.rs:207-217`：
`if list_state.item_count() != blocks.len() { list_state.reset(blocks.len()) }`），
而解析用的是 `markdown` 1.0 的 mdast（`gpui-base/Cargo.toml [dependencies.markdown] 1.0.0`，
`text/state.rs:990`）。

于是：

1. `core` 新增锚点解析：对**同一份**预览源文本（`preview_source()` 的结果）用
   `markdown::to_mdast` 取 root 的顶层子节点，每个节点记
   `position.start.line .. position.end.line`，得到 `Vec<BlockSpan>`。
   - 与预览解析**同源同版本**，块划分天然一致。
   - 纯函数、可单测（围栏 / frontmatter / 表格 / 嵌套列表 / 引用 / HTML 块 / 分隔线）。
2. `源行 → 滚动位置`：`item_ix` = 包含该行的块下标，`offset_in_item` = 块内比例 × 块高，
   调 `ListState::scroll_to(ListOffset { item_ix, offset_in_item })`（gpui-pre `list.rs:660`）。
   **完全绕开 `content_size`**，根治 R3。
3. `滚动位置 → 源行`：`ListState::logical_scroll_top() -> ListOffset`（`list.rs:560`）
   直接给出"顶部落在哪个 item 的多少偏移"，反查 `BlockSpan` 即得源行。
4. **校验 + 降级**：运行时比对 `item_count()` 与 `BlockSpan` 条数；
   不等（说明解析器行为有出入）→ 该次同步降级为旧的百分比方式，并只告警一次，绝不乱跳。

### 4.3 编辑区：源行 ↔ 滚动位置

- **写**（把 `LineAnchor` 应用到编辑区）：目标行 → 字节偏移 → 用公开的
  `range_to_bounds(&Range<usize>)`（`gpui-base input/base/state.rs:3413`）拿该行的 y，
  再 `set_scroll_offset`。软换行下同样精确，因为它走的是真实布局。
  - 该 API 只对**已布局且可见**的区间返回 `Some`；目标行不在可见区时返回 `None`
    → 先用 `行号 × line_h` 粗调一帧，下一帧再用 bounds 精调（两帧收敛，约 32ms，肉眼无感）。
- **读**（把编辑区滚动位置换算成 `LineAnchor`）：以"上一帧的锚点"为起点做**局部探测**
  （前后各探几个候选行的 `range_to_bounds`），找到 bounds.top 最接近视口顶部的行；
  探测失败就沿用上一帧锚点（容忍 1 帧）。避免 O(n) 扫描。
- 软换行若关闭（见 D1），读写都可以直接 `行号 × line_h`，O(1) 且完全精确。

### 4.4 防循环：方向锁替代单值 EPS

```
struct SyncGuard {
    /// 当前"滚动源"：None / Editor / Preview
    owner: Option<Side>,
    /// 设为 owner 的时刻
    since: Instant,
    /// 我刚写入的目标锚点（用于判定回显）
    expected: Option<LineAnchor>,
}
```

规则：

1. 检测到"非回显"的真实滚动 → 若当前无 owner 或 owner 已过期（> 200ms）→ 抢占 owner 并推送。
2. owner 有效期内，**对端的任何变化都按回显吸收**（不再比较 EPS），只更新 `expected`。
3. 写入目标后记 `expected`；回读值与 `expected` 的差**按锚点种类给阈值**，不再用固定的
   0.002 比例：`Line` 用 0.5 行（块内插值 + 布局取整的往返误差远小于半行），
   `Ratio` 沿用 0.002。避免短文档里 1px 就被误判成用户滚动（R4）。
4. owner 过期后释放，双向恢复对称。

这样"快速连续滚动"不会被误吸收，"取整误差"不会引起 ping-pong，
"双向同时滚"由最后一个手势接管。

### 4.5 边界处理

| 场景 | 处理 |
|---|---|
| 内容不足一屏（`max <= 0`） | 锚点按文档末尾 clamp，对端不跳 |
| 编辑后重解析（`set_text` 异步，块数几帧后才稳） | `refresh_doc` 时把 `anchor_version += 1`；版本不匹配则本帧不同步，等块数稳定后**用最近一次锚点重新对齐一次** |
| 图片加载完 / 密度切换导致高度变化 | 检测到 `item_count` 或块高变化 → 触发一次 re-align（同上） |
| 切换标签 / 打开新笔记 | 两侧重置 `SyncGuard` 与 `expected`（修 R6） |
| 非活动编辑器面板 | `tick_scroll_sync` 增加 `is_active` 判断，非活动直接返回（修 R5） |
| 图片预览模式 | 不同步（保持现状） |
| 数据文件 json/yaml/toml | 预览是格式化后的文本，行号不对应 → **保持旧的百分比同步**，不套用锚点 |
| 首帧未布局（`line_height()` / viewport 为 0） | 跳过，下一帧再试（保持现状） |
| 空白页 / 无笔记 | 不同步（保持现状） |

## 5. 实施步骤

| 步骤 | 内容 | 状态 |
|---|---|---|
| **S0 Spike** | ① `item_count() == mdast 顶层子节点数`；② `range_to_bounds` 语义；③ `logical_scroll_top()` 稳定性 | ⚠️ 见下方说明 |
| **M1 修 bug + 防护** | 非活动编辑器不推送（`EditorPanel::is_active`）；切换文档重置（`reset_scroll_sync` / `ActiveDocChanged`）；`SyncGuard` 方向锁取代单值 EPS | ✅ |
| **M2 锚点解析** | 新增 `core/sync.rs`：`block_spans()` / `AnchorTable` / `ScrollAnchor` / `SyncGuard`，17 个单测 | ✅ |
| **M3 预览侧改造** | 改用 `logical_scroll_top()` / `scroll_to(ListOffset)`；`item_count` 校验 + 降级；块数变化时 re-align | ✅ |
| **M4 编辑侧改造** | 源行 ↔ 滚动位置：不换行时 O(1)；软换行时"粗调 + `range_to_bounds` 精调"（最多 3 次）；新增 `editor.soft_wrap` 配置项 | ✅ |
| **M5 重对齐与收尾** | 内容变化 / 图片加载后 re-align；数据文件走 `Ratio`；调试 HUD（`preview.debug_scroll_sync`） | ✅ |

**S0 的说明**：三个事实里 ②③ 已在代码中按"最坏情况"处理（② 用"粗调 + 精调闭环"，
无论 `range_to_bounds` 返回的是窗口坐标还是内容坐标都会收敛；③ 失败时降级为比例）。
只有 ① **没有在真实渲染里实测过**——需要跑应用看 HUD：若 `Δ` 长期很大，说明
`item_count` 与块数对不上、一直在走降级路径。运行时已有 `item_count` 校验兜底，
不会乱跳，但准确性会退回旧水平。这是本次唯一需要人工确认的实现假设。

M1 可单独交付（即使后面几步推迟，抖动与多标签问题先消失）。

## 6. 验证方式

**自动化（单测）**

- `block_spans`：围栏、frontmatter、表格、嵌套列表、块引用、HTML 块、分隔线、空行、CRLF。
- 锚点换算往返：`LineAnchor -> ListOffset -> LineAnchor` 误差 ≤ 1 行。
- 越界 clamp：锚点行号超过文档行数、文档为空、只有一个块。
- `SyncGuard` 状态机：给定事件序列（A 滚 → B 回显 → A 又滚 → 200ms 后 B 滚），
  断言推送次数与 owner 归属，证明"不 ping-pong、不丢用户滚动"。
- 编辑区换算（若纯函数化）：粗调/精调两帧收敛。

**人工验收清单**

- [ ] 滚编辑区：预览停在**同一个标题 / 段落**（不是"差不多"）
- [ ] 滚预览：编辑区顶部是**同一行源码**
- [ ] 含 50 行 ```python 块的笔记：两端对齐（验证 R1 修复）
- [ ] 含大图片的笔记：图片加载完成后不漂移
- [ ] 快速甩动滚动、连续来回滚：无抖动、无跳回
- [ ] 两端交替滚动（先滚编辑再滚预览）：最后一次手势生效
- [ ] 开 3 个标签：只有活动标签带动预览
- [ ] 切标签 / 新开笔记：不跳到旧位置
- [ ] 编辑文字后（300ms 防抖触发重解析）：预览重新对齐到正在编辑的位置
- [ ] 数据文件（json）：行为与现在一致（百分比同步）
- [ ] 关闭 `同步滚动` 开关：两端完全独立

**可选调试手段**：加一个环境变量开关，在状态栏显示两端当前 `LineAnchor` 与偏差行数，
把"对不对得上"从肉眼感觉变成可读的数字。

## 7. 风险

1. **块划分一致性**：`item_count` 与 mdast 顶层子节点数若在某些文档上不等，锚点会错位。
   已用运行时校验 + 降级兜住（§4.2 第 4 条），但降级时准确性退回现状。S0 先量化命中率。
2. **`range_to_bounds` 只对可见区间返回**：编辑侧需要"粗调 + 精调"两帧，
   极端情况（一次跳很远）可能有一帧可见的过渡。
3. **每帧轮询成本**：`on_next_frame` 保持不变；新增的 bounds 探测限制在常数次调用。
4. **`markdown` crate 从 dev-dependency 提升为正式依赖**：与 gpui-base 用同一版本 1.0，
   不会引入第二份（Cargo 会合并），但要确认 feature 一致。

## 8. 待决策（确认后再动手）

- ~~D1 软换行~~：**已定** —— 配置项 `editor.soft_wrap`，默认开启。
- ~~D2 `markdown` 1.0 作为运行依赖~~：**已定** —— 接受，升为正式依赖。
- ~~D3 调试 HUD~~：**已定** —— 要，配置项 `preview.debug_scroll_sync`，默认关闭。

## 9. 参考（框架 API 事实）

| 事实 | 位置 |
|---|---|
| 预览 item = 顶层 block，`item_count` 与之同步 | `gpui-base-0.6.4/src/text/document.rs:207-217` |
| 预览解析用 `markdown` 1.0 (mdast) | `gpui-base-0.6.4/Cargo.toml`；`text/state.rs:990` |
| `ListState::{logical_scroll_top, scroll_to, bounds_for_item, item_count}` | gpui-pre-0.3.5 `elements/list.rs:560 / 660 / 711 / 478` |
| `content_size` 只统计已测量 item | gpui-pre `list.rs:770-772`；`gpui-base scrollbar.rs:134` |
| `set_offset` 未布局时无效 + 会被 clamp | gpui-pre `list.rs:1371-1384` |
| 编辑区 `set_scroll_offset` 延迟生效且被 clamp | `gpui-base input/base/state.rs:2814`；`input/base/element.rs:671-677` |
| `range_to_bounds` 仅对已布局可见区间返回 | `gpui-base input/base/state.rs:3413-3432` |
| 软换行默认开 | `gpui-base input/base/state.rs:689` |
| 两侧均无对外滚动事件 | 预览：`text/` 无 `on_scroll`；编辑：`on_scroll_wheel` 为 `pub(super)` |
