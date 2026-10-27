//! 工作区与布局持久化
//!
//! 布局与库路径都是可重建的派生数据；
//! 笔记正文的唯一真值始终是磁盘上的 .md 文件，不存这里。

use anyhow::Result;
use gpui_kit::component::dock::DockAreaState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::vault::{VaultEntry, VaultRegistry};
use crate::core::{dedup_paths, partition_existing};

const APP_DIR_NAME: &str = "xink";

/// 应用数据目录（macOS: ~/Library/Application Support/xink）
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

pub fn layout_path() -> PathBuf {
    app_data_dir().join("layout.json")
}

pub fn vault_path() -> PathBuf {
    app_data_dir().join("vault.json")
}

/// 用户配置文件（基本 / 编辑器 / 预览三类设置）
pub fn config_path() -> PathBuf {
    app_data_dir().join("config.json")
}

pub fn save_layout(state: &DockAreaState) -> Result<()> {
    write_json(&layout_path(), state)
}

pub fn load_layout() -> Result<DockAreaState> {
    read_json(&layout_path())
}

/// 记住上次打开的库，供下次启动自动恢复
#[derive(Serialize, Deserialize)]
struct LastVault {
    root: PathBuf,
}

// ---- 多库注册表（设计见 docs/multi-vault-design.md）----
//
// 旧的 `vault.json`（单个 root）已不再写入，只作为迁移来源被读取：
// 首次启动时由 `load_vaults` 把其中的路径转成一条库记录。

pub fn vaults_path() -> PathBuf {
    app_data_dir().join("vaults.json")
}

/// 保存库注册表（`VaultRegistry` 定义在 `core::vault`，这里只负责读写）
pub fn save_vaults(reg: &VaultRegistry) -> Result<()> {
    write_json_atomic(&vaults_path(), reg)
}

/// 读取库注册表。
///
/// 宽容策略（与 `AppConfig::load` 一致）：文件缺失或 JSON 损坏都返回空注册表，
/// **绝不阻塞启动**。若注册表为空且旧的 `vault.json` 有效，自动迁移成一条记录。
pub fn load_vaults() -> VaultRegistry {
    load_vaults_at(&vaults_path(), &vault_path())
}

/// 带路径参数的版本：测试可在临时目录里跑，避免污染真实的应用数据目录
fn load_vaults_at(vaults_file: &Path, legacy_file: &Path) -> VaultRegistry {
    if let Ok(reg) = read_json::<VaultRegistry>(vaults_file) {
        return reg;
    }
    // 首次升级：旧的单库记录迁移成一条库
    match migrate_last_vault(legacy_file) {
        Some(reg) => {
            // 写回迁移结果，下次启动无需再迁移；写失败不影响本次启动
            let _ = write_json_atomic(vaults_file, &reg);
            reg
        }
        None => VaultRegistry::default(),
    }
}

/// 旧版 `vault.json`（单个 root）→ 一条库记录；目录已失效则返回 None
fn migrate_last_vault(legacy_file: &Path) -> Option<VaultRegistry> {
    let last: LastVault = read_json(legacy_file).ok()?;
    if !last.root.is_dir() {
        return None;
    }
    let entry = VaultEntry::new(last.root.clone(), VaultEntry::default_name(&last.root));
    let id = entry.id.clone();
    Some(VaultRegistry {
        vaults: vec![entry],
        active: Some(id),
    })
}

/// 原子写 JSON：先写同目录临时文件再 rename。
///
/// 状态文件损坏虽然只会回落到默认值，但中途崩溃留下半截文件仍会让用户
/// 莫名其妙地"丢设置"，所以这里同样按 `.codebuddy/rules/04-notes-domain.md` 走原子写。
pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, json)?;
    Ok(())
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let json = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

// ---- 已打开的笔记（会话恢复） ----

/// `open_notes.json` 的内容。
///
/// `paths` 的顺序即标签顺序，也是恢复顺序；`active` 存路径而不是下标——
/// 某条记录失效后下标会整体错位，路径不会。两个字段都 `default`，
/// 旧版本只写了 `paths`，新版本仍能读。
#[derive(Serialize, Deserialize, Default)]
struct OpenNotes {
    #[serde(default)]
    paths: Vec<PathBuf>,
    #[serde(default)]
    active: Option<PathBuf>,
}

/// 恢复所需的会话快照：已过滤掉失效路径，可直接用于逐个打开。
pub struct OpenNotesSnapshot {
    /// 仍存在的笔记，按上次的标签顺序排列
    pub paths: Vec<PathBuf>,
    /// 上次活动的标签；已保证落在 `paths` 内
    pub active: Option<PathBuf>,
}

fn open_notes_path() -> PathBuf {
    app_data_dir().join("open_notes.json")
}

/// 记录当前打开的笔记。
///
/// 写入前做保序去重并按照仍有 active 落在 paths 内校正，
/// 保证落盘的永远是"可直接恢复"的状态。
pub fn save_open_notes(paths: &[PathBuf], active: Option<&PathBuf>) -> Result<()> {
    write_json(&open_notes_path(), &normalize_open_notes(paths, active))
}

/// 落盘前的规范化：保序去重，并保证 `active` 一定落在 `paths` 内
///
/// 抽出来是为了能单测——`save_open_notes` 本身写在真实的应用数据目录，
/// 测试不能碰（会把用户正打开着的标签清掉）。
fn normalize_open_notes(paths: &[PathBuf], active: Option<&PathBuf>) -> OpenNotes {
    let paths = dedup_paths(paths);
    let active = active_within(active.cloned(), &paths);
    OpenNotes { paths, active }
}

/// `active` 不在 `paths` 里就当没有
///
/// 恢复时不能凭空激活一篇根本没打开的笔记，否则启动会打开一篇用户没开过的文件。
fn active_within(active: Option<PathBuf>, paths: &[PathBuf]) -> Option<PathBuf> {
    active.filter(|p| paths.contains(p))
}

/// 读取历史打开的笔记。
///
/// 文件不存在、JSON 损坏都视为"没有历史记录"，绝不因此阻止启动；
/// 已删除的路径只记日志不报错——用户删了笔记是正常行为，不是故障。
pub fn load_open_notes() -> OpenNotesSnapshot {
    let notes: OpenNotes = match read_json(&open_notes_path()) {
        Ok(notes) => notes,
        Err(err) => {
            // 文件不存在 = 首次启动，无需提示；存在却读不了则记录，但绝不因此阻止启动
            if open_notes_path().exists() {
                eprintln!("[xink] 历史打开记录无法解析，按空记录处理: {err:?}");
            }
            return OpenNotesSnapshot {
                paths: Vec::new(),
                active: None,
            };
        }
    };

    let (paths, missing) = partition_existing(notes.paths);
    for path in &missing {
        println!("[xink] 历史标签已失效，跳过恢复: {}", path.display());
    }
    let active = active_within(notes.active, &paths);

    OpenNotesSnapshot { paths, active }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/state/workspace.rs"]
mod tests;
