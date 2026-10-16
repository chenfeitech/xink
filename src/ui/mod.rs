//! 视图层：只负责渲染与交互，业务规则在 core/

pub mod backlink;
pub mod command;
pub mod completion;
pub mod drawer;
pub mod editor;
pub mod file_tree;
pub mod outline;
pub mod preview;
pub mod rail;
pub mod search;
pub mod settings;
pub mod tabs;
pub mod tags;
pub mod vault_manager;

pub use backlink::BacklinkPanel;
pub use command::open_command_palette;
pub use editor::EditorPanel;
pub use file_tree::FilesPanel;
pub use outline::OutlinePanel;
pub use preview::PreviewPanel;
pub use rail::RailState;
pub use search::open_search;
pub use settings::open_settings;
pub use tabs::OpenTabs;
pub use tags::TagsPanel;

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
