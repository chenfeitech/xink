# 图片显示与预览

> 状态：已实现（2026-09-24）
> 涉及：`src/core/markdown.rs`、`src/ui/image_viewer.rs`、`src/ui/preview.rs`、`src/ui/file_tree.rs`

---

## 1. 问题与根因

两个现象其实是同一类问题的一半：

| 现象 | 直接原因 |
|---|---|
| 笔记里 `![a](path)` 显示为空白 / 破损 | Markdown 图片 URL 加载不出来 |
| 目录树里的图片文件看不出是图片 | 所有文件统一用 `FileText` 图标，且点击会当笔记打开 |

**根因**：gpui-base 把 Markdown 中的图片 URL 原样交给 `cx.http_client()`
（`gpui-pre` `elements/img.rs:635`），而桌面端默认装的是 `NullHttpClient`
（`gpui-pre` `app.rs:175-181`、`gpui-pre-platform` `gpui_platform.rs:19-21`）。
于是 **`file://`、绝对路径、相对路径全部加载失败**。

框架能加载图片的来源只有三条：

| 来源 | 是否可用 | 说明 |
|---|---|---|
| `data:` URL | ✅ | `gpui-base` text/utils.rs 的 `data_url_image` 内联解码 |
| `Resource::Path`（`img(PathBuf)`） | ✅ | `fs::read`（`img.rs:634`）—— 但 Markdown 里写不出这种形式 |
| `Resource::Uri`（http / file） | ❌ | 走 HTTP 客户端，桌面端没有 |

---

## 2. 实现方案

### 2.1 笔记里的本地图片 → `data:` URL

`core/markdown.rs::resolve_image_urls` 在交给预览解析器之前完成改写：

```
![图](shot.png)  ──►  [![图](data:image/png;base64,…)](file:///abs/shot.png)
                        └── 真正能显示的形式 ──┘   └── 点击时用来定位原图 ──┘
```

- 支持相对路径、绝对路径、`file://` 三种写法；远程 `http(s)://` 与已有 `data:` 保持原样；
- 转不出来（文件不存在 / 格式不支持 / 超 5 MB）时退回 `file://`，至少链接指向正确位置；
- 带缓存：以 `(mtime, 文件长度)` 为指纹缓存 data URL，避免每次刷新预览都读盘编码；
- 支持的格式：png / jpg / jpeg / gif / webp / bmp / svg / ico。

> 为什么不用"给应用装 HTTP 客户端"那条路：那只能解决远程图，本地 `file://`
> 依然会被当 URI 交给 HTTP 客户端；data URL 是框架唯一内联支持的通道。

### 2.1.1 显示方式与宽度（`image_display` / `image_width_percent`）

预处理按配置把图片改写成内联 HTML，从而绕过标准 Markdown 无法指定宽度的问题：

| 配置 | 改写结果 |
|---|---|
| 适应宽度 + 宽度 100%（默认） | `<img src="data:…" alt="…" style="width: 100%">` |
| 适应宽度 + 宽度 60% | `… style="width: 60%">` |
| 不显示 | 去掉 `!`，退化成 `[alt](file://…)` 文本链接 |

> 顺带解决一个体验问题：不指定宽度时，**图文同行**的图片会被压成 `line_height * 0.75`
> （`inline_flow.rs:1141`），显得特别小；一旦显式指定宽度就按指定值渲染。

配置变化时 `EditorPanel::render` 会检测到并重算预览源文（`refresh_doc` 内部有
"路径 + 版本"守卫，不会每帧重算）。

### 2.2 点击图片 → 打开预览窗口

框架**没有图片点击回调，只有链接回调**（`on_link_click`）。因此把图片包进一条
指向自身的链接（见上式），点击即触发回调：

- 回调判断链接指向本地图片 → `open_image_viewer` 打开预览窗口；
- 否则按 `preview.link_open` 配置处理：`InApp` 尝试作为笔记打开，否则系统浏览器。
  这也顺带接上了 `link_open` 配置项。

### 2.3 预览窗口（`src/ui/image_viewer.rs`）

独立窗口，用 `img(PathBuf)`（`Resource::Path` → `fs::read`，不经过 HTTP），
`ObjectFit::Contain` 等比缩放到窗口内，顶部显示文件名。

### 2.4 目录树里的图片文件

之前"目录树看不到图片"有两层原因，两层都要改：

| 层 | 问题 | 修复 |
|---|---|---|
| **扫描** | `vault::scan_dir` 只收集 `.md`（`else if is_markdown(&file_name)`），图片根本不在树里 | 加 `\|\| is_image_path(&path)` |
| **监听** | `watcher` 只上报 `.md` 与目录，新增图片不会触发刷新 | 改为 `is_visible_file()` = 笔记 + 图片 |

其余两点：

