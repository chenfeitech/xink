# 设计：命令面板（Cmd+P）— P1-T7 / R10

## 1. 现状与问题

| 事实 | 证据 |
|---|---|
| 没有任何"按名字快速打开文件"的入口：只有左侧目录树逐层点开 | `src/ui/file_tree.rs` |
| 打开笔记的唯一入口是 `tabs::open_note`，必须复用 | `src/ui/tabs.rs:181`；`docs/DESIGN.md:196` |
| 库内文件列表只能靠 `Vault::scan(show_all)` 拿，返回树结构，同步 IO | `src/core/vault.rs:41`、`scan_dir` `:120` |
| 模糊匹配已就绪（T4）：`core/fuzzy.rs`，返回分数与命中位置 | `src/core/fuzzy.rs:21` |
| 项目目前没有窗口内浮层：确认/重命名走系统原生 `window.prompt` | `src/ui/file_tree.rs:182,241`；设置是独立窗口 `src/ui/settings.rs:1429` |

需要解决的是"用什么承载浮层"与"候选从哪来、多久刷新一次"。

## 2. 目标 / 非目标

**目标**
- `Cmd+P` 唤起面板，输入即过滤当前库内的文件，上下键选择、回车打开
- 走 `tabs::open_note`：已打开的笔记只激活，不产生第二个标签
- 首屏候选立即出现（有缓存），后台重扫后刷新

**非目标（本轮不做）**
- 命令项（切换主题、保存、新建笔记…）——只做"打开文件"，命令在候选结构里预留
- 全文内容检索（R8，T12 另做）
- 跨库搜索（见 `docs/multi-vault-design.md:43`）
- 文件名高亮的像素级打磨（先保证排序与打开正确）

## 3. 方案概述

**承载：窗口内 `Dialog` + 框架自带 `Command` 组件。**

| 结论 | 依据 |
|---|---|
| 用 `Root::open_dialog(build)` 打开，`Dialog` 作为容器 | `gpui-component-0.6.4/src/root.rs:297`；`Workspace::render` 已渲染 `render_dialog_layer`（`src/app.rs:528`），不渲染这一层则浮层永远不出现 |
| `Command` 自带键盘导航与 `escape/enter/up/down` 绑定 | `command/state.rs:62-70` 注册绑定、`state.rs:822` 设置 `.key_context("Command")`；`gpui_kit::init(cx)` 会调到 `gpui_component::init`（`gpui-kit-0.6.4/src/lib.rs:156-161`），本应用在 `main.rs:43` 已调用 |
| 关闭走 `Root::update(window, cx, |root, …| root.close_dialog(...))` | `root.rs:333`；`Root::update` 的取窗口根视图方式见 `root.rs:156` |

**否决方案**

| 方案 | 否决理由 |
|---|---|
| 像设置那样新开窗口（`cx.open_window`） | 命令面板是高频瞬时 UI，独立窗口有窗口切换开销，且关闭后焦点回落要靠系统 |
| `Sheet` 侧边抽屉 | 语义是"从中部/顶部浮出的快速选择"，抽屉会挤占布局 |
| 自绘浮层（`div` + 绝对定位） | 焦点、Esc、遮罩点击、滚动都要自己管，`Dialog` 已具备 |

**数据流**

```
Cmd+P → OpenCommandPalette（Workspace 根元素 on_action）
      → Root::open_dialog(CommandPalette 视图)
      → 打开瞬间：取缓存候选（无则空 + loading），后台 Vault::scan 重扫
      → on_query：core::fuzzy 打分 → 排序 → 取前 N 装进 Command（filterable=false）
      → on_confirm(IndexPath) → matches[ix].path → tabs::open_note → 关闭 Dialog
```

## 4. 数据模型与持久化

```rust
pub struct CommandPalette {
    state: Entity<CommandState>,   // 框架组件状态（查询、选中、滚动）
    /// 候选池：当前库内可打开的文件（绝对路径）
    files: Vec<PathBuf>,
    /// 当前展示的匹配结果，与 Command 里装的项目**顺序一一对应**
    matches: Vec<Candidate>,
    /// 命中字符位置，用于渲染高亮（与 matches 同序）
    hits: Vec<Vec<usize>>,
    scanning: bool,
}

pub struct Candidate {
    /// 用于匹配的文本：相对库根的路径（含扩展名）
    label: String,
    path: PathBuf,
}
```

- **不落盘**：候选是 `Vault::scan` 的派生数据，磁盘文件才是真值
- 缓存失效：`AppState.vaults.active_root()` 变化时清空；**每次打开面板都后台重扫一次**
  （用户刚在外部新建的文件，重开面板即可见；不接 watcher，避免为一个瞬时 UI 增加订阅）

