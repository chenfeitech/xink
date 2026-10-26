//! 行级 diff（AI 建议 vs 原文）
//!
//! 自实现而非引入 `similar`：需要的只是行级增删改展示，约 80 行且可单测。
//! 先剥掉公共前后缀再对中间做 LCS——编辑类改动通常只集中在几行，
//! 这样 2000 行笔记也不会退化成 O(n·m) 的大表。

/// 一行 diff 结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Same(String),
    Add(String),
    Del(String),
}

#[allow(dead_code)]
impl DiffLine {
    pub fn text(&self) -> &str {
        match self {
            Self::Same(t) | Self::Add(t) | Self::Del(t) => t,
        }
    }
}

/// LCS 的规模上限：超过就退化为"整段替换"，避免大笔记上卡住
const MAX_LCS_LINES: usize = 1500;

/// 计算 before → after 的行级差异
pub fn diff_lines(before: &str, after: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();

    let mut out = Vec::with_capacity(b.len().max(a.len()));
    let prefix = common_prefix_len(&a, &b);
    for line in &a[..prefix] {
        out.push(DiffLine::Same((*line).to_string()));
    }

    let a_mid = &a[prefix..a.len() - common_suffix_len(&a[prefix..], &b[prefix..])];
    let b_mid = &b[prefix..b.len() - common_suffix_len(&a[prefix..], &b[prefix..])];

    if a_mid.len() <= MAX_LCS_LINES && b_mid.len() <= MAX_LCS_LINES {
        for line in lcs_diff(a_mid, b_mid) {
            out.push(line);
        }
    } else {
        for line in a_mid {
            out.push(DiffLine::Del((*line).to_string()));
        }
        for line in b_mid {
            out.push(DiffLine::Add((*line).to_string()));
        }
    }

    let suffix_start = a.len() - common_suffix_len(&a[prefix..], &b[prefix..]);
    for line in &a[suffix_start..] {
        out.push(DiffLine::Same((*line).to_string()));
    }
    out
}

/// 统计（新增行数，删除行数）
pub fn diff_stats(lines: &[DiffLine]) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for line in lines {
        match line {
            DiffLine::Add(_) => added += 1,
            DiffLine::Del(_) => removed += 1,
            DiffLine::Same(_) => {}
        }
    }
    (added, removed)
}

fn common_prefix_len(a: &[&str], b: &[&str]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn common_suffix_len(a: &[&str], b: &[&str]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

/// 标准 LCS 回溯：先算长度表，再倒着走一遍
fn lcs_diff(a: &[&str], b: &[&str]) -> Vec<DiffLine> {
    let (n, m) = (a.len(), b.len());
    let mut table = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if a[i] == b[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(DiffLine::Same(a[i].to_string()));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            out.push(DiffLine::Del(a[i].to_string()));
            i += 1;
        } else {
            out.push(DiffLine::Add(b[j].to_string()));
            j += 1;
        }
    }
    while i < n {
        out.push(DiffLine::Del(a[i].to_string()));
        i += 1;
    }
    while j < m {
        out.push(DiffLine::Add(b[j].to_string()));
        j += 1;
    }
    out
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/diff.rs"]
mod tests;
