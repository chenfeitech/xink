//! Vault（库）：一个被打开的本地文件夹，是一批 Markdown 笔记的根
//!
//! 磁盘上的文件是唯一真值，本模块只负责枚举与增删改移，不做任何缓存决策。

use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::media::{is_editable_text, is_image_path};

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
    pub fn scan(&self, show_all: bool) -> Result<FileNode> {
        scan_dir(&self.root, show_all)
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
/// 是否跳过该条目。
///
/// - `.git` / `node_modules` / `target` / `.DS_Store`：**始终跳过**——展开后节点数以万计，会拖垮扫描与渲染；
/// - `*.md.tmp`：应用自己的临时文件，始终跳过；
/// - 点开头的其余隐藏项：由 `show_hidden` 决定（默认显示）。
pub fn should_ignore(name: &str, show_all: bool) -> bool {
    matches!(name, ".git" | "node_modules" | "target" | ".DS_Store")
        || name.ends_with(".md.tmp")
        || (!show_all && name.starts_with('.'))
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

fn scan_dir(dir: &Path, show_all: bool) -> Result<FileNode> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());

    let mut children = Vec::new();
    let entries = fs::read_dir(dir).with_context(|| format!("读取目录失败: {}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if should_ignore(&file_name, show_all) {
            continue;
        }
        if path.is_dir() {
            children.push(scan_dir(&path, show_all)?);
        } else if show_all || is_editable_text(&path) || is_image_path(&path) {
            // 默认只显示能编辑的文本（笔记 + json/yaml/toml）与图片
            // （图片可在树中直接点开预览）；
            // 打开"显示全部"后，其他类型的文件也进树，便于查看与引用。
            //
            // 判据必须用 `is_editable_text`：若这里放行而 `tabs::open_note`
            // 不放行，就会出现"树里看得到、双击没反应"。
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

// ── 多库：注册表条目与路径校验 ────────────────────────────────
//
// 设计见 docs/multi-vault-design.md。`Vault` 是无身份的运行时句柄（只有 root），
// 注册表条目则需要稳定的 id 与用户可见的名称——路径可以被"重新定位"改掉，
// 而 id 不变，将来挂在库上的状态（库级配置等）才不会丢。

/// 一个已注册的库
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    /// 稳定 id，新增时派生；路径变更保持不变
    pub id: String,
    /// 显示名，默认取目录名
    pub name: String,
    pub path: PathBuf,
}

/// 库路径的可用状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultStatus {
    /// 可用
    Ok,
    /// 路径不存在（被移动 / 删除 / 所在磁盘未挂载）
    Missing,
    /// 路径是文件而不是目录
    NotDir,
    /// 无读权限
    NoPermission,
    /// 与其它库路径重复
    Duplicate,
    /// 被其它库包含：允许，但要提示"笔记会被重复计入"
    Nested,
}

impl VaultStatus {
    /// 该状态下的库能否被打开
    pub fn is_usable(self) -> bool {
        matches!(self, VaultStatus::Ok | VaultStatus::Nested)
    }

    /// 面向用户的说明
    pub fn message(self) -> &'static str {
        match self {
            VaultStatus::Ok => "可用",
            VaultStatus::Missing => "路径不存在",
            VaultStatus::NotDir => "不是目录",
            VaultStatus::NoPermission => "无读取权限",
            VaultStatus::Duplicate => "与其它库路径重复",
            VaultStatus::Nested => "被其它库包含，笔记会被重复计入",
        }
    }
}

impl VaultEntry {
    /// 新建条目；名称为空时回落到目录名
    pub fn new(path: PathBuf, name: impl Into<String>) -> Self {
        let name = name.into();
        let name = if name.trim().is_empty() {
            Self::default_name(&path)
        } else {
            name
        };
        Self {
            id: Self::derive_id(&path),
            name,
            path,
        }
    }

