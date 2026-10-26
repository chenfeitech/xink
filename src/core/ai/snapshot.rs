//! 改前快照：AI 写入前的"后悔药"
//!
//! 项目没有撤销栈，保存即落盘，因此快照是唯一低成本的回退手段
//! （见 `docs/ai/ai-assistant-design.md` §7 A7.2）。

use std::fs;
use std::path::{Path, PathBuf};

use super::AiError;

/// 快照目录名（应用数据目录下）
pub const SNAPSHOT_DIR: &str = "ai-history";

/// `<yyyy-mm-dd-HHMMSS>-<文件名>`；同名时用微秒后缀避免覆盖
pub fn snapshot_name(file_name: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 不引 chrono：手工换算 UTC 日期时间即可，精度到秒足够
    let secs = now as i64;
    let (y, m, d, hh, mm, ss) = civil_from_days_hms(secs);
    format!("{y:04}-{m:02}-{d:02}-{hh:02}{mm:02}{ss:02}-{file_name}")
}

/// 写一份快照，返回路径
pub fn write_snapshot(dir: &Path, file_name: &str, content: &str) -> Result<PathBuf, AiError> {
    fs::create_dir_all(dir).map_err(|e| AiError::Io(e.to_string()))?;
    let path = dir.join(snapshot_name(file_name));
    let path = if path.exists() {
        // 同一秒内再次快照：加一个序号
        let mut i = 1;
        loop {
            let candidate = dir.join(format!("{}-{}", snapshot_name(file_name), i));
            if !candidate.exists() {
                break candidate;
            }
            i += 1;
        }
    } else {
        path
    };
    fs::write(&path, content).map_err(|e| AiError::Io(e.to_string()))?;
    Ok(path)
}

/// 秒 → 年月日时分秒（UTC）
fn civil_from_days_hms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hh = (rem / 3600) as u32;
    let mm = ((rem % 3600) / 60) as u32;
    let ss = (rem % 60) as u32;

    // Howard Hinnant 的 civil_from_days 算法（公历，无依赖）
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, hh, mm, ss)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/snapshot.rs"]
mod tests;