## 5. 交互流程

1. 任意位置按 `Cmd+P` → 面板从窗口偏上位置浮出，输入框自动获得焦点
2. 输入字符 → `on_query` → 重新打分排序（同步，候选万级内 < 10ms）→ 列表刷新，选中项复位到第一条
3. `↑` / `↓` 移动选中，`Enter` 打开，`Esc` 或点击遮罩关闭并把焦点还给原位置
4. 回车后：关闭浮层 → `tabs::open_note(path)` → 命中已有标签则只激活（不新建、不重读盘）

## 6. 异常与边界

| 场景 | 处理 |
|---|---|
| 没有打开任何库 | 列表区显示"先在左侧打开一个库"，输入框仍可用（不报错、不空窗） |
| 库内没有可编辑文件 | 空态提示"库里还没有笔记" |
| 查询无匹配 | `Command::empty` 显示"没有匹配的文件" |
| 后台扫描失败（权限/磁盘） | 状态行提示失败原因，**保留已有缓存**，不弹系统弹窗 |
| 扫描进行中输入 | 先用缓存过滤，扫描完成后用新列表重算一次（不打断输入、不清空查询） |
| 待打开的文件已被外部删除 | `open_note` 自身会失败 → 面板照常关闭，状态栏已有提示（沿用现状，不新增分支） |
| 候选极多（万级） | 只取前 50 条装进 `Command`，避免列表测量与渲染开销 |

## 7. 测试计划

| 层 | 内容 |
|---|---|
| `core`（已有） | `tests/core/fuzzy.rs` 覆盖打分与命中位置（12 项，已合入） |
| `core`（新增） | 候选构造：树展平、相对路径、只保留 `is_editable_text` 文件 → 放 `tests/core/vault.rs` 或新 `tests/ui/command.rs` 的纯函数部分 |
| UI | `Command` 与 `Dialog` 的交互依赖窗口，本轮**不写自动测试**，靠人工验收清单 |
| 回归 | `cargo test` 全绿、`cargo clippy --all-targets` 无新增告警 |

## 8. 人工验收清单

- [ ] `Cmd+P` 在文件树 / 编辑区 / 预览区获得焦点时都能唤起
- [ ] 输入 2–3 个字符即可命中目标文件，且目标排在前三
- [ ] 文件名含中文时输入中文可命中，且不会 panic
- [ ] `↑`/`↓`/`Enter`/`Esc` 全部生效；`Esc` 后焦点回到唤起前的位置
- [ ] 点击遮罩可关闭
- [ ] 回车打开**已打开过的**笔记：不产生第二个标签，且未保存的编辑不被重载
- [ ] 空库 / 无匹配 时给出可读提示
- [ ] 万级文件的库：唤起不卡顿，输入不掉帧
- [ ] 面板关闭后编辑区输入正常（焦点没被吞）

## 9. 风险与开放问题

| # | 问题 | 需用户拍板 | 建议 |
|---|---|---|---|
| Q7 | 候选范围 | 是 | 只含 `is_editable_text`（`.md` + json/yaml/toml）；图片与其它文件不在面板里打开 |
| Q8 | 无匹配时是否提供"新建此名字的笔记" | 是 | 本轮不做，留到 T7.6 命令项一起 |
| Q9 | `Cmd+P` 与系统"打印" | 否 | 应用内不提供打印，直接占用；若实测被系统拦截再换 `Cmd+Shift+P` |
| 风险 | `Command` 的 `on_confirm` 给的是 `IndexPath`，与自排序列表的对应关系 | 否 | 用 `filterable(false)` + 只装过滤后的项，`IndexPath.item` 即 `matches` 下标；实施时先用日志核对一次 |
| 风险 | 首次唤起要扫描，可能有一瞬空白 | 否 | 用 `CommandState::set_loading` 显示加载态 |

## 10. 实施步骤（每步可独立验证）

| 步 | 内容 | 验收 |
|---|---|---|
| S1 | `OpenCommandPalette` action + `cmd-p` + `Dialog` 内空 `Command`（placeholder、Esc 关闭、焦点回落） | 面板能开能关 |
| S2 | 后台 `Vault::scan` 枚举候选 + 空库/无匹配空态 + loading | 能看到库内文件名 |
| S3 | 接 `core::fuzzy`：打分排序、取前 50、显示"相对路径" | 输入即过滤，排序合理 |
| S4 | `on_confirm` → `tabs::open_note` → 关闭；验证重复打开不新开标签 | 回车能打开，且不产生重复标签 |
| S5 | 命中字符高亮 + 候选数量上限 + 万文件库性能确认 | 高亮正确，大库不卡 |
