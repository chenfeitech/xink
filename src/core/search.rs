//! 全文检索：在候选文件里逐行找关键词。
//!
//! 设计要点：
//! - **不预存正文**：索引只存事实（T6 §3 决定），检索时按需读文件；
//!   代价是每次搜索要读盘，收益是内存与磁盘内容永远一致；
//! - 调用方把它丢到后台线程（读盘是同步 IO），本模块只提供纯函数；
//! - 大小写不敏感（`to_lowercase()`，中文不受影响）。

use std::path::{Path, PathBuf};

/// 检索结果条数上限：结果太多时后面的既显示不下也没有意义
const MAX_HITS: usize = 200;
/// 超过这个大小的文件跳过：MB 级的笔记逐行扫描会明显拖慢搜索
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// 一条命中
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// 0-based 行号，供跳转
    pub line: usize,
    /// 命中所在行的文本（已去掉换行），用于展示上下文
    pub text: String,
}

/// 在 `files` 里检索 `query`，返回命中（按文件顺序、行号升序）。
///
/// 读不到的文件（权限、二进制、被删除）直接跳过，不算错误：
/// 搜索不该因为一个文件打不开就整体失败。
pub fn search_files(files: &[PathBuf], query: &str, limit: usize) -> Vec<Hit> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let needle = query.to_lowercase();
    let limit = limit.min(MAX_HITS);

    let mut hits = Vec::new();
    for path in files {
        if hits.len() >= limit {
            break;
        }
        if let Ok(meta) = std::fs::metadata(path)
            && meta.len() > MAX_FILE_BYTES
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for (ix, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                hits.push(Hit {
                    path: path.clone(),
                    line: ix,
                    text: trim_context(line),
                });
                if hits.len() >= limit {
                    break;
                }
            }
        }
    }
    hits
}

/// 上下文截断：超长的行（例如压缩过的 JSON）只保留前后各一段
fn trim_context(line: &str) -> String {
    const MAX_CHARS: usize = 160;
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= MAX_CHARS {
        return line.trim().to_string();
    }
    let head: String = chars[..80].iter().collect();
    let tail: String = chars[chars.len() - 60..].iter().collect();
    format!("{head}…{tail}")
}

/// 文件名是否值得参与检索（与索引口径一致：只收 Markdown）
#[allow(dead_code)]
pub fn is_searchable(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/search.rs"]
mod tests;
