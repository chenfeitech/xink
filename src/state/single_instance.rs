//! 单实例保护
//!
//! 每个实例都有独立的 GPU 渲染窗口与文件监听线程，
//! 重复启动会让 CPU 与内存成倍上涨，因此这里挡住第二个实例。

use std::fs;
use std::path::{Path, PathBuf};

use crate::state::workspace::app_data_dir;

/// 尝试取得"运行权"。已有实例在跑时返回 false，调用方应直接退出。
pub fn try_acquire() -> bool {
    let path = pid_file();

    // 记录过 pid 且该进程还活着 → 已有实例
    if let Ok(content) = fs::read_to_string(&path)
        && let Ok(pid) = content.trim().parse::<i32>()
        && process_alive(pid)
    {
        return false;
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, std::process::id().to_string()).is_ok()
}

fn pid_file() -> PathBuf {
    app_data_dir().join("xink.pid")
}

/// 用 signal 0 探测进程是否存在（不发送实际信号）
fn process_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill(pid, 0) 只做权限与存在性检查，不会真的发信号
    unsafe { libc::kill(pid, 0) == 0 }
}

/// 仅供测试：判断路径是否指向 pid 文件
#[allow(dead_code)]
fn is_pid_file(path: &Path) -> bool {
    path.file_name().map(|n| n == "xink.pid").unwrap_or(false)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/state/single_instance.rs"]
mod tests;
