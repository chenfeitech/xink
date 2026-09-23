//! 工作区与布局持久化
//!
//! 布局与库路径都是可重建的派生数据；
//! 笔记正文的唯一真值始终是磁盘上的 .md 文件，不存这里。

use anyhow::Result;
use gpui_kit::component::dock::DockAreaState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn save_vault(root: &Path) -> Result<()> {
    write_json(
        &vault_path(),
        &LastVault {
            root: root.to_path_buf(),
        },
    )
}

/// 库已失效（被移动/删除）时返回 None，由调用方回落
pub fn load_vault() -> Option<PathBuf> {
    let last: LastVault = read_json(&vault_path()).ok()?;
    if last.root.is_dir() {
        Some(last.root)
    } else {
        None
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, json)?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
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
    let paths = dedup_paths(paths);
    let active = active.filter(|p| paths.iter().any(|open| open == *p));
    write_json(
        &open_notes_path(),
        &OpenNotes {
            paths,
            active: active.cloned(),
        },
    )
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
    let active = notes.active.filter(|p| paths.contains(p));

    OpenNotesSnapshot { paths, active }
}

// ---- 最近打开的笔记 ----

const MAX_RECENT: usize = 10;

#[derive(Serialize, Deserialize, Default)]
struct RecentNotes {
    paths: Vec<PathBuf>,
}

/// 记录一条最近打开，最新的在前，去重且截断
pub fn push_recent_note(path: &Path) -> Result<()> {
    let file = app_data_dir().join("recent.json");
    let mut notes: RecentNotes = read_json(&file).unwrap_or_default();
    notes.paths.retain(|p| p != path);
    notes.paths.insert(0, path.to_path_buf());
    notes.paths.truncate(MAX_RECENT);
    write_json(&file, &notes)
}

pub fn load_recent_notes() -> Vec<PathBuf> {
    read_json::<RecentNotes>(&app_data_dir().join("recent.json"))
        .map(|n| n.paths.into_iter().filter(|p| p.is_file()).collect())
        .unwrap_or_default()
}
