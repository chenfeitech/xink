//! Vault（库）：一个被打开的本地文件夹，是一批 Markdown 笔记的根
//!
//! 磁盘上的文件是唯一真值，本模块只负责枚举与增删改移，不做任何缓存决策。

use anyhow::{Context as _, Result, ensure};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::media::is_image_path;

/// 文件树节点（领域数据，不含 UI 状态）
#[derive(Debug, Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
}

pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn open(root: PathBuf) -> Result<Self> {
        ensure!(root.is_dir(), "不是一个目录: {}", root.display());
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 递归扫描整个库。
    ///
    /// 取舍：当前为全量扫描（MVP）。`TreeState` 需要完整的 `TreeItem` 树才能渲染展开箭头，
    /// 因此暂不做子目录懒加载；万级文件规模的扫描耗时在可接受范围。
    /// 后续由 `core/watcher.rs` 的增量事件驱动刷新。
    pub fn scan(&self) -> Result<FileNode> {
        scan_dir(&self.root)
    }

    /// 在 dir 下新建笔记，返回最终路径（自动补 .md 后缀）
    pub fn create_note(&self, dir: &Path, name: &str) -> Result<PathBuf> {
        let path = dir.join(ensure_md_suffix(name));
        ensure!(!path.exists(), "已存在同名文件: {}", path.display());
        fs::write(&path, "").with_context(|| format!("创建笔记失败: {}", path.display()))?;
        Ok(path)
    }

    pub fn create_dir(&self, dir: &Path, name: &str) -> Result<PathBuf> {
        let path = dir.join(name);
        ensure!(!path.exists(), "已存在同名目录: {}", path.display());
        fs::create_dir_all(&path).with_context(|| format!("创建目录失败: {}", path.display()))?;
        Ok(path)
    }

    /// 删除文件或目录（含非空目录）。调用方需先做二次确认。
    pub fn remove(&self, path: &Path) -> Result<()> {
        ensure!(path != self.root, "不允许删除库根目录");
        ensure!(path.starts_with(&self.root), "只能删除库内的路径");
        if path.is_dir() {
            fs::remove_dir_all(path)
                .with_context(|| format!("删除目录失败: {}", path.display()))?;
        } else {
            fs::remove_file(path).with_context(|| format!("删除文件失败: {}", path.display()))?;
        }
        Ok(())
    }

    pub fn rename(&self, from: &Path, new_name: &str) -> Result<PathBuf> {
        ensure!(from.is_file(), "只能重命名文件: {}", from.display());
        let name = ensure_md_suffix(new_name);
        let to = from.parent().unwrap_or(&self.root).join(name);
        ensure!(!to.exists(), "已存在同名文件: {}", to.display());
        fs::rename(from, &to).with_context(|| format!("重命名失败: {}", from.display()))?;
        Ok(to)
    }

    pub fn move_to(&self, from: &Path, to_dir: &Path) -> Result<PathBuf> {
        ensure!(to_dir.is_dir(), "目标不是目录: {}", to_dir.display());
        let file_name = from
            .file_name()
            .context("源路径没有文件名")?
            .to_string_lossy()
            .to_string();
        let to = to_dir.join(file_name);
        ensure!(!to.exists(), "目标已存在同名项: {}", to.display());
        fs::rename(from, &to).with_context(|| format!("移动失败: {}", from.display()))?;
        Ok(to)
    }
}

/// 默认忽略规则：版本控制目录、依赖目录、隐藏文件、应用自身临时文件
pub fn should_ignore(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "target" | ".DS_Store")
        || name.starts_with('.')
        || name.ends_with(".md.tmp")
}

fn is_markdown(name: &str) -> bool {
    name.to_lowercase().ends_with(".md")
}

fn ensure_md_suffix(name: &str) -> String {
    if is_markdown(name) {
        name.to_string()
    } else {
        format!("{name}.md")
    }
}

fn scan_dir(dir: &Path) -> Result<FileNode> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());

    let mut children = Vec::new();
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录失败: {}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if should_ignore(&file_name) {
            continue;
        }
        if path.is_dir() {
            children.push(scan_dir(&path)?);
        } else if is_markdown(&file_name) || is_image_path(&path) {
            // 目录树里显示笔记与图片：图片可在树中直接点开预览
            children.push(FileNode {
                path,
                name: file_name,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }

    // 目录在前，同层内按名称排序
    children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(FileNode {
        path: dir.to_path_buf(),
        name,
        is_dir: true,
        children,
    })
}

#[cfg(test)]
mod tests;
