//! 媒体文件判定
//!
//! 图片判据被多处共用，必须只有一份：目录树据此决定显示哪些文件、
//! 文件监听据此决定是否上报变更、Markdown 预处理据此决定是否转成 data URL、
//! 点击回调据此决定是否打开预览。若各写一份，就会出现"树里看得到但预览打不开"
//! 这类不一致。

use std::path::Path;

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
    /// 压缩包
    Archive,
    /// 表格
    Spreadsheet,
    /// 其余一切（含 json/yaml 等数据文件、音视频、未知扩展名）
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
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cpp" | "cc" | "h"
        | "hpp" | "rb" | "sh" | "bash" | "zsh" | "swift" | "kt" | "lua" | "php" | "sql" => {
            FileKind::Code
        }
        "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "bz2" | "xz" => FileKind::Archive,
        "xls" | "xlsx" | "numbers" | "ods" | "csv" => FileKind::Spreadsheet,
        // json / yaml / 音视频等按设计决策归入 Other，与"普通文件"统一呈现
        _ => FileKind::Other,
    }
}

#[cfg(test)]
mod tests;