    /// 目录名作为默认显示名
    pub fn default_name(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string())
    }

    /// 派生 id：路径哈希 + 时间戳。
    /// 只需在注册表内唯一，不为此引入 uuid 依赖。
    fn derive_id(path: &Path) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("v{:x}-{:x}", hasher.finish(), nanos)
    }

    /// 路径自身的可用状态（不关心与其它库的关系）
    pub fn path_status(&self) -> VaultStatus {
        if !self.path.exists() {
            return VaultStatus::Missing;
        }
        if !self.path.is_dir() {
            return VaultStatus::NotDir;
        }
        if fs::read_dir(&self.path).is_err() {
            return VaultStatus::NoPermission;
        }
        VaultStatus::Ok
    }

    /// 综合状态：先看自身路径是否可用，再看与其它库是否重复 / 嵌套
    pub fn status(&self, others: &[VaultEntry]) -> VaultStatus {
        let own = self.path_status();
        if own != VaultStatus::Ok {
            return own;
        }

        let self_key = fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        for other in others {
            if other.id == self.id {
                continue;
            }
            let other_key = fs::canonicalize(&other.path).unwrap_or_else(|_| other.path.clone());
            if other_key == self_key {
                return VaultStatus::Duplicate;
            }
            if self_key.starts_with(&other_key) {
                return VaultStatus::Nested;
            }
        }
        VaultStatus::Ok
    }
}

/// 两个路径是否指向同一目录（`canonicalize` 失败时退回字面比较）。
/// 用于判断"这个目录是不是已经注册过的库"。
pub fn same_path(a: &Path, b: &Path) -> bool {
    let ka = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let kb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ka == kb
}

/// 库注册表：已注册的库 + 当前选中的库
///
/// 放在 `core` 而不是 `state`，因为"哪个库可用、该选哪个"是领域规则；
/// `state/workspace.rs` 只负责它的读写。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultRegistry {
    pub vaults: Vec<VaultEntry>,
    /// 当前库 id；`None` 表示还没选过库
    pub active: Option<String>,
}

impl VaultRegistry {
    /// 按 id 查找
    pub fn find(&self, id: &str) -> Option<&VaultEntry> {
        self.vaults.iter().find(|v| v.id == id)
    }

    /// 当前选中的库；id 不在列表中（例如库已被移除）时视为未选择
    pub fn active_entry(&self) -> Option<&VaultEntry> {
        self.active.as_deref().and_then(|id| self.find(id))
    }

    /// 当前库的路径
    pub fn active_root(&self) -> Option<PathBuf> {
        self.active_entry().map(|e| e.path.clone())
    }

    /// 启动时解析当前库。
    ///
    /// - active 存在且可用 → 保持不变；
    /// - active 缺失或失效 → 回落到第一个可用库；
    /// - 全部不可用 → 未选库（由上层提示用户打开/添加库）。
    ///
    /// 返回需要告知用户的提示（失效原因、已回落等）。**调用方不得因这些提示阻塞启动**。
    pub fn resolve_active(&mut self) -> Vec<String> {
        let mut notes = Vec::new();

        // 先收集可用库的 id，避免后面修改 self.active 时与借用冲突
        let usable_ids: Vec<String> = self
            .vaults
            .iter()
            .filter(|v| v.status(&self.vaults).is_usable())
            .map(|v| v.id.clone())
            .collect();

        for v in &self.vaults {
            let status = v.status(&self.vaults);
            if !status.is_usable() {
                notes.push(format!("库「{}」{}", v.name, status.message()));
            }
        }

        let active_is_usable = self
            .active_entry()
            .map(|e| usable_ids.contains(&e.id))
            .unwrap_or(false);
        if active_is_usable {
            return notes;
        }

        let previous_name = self.active_entry().map(|e| e.name.clone());
        match usable_ids.first() {
            Some(id) => {
                self.active = Some(id.clone());
                let new_name = self.find(id).map(|e| e.name.clone()).unwrap_or_default();
                match previous_name {
                    Some(old) => notes.push(format!("原库「{old}」已失效，已切换到「{new_name}」")),
                    None => notes.push(format!("已选择库「{new_name}」")),
                }
            }
            None => {
                self.active = None;
                if !self.vaults.is_empty() {
                    notes.push("所有库路径都不可用，请重新打开或添加库".to_string());
                }
            }
        }
        notes
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/vault.rs"]
mod tests;
