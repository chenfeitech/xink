# 设计：大纲（TOC）面板（P1-T8 / R9）

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 已能解析标题（层级、文本、0-based 行号） | `core/markdown.rs::parse_facts`（T5） |
| 打开笔记只有 `open_note(path)`，没有"跳到某一行"的入口 | `src/ui/tabs.rs:181` |
| 编辑区可用 `set_cursor_position(Position)` + `set_scroll_offset` 定位 | `src/ui/editor.rs:1232`、`src/ui/editor.rs:434`（预览滚动同步已用） |
| 右侧 dock 目前只有预览一个 tab | `src/app.rs:603` |
| 索引（T6）是"保存后"语义，不适合跟手的大纲 | `docs/navigation/note-index-design.md` §5 |

缺的是：把标题显示出来，并且点一下能跳过去。

## 2. 目标 / 非目标

**目标**

- 右侧"大纲"tab：按层级缩进展示当前笔记的标题
- 点击标题 → 编辑区光标落到该行并把该行滚到可见
- 跟随编辑与切标签刷新

**非目标**

- 不做标题折叠 / 过滤
- 不做拖拽调整标题顺序
- 不读索引（索引留给反链、标签、搜索）

## 3. 方案概述

**数据源：编辑器实时解析，不读索引。**

大纲必须跟手：输入 `# 新标题` 就应该出现。索引在保存后才更新（且要等 300ms 防抖），
语义不对；单篇笔记 `parse_facts` 是纯内存操作，成本可忽略。

链路：`EditorPanel` 在两处把"（路径, 标题列表）"写进 `AppState.outline`：

| 时机 | 覆盖场景 |
|---|---|
| `refresh_doc`（笔记载入 / 内容变更后） | 编辑、保存、外部重载 |
| `set_active`（标签切换） | 切到另一个已打开的标签 |

面板只渲染 `AppState.outline`，不持有自己的文本。

**跳转**：`tabs::open_note_at(path, Option<usize>)` 作为**唯一入口**的扩展
（`open_note` 退化为 `open_note_at(path, None)`），命中已打开标签时只激活再跳行，
不新建标签、不重新读盘——与现有两条硬规则一致（`docs/DESIGN.md` §2.5）。

编辑器侧 `EditorPanel::goto_line(line)`：

1. `set_cursor_position(Position::new(line, 0))`；
2. 按 `line_height × line` 算出目标偏移，`set_scroll_offset` 并夹到
   `[-(内容高-视口高), 0]`，避免越界回弹。

**面板位置**：右侧 dock，与"预览"同为一组 tab（Q2 默认）。
因为新增了一个默认面板，`MAIN_DOCK_VERSION` 递增到 3，旧布局按设计回落默认布局。

## 4. 数据模型

```rust
pub struct OutlineState {
    /// 当前笔记；`None` 表示没有活动笔记
    pub path: Option<PathBuf>,
    pub items: Vec<OutlineItem>,
}

pub struct OutlineItem { pub level: u8, pub text: String, pub line: usize }
```

- 不落盘：与预览一样是编辑器的派生状态
- 空笔记 / 数据文件 → `items` 为空，面板显示"这篇笔记还没有标题"

## 5. 异常与边界

| 场景 | 处理 |
|---|---|
| 没有打开的笔记 | 空态提示，不报错 |
| 打开的是数据文件（json/yaml/toml） | 没有标题 → 空态 |
| 行号超出（笔记比索引新） | 光标与滚动都做夹取，不 panic |
| 标题在代码块围栏内 | 已被 `parse_facts` 排除（T5） |
| 点击时目标标签未打开 | 走 `open_note_at` 正常打开后再跳行 |
| 超长笔记（数千标题） | 全部渲染；真出现性能问题再虚拟化（当前无证据） |

## 6. 测试计划

| 层 | 内容 |
|---|---|
| `core`（已有） | `tests/core/markdown.rs` 覆盖标题解析与行号 |
| `ui`（新增，纯函数） | `tests/ui/outline.rs`：由 `DocFacts` 生成条目——层级过滤、缩进上限、行号透传、空输入 |
| UI 交互 | 依赖窗口，走人工验收 |

## 7. 人工验收清单

- [ ] 新建笔记后右侧出现"大纲"tab（与"预览"并列）
- [ ] 输入 `## 小节` 后大纲出现该标题（保存后即可，无需重开）
- [ ] 切到另一个标签 → 大纲跟着换
- [ ] 点击标题 → 编辑区光标落到该行、该行滚到可见；长文档定位准确
- [ ] 点击当前笔记的标题不产生第二个标签
- [ ] 打开 json/yaml 文件 → 大纲显示空态提示
- [ ] 中文标题正常显示

## 8. 开放问题

| # | 问题 | 决定 |
|---|---|---|
| 1 | 大纲是否与预览同组 tab | 是（Q2 默认） |
| 2 | 数据源用索引还是实时解析 | 实时解析（索引是保存后语义） |
| 3 | `MAIN_DOCK_VERSION` 是否递增 | 递增到 3：默认布局新增面板，旧布局回落默认 |

## 9. 实施步骤

| 步 | 内容 |
|---|---|
| S1 | `open_note_at` + `EditorPanel::goto_line` |
| S2 | `OutlineState` / `OutlineItem` + 生成函数 + 单测 |
| S3 | `OutlinePanel` UI + 注册面板 + 默认布局加 tab + 编辑器推送 |
