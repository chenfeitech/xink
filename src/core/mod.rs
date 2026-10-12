//! 领域层：与 UI 完全解耦，可独立单元测试
//!
//! 本层不得引用任何 `gpui_kit::component::*` 组件类型或样式 API。

pub mod config;
pub mod datafile;
pub mod fs;
pub mod highlight;
pub mod markdown;
pub mod media;
pub mod note;
pub mod session;
pub mod vault;
pub mod watcher;

pub use note::Note;
pub use session::{dedup_paths, partition_existing, resolve_active, tab_key};
pub use vault::{FileNode, Vault};
pub use watcher::VaultWatcher;
