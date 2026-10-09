//! 视图层：只负责渲染与交互，业务规则在 core/

pub mod editor;
pub mod file_tree;
pub mod preview;
pub mod settings;
pub mod vault_manager;
pub mod tabs;

pub use editor::EditorPanel;
pub use file_tree::FilesPanel;
pub use preview::PreviewPanel;
pub use settings::open_settings;
pub use tabs::OpenTabs;

use std::time::{Duration, Instant};

/// 一帧预算（60FPS ≈ 16ms），超过一半就认为值得打日志
const SLOW_FRAME: Duration = Duration::from_millis(8);

/// 排查卡顿时给 render / 关键路径打点用。
/// 正常帧不会输出，只有明显超过预算时才打印。
pub fn log_slow(label: &str, started: Instant) {
    let elapsed = started.elapsed();
    if elapsed >= SLOW_FRAME {
        println!("[xink] ⏱ {label} 耗时 {elapsed:?}");
    }
}
