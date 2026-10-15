use super::*;

fn score(query: &str, text: &str) -> u32 {
    fuzzy_score(query, text).unwrap_or_else(|| panic!("{query:?} 应匹配 {text:?}"))
}

fn positions(query: &str, text: &str) -> Vec<usize> {
    fuzzy_match(query, text)
        .unwrap_or_else(|| panic!("{query:?} 应匹配 {text:?}"))
        .positions
}

// ── 基本命中 ──────────────────────────────────────────────────

#[test]
fn empty_query_matches_everything() {
    let matched = fuzzy_match("", "任意候选.md").expect("空查询应匹配一切");
    assert!(matched.positions.is_empty(), "空查询没有命中字符");
    assert!(fuzzy_match("", "").is_some(), "空候选也算匹配");
}

#[test]
fn subsequence_characters_are_matched_in_order() {
    // r@0 p@2 n@10 l@12 —— 贪心取最先出现的位置
    assert_eq!(positions("rpnl", "replace_panel.rs"), vec![0, 2, 10, 12]);
}

#[test]
fn unmatched_query_is_rejected() {
    assert_eq!(fuzzy_match("xyz", "abc"), None, "字符全部缺失");
    assert_eq!(fuzzy_match("abcd", "abc"), None, "查询比候选长");
    assert_eq!(fuzzy_match("aab", "ab"), None, "顺序不满足（重复字符）");
}

#[test]
fn matching_is_case_insensitive() {
    // README.MD：m 取第 4 个字符（不是末尾那个），d 取第 8 个
    assert_eq!(positions("md", "README.MD"), vec![4, 8]);
    assert_eq!(score("MD", "readme.md"), score("md", "readme.md"));
}

// ── CJK ──────────────────────────────────────────────────────

#[test]
fn chinese_is_matched_per_character() {
    // 下标是字符下标：中文按字节切片会错位，这里固化按字符计量
    assert_eq!(positions("笔记", "我的笔记.md"), vec![2, 3]);
    assert!(fuzzy_match("笔记", "我的笔记.md").is_some());
}

#[test]
fn chinese_never_panics_on_partial_input() {
    // 模拟输入法逐字上屏的中间态（含不成对的代理项不会出现在 Rust 的 &str 里，
    // 但逐字匹配必须健壮）
    for q in ["笔", "笔记", "笔记m", "笔记md"] {
        assert!(fuzzy_match(q, "我的笔记.md").is_some(), "{q:?}");
    }
    assert!(fuzzy_match("我的笔记.md", "笔记").is_none());
}

// ── 排序倾向 ──────────────────────────────────────────────────

#[test]
fn prefix_beats_middle_match() {
    assert!(
        score("note", "notes.md") > score("note", "my notes.md"),
        "开头命中应排在中间命中之前"
    );
}

#[test]
fn consecutive_characters_beat_scattered_ones() {
    assert!(
        score("ab", "ab_cd.rs") > score("ab", "a_long_b.rs"),
        "连续片段得分更高"
    );
}

#[test]
fn word_boundary_is_preferred() {
    // 同样的 `bc`，一个紧跟分隔符，一个嵌在单词中间
    assert!(
        score("bc", "a/bc.md") > score("bc", "abc.md"),
        "分隔符后的命中应优先"
    );
}

#[test]
fn shorter_candidates_win_on_equal_matches() {
    assert!(
        score("ab", "ab.md") > score("ab", "ab_with_a_very_long_name.md"),
        "命中质量相同时短名在前"
    );
}

#[test]
fn scores_are_stable_across_repeats() {
    let a = score("todo", "notes/todo.md");
    let b = score("todo", "notes/todo.md");
    assert_eq!(a, b, "同样输入必须得到同样分数，否则列表会抖动");
}

// ── 文件名的真实场景 ──────────────────────────────────────────

#[test]
fn ranks_a_typical_vault_the_expected_way() {
    let query = "p1";
    let mut ranked: Vec<(&str, u32)> = [
        "docs/P1-PLAN.md",
        "docs/design/p1-notes.md",
        "planning.md",
        "notes/apple-1.md",
    ]
    .into_iter()
    .filter_map(|name| fuzzy_score(query, name).map(|s| (name, s)))
    .collect();
    ranked.sort_by_key(|item| std::cmp::Reverse(item.1));

    let top = ranked.first().map(|(name, _)| *name).unwrap();
    assert_eq!(top, "docs/P1-PLAN.md", "实际排序：{ranked:?}");
    assert!(
        ranked.iter().any(|(name, _)| *name == "notes/apple-1.md"),
        "跳字过多的候选也要留在结果里，只是排后面：{ranked:?}"
    );
}
