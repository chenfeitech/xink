//! 文件系统操作服务：导入、移动、重名处理。
//!
//! 与 UI 解耦（不引用任何 `gpui_kit::component::*`），可独立单元测试。
//! 供目录树的三项需求共用：外部拖入（需求 2）、粘贴（需求 3）、拖拽移动（需求 1a）。

use std::fs;
use std::path::{Path, PathBuf};

/// 导入方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    /// 复制：源文件保留
    Copy,
    /// 移动：源文件不再保留
    Move,
}

/// 重名时递增后缀的上限，避免异常情况下死循环
const MAX_SUFFIX: u32 = 999;

/// 在目标目录里为 `name` 生成一个不冲突的路径。
///
/// 规则（详见 `docs/file-tree-interactions-design.md` §8）：
/// - 不冲突则原样返回；
/// - 冲突时在主文件名后加 `-N`（N 从 1 起），**扩展名保持不变**；
/// - 主文件名已带尾部序号时先剥离再递增，避免出现 `a-1-1.png`；
/// - 目录同名适用同一规则。
pub fn unique_name(dir: &Path, name: &str) -> PathBuf {
    let direct = dir.join(name);
    if !direct.exists() {
        return direct;
    }

    let (stem, ext) = split_name(name);
    let base = strip_trailing_index(stem);
    let suffix = match ext {
        Some(ext) => format!(".{ext}"),
        None => String::new(),
    };

    for n in 1..=MAX_SUFFIX {
        let candidate = dir.join(format!("{base}-{n}{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // 极端情况：退回原路径，交给调用方按失败处理
    direct
}

/// 计算重命名后的文件名：**只有 Markdown 才会自动补 `.md`**。
///
/// 规则（`from` 是被改名的路径，`new_name` 是用户输入）：
///
/// - 用户显式写了扩展名 → 一律尊重（改名换类型应是有意为之）；
/// - 没写扩展名时补回**源文件自己的扩展名**：`.md` 源文件补 `.md`，
///   `.json` 源文件补 `.json`；
/// - 源文件本身没有扩展名 → 不加后缀（不凭空创造类型）。
///
/// 关键在于**绝不拼 `.md`**：否则把 `conf.json` 改名为 `conf` 会变成
/// `conf.json.md`，再遇到重名时又会被当作"不冲突"而静默改名，
/// 文件类型与内容从此对不上。目录调用此函数也安全。
pub fn rename_name(from: &Path, new_name: &str) -> String {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if split_name(trimmed).1.is_some() {
        return trimmed.to_string();
    }
    match from.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => format!("{trimmed}.{ext}"),
        None => trimmed.to_string(),
    }
}

/// 拆分为 (主文件名, 扩展名)；无扩展名或点号在开头时扩展名为 `None`
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    }
}

/// 去掉主文件名末尾的 `-数字`，使再次导入时递增而不是叠加
fn strip_trailing_index(stem: &str) -> &str {
    match stem.rsplit_once('-') {
        Some((head, tail)) if !head.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => stem,
    }
}

/// 复制文件或目录（目录递归）。
pub fn copy_path(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| format!("创建目录失败: {e}"))?;
        let entries = fs::read_dir(from).map_err(|e| format!("读取目录失败: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {e}"))?;
            copy_path(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
        fs::copy(from, to).map_err(|e| format!("复制失败: {e}"))?;
        Ok(())
    }
}

/// 移动：同设备直接 `rename`；跨设备回退为「复制 + 校验 + 删除源」。
///
/// 跨设备复制后若大小不一致，**保留源文件**并返回错误，绝不丢数据。
pub fn move_path(from: &Path, to: &Path) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    if to.exists() {
        return Err(format!("目标已存在: {}", to.display()));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    if fs::rename(from, to).is_ok() {
        return Ok(());
    }

    // rename 失败通常是跨设备：改为复制后删除
    copy_path(from, to)?;
    if !from.is_dir() {
        let src_len = fs::metadata(from)
            .map_err(|e| format!("读取源文件失败: {e}"))?
            .len();
        let dst_len = fs::metadata(to)
            .map_err(|e| format!("读取目标文件失败: {e}"))?
            .len();
        if src_len != dst_len {
            let _ = fs::remove_file(to);
            return Err("跨设备移动失败：复制结果不完整，已保留源文件".into());
        }
    }
    remove_path(from)?;
    Ok(())
}

/// 删除文件或目录（目录递归）。
pub fn remove_path(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| format!("删除目录失败: {e}"))
    } else {
        fs::remove_file(path).map_err(|e| format!("删除文件失败: {e}"))
    }
}

/// 把一批路径导入目标目录，逐个返回目标路径或失败原因。
///
/// 单个失败不影响其余文件：结果按输入顺序一一对应。
pub fn import_paths(
    paths: &[PathBuf],
    target_dir: &Path,
    mode: ImportMode,
) -> Vec<Result<PathBuf, String>> {
    paths
        .iter()
        .map(|src| {
            let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
                return Err(format!("无法取得文件名: {}", src.display()));
            };
            let dst = unique_name(target_dir, name);
            match mode {
                ImportMode::Copy => copy_path(src, &dst).map(|_| dst),
                ImportMode::Move => move_path(src, &dst).map(|_| dst),
            }
        })
        .collect()
}

/// 把一段字节写成目标目录里的新文件。
///
/// 用于**没有源文件**的场景（如剪贴板里的截图）：调用方给出主文件名与扩展名，
/// 冲突时按 `unique_name` 递增。
pub fn write_new_file(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let name = if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    };
    let path = unique_name(dir, &name);
    fs::write(&path, bytes).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/fs.rs"]
mod tests;
