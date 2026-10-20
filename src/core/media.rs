//! 媒体文件判定
//!
//! 图片判据被多处共用，必须只有一份：目录树据此决定显示哪些文件、
//! 文件监听据此决定是否上报变更、Markdown 预处理据此决定是否转成 data URL、
//! 点击回调据此决定是否打开预览。若各写一份，就会出现"树里看得到但预览打不开"
//! 这类不一致。
//!
//! [`is_editable_text`] 同理：它是"哪些文件能进编辑器"的唯一判据，
//! 目录树、文件监听、打开入口都用它，避免"树里看得到但双击打不开"。

use std::path::Path;

use crate::core::datafile::{data_kind, is_data_file};
use crate::core::note::is_markdown;

/// 能否在编辑器里打开：Markdown 笔记 + 数据文件（JSON / YAML / TOML）
///
/// 这是"能打开"的唯一判据。目录树（`vault::scan_dir`）、文件监听
/// （`watcher::is_visible_file`）、打开入口（`tabs::open_note`）都用它，
/// 否则会出现"树里能看到但双击没反应"。
pub fn is_editable_text(path: &Path) -> bool {
    is_markdown(path) || is_data_file(path)
}

/// 是否按图片处理
pub fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png")
            | Some("jpg")
            | Some("jpeg")
            | Some("gif")
            | Some("webp")
            | Some("bmp")
            | Some("svg")
            | Some("ico")
    )
}

/// 文件类型：目录树用来区分图标
///
/// 只做**分类**，不含任何 UI 依赖；图标映射在 UI 层完成。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// 目录
    Dir,
    /// Markdown 笔记
    Markdown,
    /// 图片
    Image,
    /// 代码源文件
    Code,
    /// 数据文件（json / yaml / toml）：可编辑、预览显示格式化结果
    Data,
    /// 压缩包
    Archive,
    /// 表格
    Spreadsheet,
    /// 音频
    Audio,
    /// 视频
    Video,
    /// 其余一切（未知扩展名）
    Other,
}

/// 判定文件类型。
///
/// `is_dir` 优先：目录不看扩展名（存在名为 `x.md` 的目录也不应判成笔记）。
/// 未识别的扩展名一律落到 `Other`，由 UI 层给出兜底图标。
pub fn file_kind(path: &Path, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Dir;
    }
    if is_image_path(path) {
        return FileKind::Image;
    }
    let Some(ext) = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
    else {
        return FileKind::Other;
    };

    match ext.as_str() {
        "md" | "markdown" => FileKind::Markdown,
        // 数据文件单独一类：需要自己的图标，且是可编辑的
        "json" | "yaml" | "yml" | "toml" => FileKind::Data,
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cpp" | "cc" | "h"
        | "hpp" | "rb" | "sh" | "bash" | "zsh" | "swift" | "kt" | "lua" | "php" | "sql" => {
            FileKind::Code
        }
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "bz2" | "xz" => FileKind::Archive,
        "xls" | "xlsx" | "numbers" | "ods" | "csv" => FileKind::Spreadsheet,
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "oga" | "opus" | "wma" | "aiff"
        | "aif" => FileKind::Audio,
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" | "mpg" | "mpeg" | "wmv" | "flv" => {
            FileKind::Video
        }
        // 未识别的扩展名按设计决策归入 Other，与"普通文件"统一呈现
        _ => FileKind::Other,
    }
}

/// 编辑区 `set_highlighter` 的语言名
///
/// 只返回**已启用 feature** 的语言：json / yaml / toml / markdown。
/// 传未注册的名字（或大小写不对，如 "Markdown"）时，框架只打一条
/// `tracing::warn` 就静默退化成纯文本，不报错也不提示——所以宁可返回
/// `"text"` 也不要去猜，猜错的表现是"看起来什么都没发生"，极难排查。
///
/// 数据文件的语言名复用 [`crate::core::datafile::DataKind::lang`]，
/// 避免"预览围栏标签"和"编辑区高亮"两处各写一份扩展名列表而漂移。
pub fn editor_language(path: &Path) -> &'static str {
    if let Some(kind) = data_kind(path) {
        return kind.lang();
    }
    if is_markdown(path) {
        return "markdown";
    }
    "text"
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/media.rs"]
mod tests;
