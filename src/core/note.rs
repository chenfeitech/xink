//! Note（笔记）：一篇 Markdown 文件的内存表示
//!
//! 磁盘上的文件是唯一真值。本模块负责加载、编辑标记与原子保存。

use anyhow::{Context as _, Result};
use ropey::{LineType, Rope};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Note {
    path: PathBuf,
    rope: Rope,
    /// 有未保存的修改
    dirty: bool,
    /// 加载/保存时记录的磁盘 mtime，用于检测外部修改
    disk_mtime: Option<SystemTime>,
    /// 内容版本号，每次内容变化递增。
    /// UI 用它判断是否需要重新生成派生数据，避免每帧做 Rope → String 全量拷贝。
    version: u64,
}

impl Note {
    pub fn load(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("读取笔记失败: {}", path.display()))?;
        let mtime = mtime_of(&path).ok();

        Ok(Self {
            path,
            rope: Rope::from_str(&content),
            dirty: false,
            disk_mtime: mtime,
            version: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 内容版本号。内容没变则不变，UI 可据此跳过重算。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 行数。基于 Rope 直接统计，不产生字符串拷贝（编辑时滚动同步需要高频调用）。
    pub fn line_count(&self) -> usize {
        self.rope.len_lines(LineType::LF).max(1)
    }

    /// 编辑器内容变更时调用。内容未变则不置脏，避免无意义的写盘。
    pub fn set_text(&mut self, text: &str) {
        if self.text() == text {
            return;
        }
        self.rope = Rope::from_str(text);
        self.dirty = true;
        self.version += 1;
    }

    /// 原子保存：先写同目录临时文件，再 rename 替换。
    /// 这样崩溃时不会留下半截笔记。
    pub fn save(&mut self) -> Result<()> {
        atomic_write(&self.path, &self.text())?;
        self.dirty = false;
        // 更新 mtime，避免把自己的写入误判成外部修改
        self.disk_mtime = mtime_of(&self.path).ok();
        Ok(())
    }

    /// 磁盘上的文件是否被外部改动过（M5 由 watcher 使用）
    #[allow(dead_code)]
    pub fn disk_changed(&self) -> bool {
        match (self.disk_mtime, mtime_of(&self.path).ok()) {
            (Some(recorded), Some(current)) => current != recorded,
            _ => false,
        }
    }

    /// 放弃内存修改，重新从磁盘加载（M5 外部变更提示时使用）
    #[allow(dead_code)]
    pub fn reload(&mut self) -> Result<()> {
        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("重新加载失败: {}", self.path.display()))?;
        self.rope = Rope::from_str(&content);
        self.dirty = false;
        self.disk_mtime = mtime_of(&self.path).ok();
        self.version += 1;
        Ok(())
    }
}

/// 原子写盘：同目录临时文件 + rename（同一文件系统内 rename 是原子的）
pub fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let mut tmp_os = path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);

    fs::write(&tmp, content).with_context(|| format!("写临时文件失败: {}", tmp.display()))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            // 清理临时文件，避免残留
            let _ = fs::remove_file(&tmp);
            Err(err).with_context(|| format!("替换文件失败: {}", path.display()))
        }
    }
}

fn mtime_of(path: &Path) -> Result<SystemTime> {
    Ok(fs::metadata(path)?.modified()?)
}

/// 路径是否指向 Markdown 文件
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/note.rs"]
mod tests;
