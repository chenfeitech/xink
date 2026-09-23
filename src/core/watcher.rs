//! 库目录监听：感知外部编辑器对笔记的改动
//!
//! 笔记的唯一真值是磁盘文件，用户随时可能用别的编辑器改动它。
//! 这里只负责"发现变更"，如何取舍（自动重载还是提示）交给上层决定。

use anyhow::{Context as _, Result};
use flume::Receiver;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

/// 变更类型。
///
/// 必须区分「内容被改」与「结构变了」：应用自身的自动保存会写入文件，
/// 属于 Modified，若据此重扫整棵文件树，编辑时会持续卡顿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchKind {
    Created,
    Removed,
    Modified,
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

            let kind = match event.kind {
                EventKind::Create(_) => WatchKind::Created,
                EventKind::Remove(_) => WatchKind::Removed,
                EventKind::Modify(_) => WatchKind::Modified,
                // 其余（访问、其它平台事件）与可见内容无关
                _ => return,
            };

            let paths: Vec<PathBuf> = event
                .paths
                .into_iter()
                // 目录变化意味着结构变了；也忽略应用自身的临时文件
                .filter(|p| p.is_dir() || (is_markdown(p) && !is_temp(p)))
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

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// 应用原子写盘产生的临时文件
fn is_temp(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".md.tmp")
}

#[cfg(test)]
mod tests;
