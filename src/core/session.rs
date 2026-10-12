//! Session（会话）：已打开标签的纯路径规则
//!
//! 只回答四个问题，不涉及任何 UI 概念，因此可以在 `#[test]` 里直接验证：
//! - 这个路径对应的标签是哪个（`tab_key`）
//! - 一串路径里哪些是重复项（`dedup_paths`）
//! - 哪些路径已经恢复不了（`partition_existing`）
//! - 活动标签失效后该激活谁（`resolve_active`）

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 标签的唯一标识：规范化后的绝对路径。
///
/// 用路径而非文件名——不同目录下允许同名笔记。
/// 用 canonicalize 而非原始字符串——软链接、`./a.md` 与 `/abs/a.md`
/// 指向同一文件时必须判为同一标签，否则会开出两个编辑同一文件的标签，互相覆盖。
/// 文件已不存在时 canonicalize 会失败，此时回落原路径，保证仍能作为键参与比较。
pub fn tab_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// 保序去重：按 [`tab_key`] 判定重复，保留第一次出现的位置。
///
/// 顺序即标签栏顺序，也是恢复顺序，绝不能因去重而改变。
pub fn dedup_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    paths
        .iter()
        .filter(|p| seen.insert(tab_key(p)))
        .cloned()
        .collect()
}

/// 拆分"仍可打开"与"已失效"的路径。
///
/// 失效指文件被删除或不再是普通文件；存在但读不了（权限、非 UTF-8）不在此列，
/// 那是加载阶段的事，需要保留在结果里由调用方尝试并给出提示。
pub fn partition_existing(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    paths.into_iter().partition(|p| p.is_file())
}

/// 活动标签失效时回落到最后一个仍可用的标签。
///
/// 存路径而不是下标正是为了这个：某个标签恢复失败后下标会整体错位，路径不会。
pub fn resolve_active(active: Option<PathBuf>, restored: &[PathBuf]) -> Option<PathBuf> {
    match active {
        Some(key) if restored.contains(&key) => Some(key),
        _ => restored.last().cloned(),
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/session.rs"]
mod tests;