- **图标**：改用完整图标集 `gpui_kit::assets::IconName`（组件库内置的 101 项里没有图片图标），
  图片文件显示 `FileImage`，其余文件仍是 `FileText`；
- **点击**：图片文件打开预览窗口，非图片文件才走 `open_note`；
- **刷新**：`file_tree::handle_watch_event` 对 `Created / Removed` 重扫整棵树，
  因此新增 / 删除图片会立即反映；图片内容被修改（`Modified`）不重扫，
  避免编辑时的持续卡顿。

> 扫描与监听必须共用同一判据，否则会出现"重新打开库能看到、新增却不刷新"的割裂。
> 因此图片判定抽到了 `src/core/media.rs`，由树、监听、预处理、点击回调四处共用。

---

## 3. 限制与后续

| 限制 | 说明 | 后续 |
|---|---|---|
| 远程图片不显示 | 桌面端未安装 HTTP 客户端，远程 URL 保持原样但加载不出来 | 如需支持，给 `Application` 装一个真实 HTTP 客户端 |
| 单图 > 5 MB 不转 data URL | 避免 Markdown 文本与内存膨胀 | 可改为按需加载或改用图片扩展点 |
| 目录树只显示图标，非缩略图 | 缩略图要为每张图解码，成本高 | 可按需加（带缓存与尺寸上限） |
| "原始尺寸"无法实现 | 渲染层在三处硬编码 `max_w(relative(1.))`（`node.rs:1813`、`inline_flow.rs:170/1087`），Markdown 侧无法突破容器宽 | 该选项已从枚举与设置界面移除；配置里的旧值由 `Legacy` 变体兜底并夹回"适应宽度" |
| 远程图片不显示 | 同前 | 同前 |

---

## 5. 复用 Markdown 预览窗显示图片（2026-09-28 提出，方案待确认）

### 5.1 现状

点击图片目前调用 `open_image_viewer`（`ui/image_viewer.rs`）**每次新开一个独立窗口**。
而预览面板（`ui/preview.rs`）只渲染 Markdown：`PreviewPanel.doc: Option<Entity<TextViewState>>`
由 `ActiveDocChanged` 事件驱动，doc 为 `None` 时显示"没有打开的笔记"（`preview.rs:15-23,164`）。

### 5.2 推荐方案：预览面板增加"图片模式"（增量改造）

不改 `active_doc` 的现有语义，而是**新增**一条通道：

```rust
pub struct PreviewState {
    pub active_doc: Option<Entity<TextViewState>>,   // 不变
    pub image: Option<PathBuf>,                      // 新增：图片模式
}
```

| 环节 | 设计 |
|---|---|
| 点击图片 | 发 `PreviewImageChanged(Some(path))`，预览面板切换为图片渲染 |
| 回到笔记 | 切换/编辑笔记时（`ActiveDocChanged`）清掉 `image`，自动回到 Markdown |
| 渲染 | `image.is_some()` → `img(path).object_fit(Contain)`；否则走原 `TextView` 分支 |
| 滚动同步 | 图片模式下跳过 `apply_editor_ratio` / `tick_scroll_sync`（无 Markdown 可同步） |
| 空态 | 图片 + 无笔记时显示图片名，避免显示"没有打开的笔记"造成困惑 |

窗口管理的影响：不再每次弹窗，减少窗口冗余；`image_viewer.rs` 可保留为"在新窗口放大查看"的可选入口
（例如右键菜单），否则应删除以免留下死代码。

### 5.3 取舍与影响

| 维度 | 复用预览面板 | 保持独立窗口 |
|---|---|---|
| 一致性 | ✅ 与 Markdown 预览同一区域、同一套主题 | ❌ 额外窗口，风格割裂 |
| 窗口冗余 | ✅ 无新窗口 | ❌ 每点一次图多一个窗口 |
| 可视面积 | ⚠️ 三栏布局下右栏较窄，大图会缩小显示 | ✅ 可独立放大、拖动 |
| 改动面 | 中：`PreviewState` 加字段 + 新事件 + render 分支 + 同步跳过 | 无 |

结论：**默认复用预览面板**；若要兼顾"看大图"，保留独立窗口作为二级入口（不在默认点击路径上）。

## 4. 验证

单测（`src/core/markdown/tests.rs`）：相对路径转 data URL、包链接、文件缺失退回、
base64 标准向量、扩展名判定的大小写无关性。全仓 `cargo test` **90 passed / 0 failed**。

人工验证：

1. 笔记中 `![a](相对路径.png)` → 预览区显示图片；点击 → 弹出预览窗口
2. 绝对路径、`file://` 写法的图片同样能显示并点击
3. 目录树中 `.png/.jpg` 等显示为图片图标；点击 → 预览窗口（不会当笔记打开）
4. 普通 `.md` 文件点击行为不变；切换笔记、保存、滚动同步无回归
