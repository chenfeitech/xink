# 目录树文件类型图标 — 设计方案

> 状态：**待确认**（2026-10-12）· 确认后再开发
> 范围：`src/core/media.rs`（分类，可单测）、`src/ui/file_tree.rs`（图标映射）

---

## 1. 现状

树节点目前只有三类分支：

```rust
if is_dir {                    // 目录：FolderOpen / FolderClosed
} else if is_image_path(&path) {  // 图片：FileImage
} else {                       // 其余一律 FileText
}
```

即：**图片已有专属图标**，但 Markdown、代码、压缩包、音视频、数据文件全部共用
`FileText`，无法快速区分。

`is_image_path` 已覆盖 png / jpg / jpeg / gif / webp / bmp / svg / ico。

## 2. 目标

- 每种文件类型都有对应图标，一眼可辨；
- 与 Markdown 的标识方式保持一致（同属"文件"图标家族，风格统一）；
- 分类逻辑放领域层并可单测，UI 层只做"类型 → 图标"的映射。

## 3. 分类与图标映射

| 类型 | 扩展名 | 图标 | 备注 |
|---|---|---|---|
| 目录 | — | `FolderOpen` / `FolderClosed` | 已有，保持不变 |
| Markdown | `.md` `.markdown` | `FileText` | 与现状一致，作为"文档"基准 |
| 图片 | png/jpg/jpeg/gif/webp/bmp/svg/ico | `FileImage` | 已有 |
| 代码 | rs/py/js/ts/jsx/tsx/go/java/c/cpp/h/rb/sh | `FileCode` | 可用 ✓ |
| 数据 / 配置 | json/toml/yaml/yml/xml/csv/ini | `FileCode` | 无 `file-json`，归入代码类 |
| 压缩包 | zip/tar/gz/tgz/7z/rar | `FileArchive` | 可用 ✓ |
| 表格 | xls/xlsx/numbers | `FileSpreadsheet` | 可用 ✓ |
| 音频 | mp3/wav/flac/aac/m4a/ogg | `Music`（待确认） | 无 `file-audio` |
| 视频 | mp4/mov/mkv/avi/webm | `Video`（待确认） | 无 `file-video` |
| 其他 | 其余一切 | `FileText` | 兜底，保证"都有标识" |

> 图标名是否存在的核实结果：
> `file-text` ✓、`file-code` ✓、`file-image` ✓、`file-archive` ✓、
> `file-spreadsheet` ✓、`file-type` ✓、`file` ✓；
> `file-json` ✗、`file-pdf` ✗、`file-audio` ✗、`file-video` ✗。
> 后两类改用 `music` / `video`，实施时先确认，缺失则回落 `FileText`。

## 4. 实现结构

**领域层（`core/media.rs`，可单测）**

```rust
pub enum FileKind {
    Dir, Markdown, Image, Code, Archive, Spreadsheet, Audio, Video, Other,
}

/// 由路径与"是否目录"判定类型；目录优先
pub fn file_kind(path: &Path, is_dir: bool) -> FileKind
```

单测覆盖：每类挑 2–3 个扩展名、大小写（`.PNG`）、无扩展名、多点文件名
（`a.b.png`）、目录优先于扩展名。

**UI 层（`file_tree.rs`）**

```rust
fn kind_icon(kind: FileKind) -> AssetIcon {
    match kind {
        FileKind::Markdown | FileKind::Other => AssetIcon::FileText,
        FileKind::Image => AssetIcon::FileImage,
        FileKind::Code => AssetIcon::FileCode,
        FileKind::Archive => AssetIcon::FileArchive,
        FileKind::Spreadsheet => AssetIcon::FileSpreadsheet,
        FileKind::Audio => AssetIcon::Music,
        FileKind::Video => AssetIcon::Video,
        FileKind::Dir => AssetIcon::FolderClosed,
    }
}
```

树节点渲染时：目录仍走"展开箭头 + 文件夹"的既有逻辑，其余按 `file_kind` 取图标。

## 5. 约束

- 不改动点击行为（图片仍是在预览区打开，md 仍是打开标签）；
- 图标尺寸沿用 `icon_size` 配置，与现状一致；
- 未识别类型必须有兜底图标（不允许出现无图标的行）；
- 分类函数不得引入 UI 依赖（领域层约束）。

## 6. 验收

1. 目录中同时存在 md / 图片 / 代码 / 压缩包等文件时，图标各不相同
2. `.md` 与图片图标风格一致（同属文件图标家族）
3. 大写扩展名（`.PNG`）也识别为图片
4. 无扩展名、罕见扩展名的文件有兜底图标
5. 改"图标大小"配置后，各类图标同步变化
6. 明暗主题下均可辨识

## 7. 待确认

- 音频/视频是否值得单独分类（库里若几乎没有这类文件，可合并为"其他"）？
- 数据/配置（json/yaml）归入"代码"类是否合适，还是单独给图标？
EOF
echo written
---

## 8. 决策与实施（2026-10-12）

**决策**：音频/视频并入"其他"；json/yaml 归入普通文件，与 Markdown 统一处理。

最终映射：

| 类型 | 扩展名 | 图标 |
|---|---|---|
| 目录 | — | 文件夹（展开 / 折叠，不变） |
| 图片 | png/jpg/jpeg/gif/webp/bmp/svg/ico | `FileImage` |
| 代码 | rs/py/js/ts/go/java/c/cpp/sh 等 | `FileCode` |
| 压缩包 | zip/tar/gz/tgz/7z/rar/bz2/xz | `FileArchive` |
| 表格 | xls/xlsx/numbers/ods/csv | `FileSpreadsheet` |
| Markdown | md/markdown | `FileText` |
| 其他（含 json/yaml、音视频、未知） | 其余 | `FileText` |

实现：

- `core/media.rs`：`FileKind` 枚举 + `file_kind(path, is_dir)`（目录优先、大小写不敏感），
  6 个单测（目录优先、Markdown、图片大小写、代码/压缩包/表格、Other 兜底、多点文件名）。
- `ui/file_tree.rs`：`kind_icon(FileKind) -> AssetIcon`；树节点原来"图片 / 其他"两个分支
  合并为一个按类型取图标的分支。点击行为不变（图片仍在预览区打开，md 仍开标签）。

验证：146 passed、clippy 0 error、已 fmt。

**待人工验收**：目录里同时有 md / 图片 / 代码 / 压缩包 / 表格时图标各不相同；
json/yaml 与 md 同为文本文档图标；改图标大小后各类同步变化。
