//! hunk 分组 / 定位 / 局部应用的测试

use super::{Located, MAX_HUNKS, PatchError, apply_hunk, hunks, locate_range};
use crate::core::ai::diff::DiffLine;

const CTX: usize = 2;

fn note() -> String {
    (1..=12)
        .map(|ix| format!("第 {ix} 行"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// 改第 3 行
fn changed(ix: usize, to: &str) -> String {
    note()
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if i == ix {
                to.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn hunks_groups_adjacent_changes() {
    // 改第 3、4 行（相邻，应合成一块）
    let after = note()
        .lines()
        .enumerate()
        .map(|(ix, line)| match ix {
            2 => "改过的第 3 行".to_string(),
            3 => "改过的第 4 行".to_string(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hunks = hunks(&note(), &after, CTX);
    assert_eq!(hunks.len(), 1, "相邻改动应合成一块：{hunks:?}");
    assert_eq!(hunks[0].start_line, 2);
    assert_eq!(hunks[0].added, 2);
    assert_eq!(hunks[0].removed, 2);
}

#[test]
fn hunks_splits_distant_changes() {
    // 改第 2 行与第 10 行（隔得远，应是两块）
    let after = note()
        .lines()
        .enumerate()
        .map(|(ix, line)| match ix {
            1 => "改过的第 2 行".to_string(),
            9 => "改过的第 10 行".to_string(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hunks = hunks(&note(), &after, CTX);
    assert_eq!(hunks.len(), 2, "隔得远的改动应分成两块：{hunks:?}");
    assert_eq!(hunks[0].start_line, 1);
    assert_eq!(hunks[1].start_line, 9);
}

#[test]
fn hunks_include_context_lines() {
    let hunks = hunks(&note(), &changed(5, "改过的第 6 行"), CTX);
    assert_eq!(hunks.len(), 1);
    let lines = &hunks[0].lines;
    // 上下文 2 + 改动 2（一删一增）+ 上下文 2
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert!(matches!(lines[0], DiffLine::Same(_)));
    assert!(matches!(lines[1], DiffLine::Same(_)));
    assert!(matches!(lines[2], DiffLine::Del(_)));
    assert!(matches!(lines[3], DiffLine::Add(_)));
    assert!(matches!(lines[4], DiffLine::Same(_)));
    assert!(matches!(lines[5], DiffLine::Same(_)));
}

#[test]
fn hunk_carries_original_and_new_lines() {
    let hunks = hunks(&note(), &changed(5, "改过的第 6 行"), CTX);
    let hunk = &hunks[0];
    assert_eq!(hunk.orig_lines, vec!["第 6 行".to_string()]);
    assert_eq!(hunk.new_lines, vec!["改过的第 6 行".to_string()]);
}

#[test]
fn locate_range_finds_unique_block() {
    let hunks = hunks(&note(), &changed(5, "改过的第 6 行"), CTX);
    match locate_range(&note(), &hunks[0]) {
        Located::Unique(range) => assert_eq!(&note()[range], "第 6 行"),
        other => panic!("应唯一命中，实际 {other:?}"),
    }
}

#[test]
fn locate_range_reports_ambiguous() {
    // 原文里"重复行"出现两次
    let text = "开头\n重复行\n中间\n重复行\n结尾\n";
    let hunk = super::Hunk {
        start_line: 1,
        orig_lines: vec!["重复行".into()],
        new_lines: vec!["改过的重复行".into()],
        lines: Vec::new(),
        added: 1,
        removed: 1,
    };
    assert_eq!(locate_range(text, &hunk), Located::Ambiguous(2));
}

#[test]
fn locate_range_reports_not_found() {
    let hunks = hunks(&note(), &changed(5, "改过的第 6 行"), CTX);
    let edited = note().replace("第 6 行", "我手改过的第 6 行");
    assert_eq!(locate_range(&edited, &hunks[0]), Located::NotFound);
}

#[test]
fn pure_insert_hunk_cannot_be_located() {
    let hunk = super::Hunk {
        start_line: 0,
        orig_lines: Vec::new(),
        new_lines: vec!["新增一行".into()],
        lines: Vec::new(),
        added: 1,
        removed: 0,
    };
    assert_eq!(locate_range(&note(), &hunk), Located::NotFound);
}

#[test]
fn apply_hunk_only_touches_that_block() {
    let after = note()
        .lines()
        .enumerate()
        .map(|(ix, line)| match ix {
            2 => "改过的第 3 行".to_string(),
            9 => "改过的第 10 行".to_string(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hunks = hunks(&note(), &after, CTX);
    assert_eq!(hunks.len(), 2);

    let out = apply_hunk(&note(), &hunks[1]).expect("应能应用");
    assert!(out.contains("改过的第 10 行"), "接受的那块应生效：{out}");
    assert!(
        out.contains("第 3 行") && !out.contains("改过的第 3 行"),
        "没接受的那块必须原样保留：{out}"
    );
}

#[test]
fn apply_hunks_in_any_order_is_stable() {
    let after = note()
        .lines()
        .enumerate()
        .map(|(ix, line)| match ix {
            2 => "改过的第 3 行".to_string(),
            9 => "改过的第 10 行".to_string(),
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hunks = hunks(&note(), &after, CTX);

    // 从后往前接受
    let mut back_to_front = note();
    for hunk in hunks.iter().rev() {
        back_to_front = apply_hunk(&back_to_front, hunk).expect("应能应用");
    }
    // 从前往后接受
    let mut front_to_back = note();
    for hunk in &hunks {
        front_to_back = apply_hunk(&front_to_back, hunk).expect("应能应用");
    }
    assert_eq!(back_to_front, front_to_back, "接受顺序不该影响结果");
    assert_eq!(back_to_front, after, "全部接受后应等于改后文本");
}

#[test]
fn applying_a_deletion_leaves_no_blank_line() {
    let after = note()
        .lines()
        .enumerate()
        .filter(|(ix, _)| *ix != 4)
        .map(|(_, line)| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hunks = hunks(&note(), &after, CTX);
    let out = apply_hunk(&note(), &hunks[0]).expect("应能应用");
    assert!(!out.contains("\n\n"), "删行不该留下空行：{out:?}");
}

#[test]
fn stale_hunk_reports_patch_error() {
    let hunks = hunks(&note(), &changed(5, "改过的第 6 行"), CTX);
    let edited = note().replace("第 6 行", "手改过");
    match apply_hunk(&edited, &hunks[0]) {
        Err(PatchError::NotFound) => {}
        other => panic!("应报 NotFound，实际 {other:?}"),
    }
}

#[test]
fn hunks_are_capped() {
    let before: String = (0..MAX_HUNKS + 10)
        .map(|ix| format!("行 {ix}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    // 隔一行改一行 → 改动块数远超上限
    let after: String = before
        .lines()
        .enumerate()
        .map(|(ix, line)| {
            if ix % 2 == 0 {
                format!("改过的 {line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hunks = hunks(&before, &after, 0);
    assert!(
        hunks.len() <= MAX_HUNKS,
        "应截断到 {MAX_HUNKS}：{}",
        hunks.len()
    );
}
