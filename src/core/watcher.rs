//! 库目录监听：感知外部编辑器对笔记的改动
//!
//! 笔记的唯一真值是磁盘文件，用户随时可能用别的编辑器改动它。
//! 这里只负责"发现变更"，如何取舍（自动重载还是提示）交给上层决定。

use anyhow::{Context as _, Result};
use flume::Receiver;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

use crate::core::media::{is_editable_text, is_image_path};

/// 变更类型。
///
/// 必须区分「内容被改」与「结构变了」：应用自身的自动保存会写入文件，
/// 属于 Modified，若据此重扫整棵文件树，编辑时会持续卡顿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchKind {
    Created,
    Removed,
    Modified,
    /// 重命名 / 移动。
    ///
    /// `notify` 把改名折叠为 `Modify(Name(..))`（macOS FSEvents 后端对
    /// `RenameMode::Both` 给出 `[旧路径, 新路径]`）。若与"内容被改"混为一谈，
    /// 在访达里改名后目录树不会更新——必须单独认出来。
    Renamed,
}

/// 把 `notify` 的事件种类映射为 [`WatchKind`]；与可见内容无关的事件返回 `None`。
///
/// 抽成纯函数是为了可测：这个映射决定了"哪些改动会触发重扫"，
/// 写死在闭包里就只能靠肉眼看目录树有没有刷新。
pub fn classify_kind(kind: &EventKind) -> Option<WatchKind> {
    match kind {
        EventKind::Create(_) => Some(WatchKind::Created),
        EventKind::Remove(_) => Some(WatchKind::Removed),
        EventKind::Modify(ModifyKind::Name(_)) => Some(WatchKind::Renamed),
        // 其余 Modify（Data / Metadata / Any）视为内容改动：
        // 自动保存走这条，据此重扫会让编辑时持续卡顿
        EventKind::Modify(_) => Some(WatchKind::Modified),
        // 访问、其它平台事件与可见内容无关
        _ => None,
    }
}

pub struct WatchEvent {
    pub paths: Vec<PathBuf>,
    pub kind: WatchKind,
}

/// 后台运行的目录监听器。
///
/// `notify` 的 watcher 必须保持存活才有效，因此这里同时持有它和接收端。
/// UI 线程通过 [`Self::poll`] 非阻塞地取出已到达的变更。
pub struct VaultWatcher {
    /// 接收端。交给 UI 的异步任务后即为 None，可实现零轮询的事件驱动。
    rx: Option<Receiver<WatchEvent>>,
    _watcher: RecommendedWatcher,
}

impl VaultWatcher {
    pub fn new(root: &Path) -> Result<Self> {
        let (tx, rx) = flume::unbounded();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else {
                return;
            };

            let Some(kind) = classify_kind(&event.kind) else {
                return;
            };

            let paths: Vec<PathBuf> = event
                .paths
                .into_iter()
                // 目录变化意味着结构变了；也忽略应用自身的临时文件
                .filter(|p| p.is_dir() || (is_visible_file(p) && !is_temp(p)))
                .collect();

            if !paths.is_empty() {
                let _ = tx.send(WatchEvent { paths, kind });
            }
        })
        .context("创建文件监听器失败")?;

        watcher
            .watch(root, RecursiveMode::Recursive)
            .with_context(|| format!("监听目录失败: {}", root.display()))?;

        Ok(Self {
            rx: Some(rx),
            _watcher: watcher,
        })
    }

    /// 取出接收端交给异步任务，之后变更由 `recv_async().await` 驱动，不再需要轮询。
    /// 只能取一次。
    pub fn take_receiver(&mut self) -> Option<Receiver<WatchEvent>> {
        self.rx.take()
    }

    /// 非阻塞取出当前已到达的全部变更（同步场景用）
    #[allow(dead_code)]
    pub fn poll(&self) -> Vec<WatchEvent> {
        let Some(rx) = self.rx.as_ref() else {
            return Vec::new();
        };
        let mut all = Vec::new();
        while let Ok(event) = rx.try_recv() {
            all.push(event);
        }
        all
    }
}

/// 目录树里会出现的文件：可编辑文本（笔记 + 数据文件）+ 图片。
/// 必须与 `vault::scan_dir` 的判据一致（都用 `is_editable_text`），
/// 否则会出现"新增的 json 在树里不刷新"的问题。
fn is_visible_file(path: &Path) -> bool {
    is_editable_text(path) || is_image_path(path)
}

/// 应用原子写盘产生的临时文件
fn is_temp(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".md.tmp")
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/watcher.rs"]
mod tests;
