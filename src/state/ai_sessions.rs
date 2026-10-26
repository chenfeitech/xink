//! AI 会话持久化：按笔记保存对话块
//!
//! 设计见 `docs/ai/ai-chat-blocks-design.md` §6.1。
//!
//! 两个约定：
//! 1. **会话按笔记归属**：面板即笔记（`ui::tabs::OpenTabs` 按路径持有面板），
//!    而系统提示里带的是当前笔记的正文，语义上"这段对话就是关于这篇笔记的"。
//! 2. **可丢、不可阻塞**：读不到 / JSON 损坏一律当作"没有历史"，
//!    写失败只返回 `Err`，由调用方决定要不要提示——绝不打断对话。
//!
//! 存的是**展示用的块**（`StoredBlock`），不是发给模型的 `history`：
//! 后者随时能从块重建，且模型上下文有长度预算，不值得落盘。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::workspace::{app_data_dir, read_json, write_json_atomic};

/// 会话数上限：超了丢最旧的，文件不能无限长
pub const MAX_SESSIONS: usize = 30;
/// 单个会话保留的块数上限
pub const MAX_BLOCKS: usize = 200;

/// 一个可序列化的对话块（`ui::ai::Block` 的镜像）
///
/// 折叠状态不落盘：加载后按各类型的默认值重建（过程块收起、正文展开）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoredBlock {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    Tool {
        title: String,
        output: String,
    },
    Verify {
        detail: String,
    },
    /// diff 的一行
    ///
    /// `kind` 直接存符号（`' '` 未变 / `'+'` 新增 / `'-'` 删除），
    /// 省掉一个枚举的 serde 配置。
    Diff {
        added: usize,
        removed: usize,
        applied: bool,
        lines: Vec<(char, String)>,
    },
}

fn file_path() -> PathBuf {
    app_data_dir().join("ai_sessions.json")
}

/// 读整张表；缺失或损坏都返回空表
pub fn load_all() -> HashMap<PathBuf, Vec<StoredBlock>> {
    read_json::<HashMap<PathBuf, Vec<StoredBlock>>>(&file_path()).unwrap_or_default()
}

/// 读某篇笔记的会话
pub fn load_note(path: &Path) -> Vec<StoredBlock> {
    load_all().remove(path).unwrap_or_default()
}

/// 写某篇笔记的会话；空块列表等于删除
pub fn save_note(path: &Path, blocks: &[StoredBlock]) -> Result<()> {
    let mut all = load_all();
    if blocks.is_empty() {
        all.remove(path);
    } else {
        all.insert(path.to_path_buf(), blocks.to_vec());
    }
    trim(&mut all);
    write_json_atomic(&file_path(), &all)
}

/// 删除某篇笔记的会话
pub fn clear_note(path: &Path) -> Result<()> {
    save_note(path, &[])
}

/// 裁剪：只保留最近 `MAX_SESSIONS` 个会话、每个最多 `MAX_BLOCKS` 块
///
/// "最近"按插入顺序（`HashMap` 无序，这里由调用方的写入顺序决定），
/// 超限时丢最早写入的那些。
fn trim(all: &mut HashMap<PathBuf, Vec<StoredBlock>>) {
    for blocks in all.values_mut() {
        let len = blocks.len();
        if len > MAX_BLOCKS {
            blocks.drain(..len - MAX_BLOCKS);
        }
    }
    if all.len() > MAX_SESSIONS {
        // HashMap 没有可靠的"最旧"，按路径排序取一个稳定子集：
        // 语义上是"只保留 30 篇笔记的会话"，顺序不重要，稳定才重要。
        let mut keys: Vec<PathBuf> = all.keys().cloned().collect();
        keys.sort();
        for key in keys.iter().take(all.len() - MAX_SESSIONS) {
            all.remove(key);
        }
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/state/ai_sessions.rs"]
mod tests;
