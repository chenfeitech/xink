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

#[cfg(test)]
mod tests;
