//! 模糊匹配：命令面板、全文搜索、标签筛选共用的打分与命中位置
//!
//! 采用 fzf 式的"子序列 + 位置加权"：查询串的字符按顺序出现在候选串中即算命中，
//! 命中位置越靠前、越连续、越贴合词边界，得分越高。
//!
//! 只依赖标准库：候选规模是万级文件名，线性扫描足够，
//! 引入第三方模糊匹配库换来的收益抵不上多一个依赖。

/// 一次模糊匹配的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// 得分，越大越靠前；只在同一次查询内相对比较
    pub score: u32,
    /// 命中字符在候选串中的**字符下标**（不是字节下标），供 UI 高亮
    pub positions: Vec<usize>,
}

/// 模糊匹配；不匹配返回 `None`，空查询匹配一切。
///
/// 按**字符**匹配，中文与 emoji 都不会被截断。
// 首个使用者是 P1-T7 命令面板（随后 T11 标签筛选、T12 全文搜索也会用）。
// 目前只有测试在用，故显式豁免死代码告警——它不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub fn fuzzy_match(query: &str, text: &str) -> Option<FuzzyMatch> {
    let query: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: short_text_bonus(text),
            positions: Vec::new(),
        });
    }

    let chars: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    if query.len() > chars.len() {
        return None;
    }

    let mut score = 0u32;
    let mut positions = Vec::with_capacity(query.len());
    let mut q_ix = 0usize;
    let mut prev_ix: Option<usize> = None;

    for (ix, ch) in chars.iter().enumerate() {
        if q_ix >= query.len() {
            break;
        }
        if *ch != query[q_ix] {
            continue;
        }

        score = score.saturating_add(BASE);
        score = score.saturating_add(boundary_bonus(&chars, ix, prev_ix));
        if let Some(prev) = prev_ix {
            if ix == prev + 1 {
                score = score.saturating_add(CONSECUTIVE);
            } else {
                // 跳字越多越不像目标，但设上限，避免长路径被一路扣到沉底
                let gap = (ix - prev - 1).min(MAX_GAP) as u32;
                score = score.saturating_sub(gap * GAP_PENALTY);
            }
        }

        positions.push(ix);
        prev_ix = Some(ix);
        q_ix += 1;
    }

    if q_ix < query.len() {
        return None;
    }
    score = score.saturating_add(short_text_bonus(text));
    Some(FuzzyMatch { score, positions })
}

/// 只取分数：排序时用它，避免给每个候选都分配 `positions`
#[allow(dead_code)]
pub fn fuzzy_score(query: &str, text: &str) -> Option<u32> {
    fuzzy_match(query, text).map(|matched| matched.score)
}

// ── 打分常量 ──────────────────────────────────────────────────
//
// 数值只在同一次查询内相对比较，量级本身没有含义。

/// 命中一个字符的基础分
const BASE: u32 = 10;
/// 紧邻上一个命中（连续片段）
const CONSECUTIVE: u32 = 8;
/// 命中在词边界（路径分隔符、空格、连字符、驼峰边界等之后）
const BOUNDARY: u32 = 6;
/// 命中在候选串开头
const LEADING: u32 = 4;
/// 每跳过一个字符的惩罚（上限见 `MAX_GAP`）
const GAP_PENALTY: u32 = 1;
/// 跳字惩罚的上限
const MAX_GAP: usize = 8;
/// 短候选奖励的上限
const SHORT_BONUS: u32 = 12;

/// 位置加权：开头、分隔符之后、驼峰边界都更"像"用户想找的东西。
fn boundary_bonus(chars: &[char], ix: usize, prev_ix: Option<usize>) -> u32 {
    if ix == 0 {
        return LEADING + BOUNDARY;
    }
    let prev_char = chars[ix - 1];
    if is_separator(prev_char) || (prev_char.is_lowercase() && chars[ix].is_uppercase()) {
        return BOUNDARY;
    }
    // 首个命中之前的字符都是噪音：越靠前越优先
    if prev_ix.is_none() {
        return LEADING.saturating_sub((ix / 4) as u32);
    }
    0
}

/// 词分隔符：路径、扩展名、空格与常见括号
fn is_separator(c: char) -> bool {
    matches!(
        c,
        '/' | '\\' | '.' | '-' | '_' | ' ' | ':' | '（' | '(' | '['
    )
}

/// 短候选的基准奖励：每多 8 个字符减 1 分，下限 0。
fn short_text_bonus(text: &str) -> u32 {
    SHORT_BONUS.saturating_sub((text.chars().count() / 8) as u32)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/fuzzy.rs"]
mod tests;
