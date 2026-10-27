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

// ── 展示层：评审态（原文 + 就地插入的 AI 新行） ────────────────

use super::{ReviewKind, locate_lines, review_layout};

/// 模拟"逐块处置"：把每块的评审态行换成接受 / 拒绝后留下的行
fn resolve_all(before: &str, after: &str, accepted: bool) -> String {
    let layout = review_layout(before, after);
    let mut lines: Vec<String> = layout.lines.iter().map(|l| l.text.clone()).collect();
    for block in layout.blocks.iter().rev() {
        let text = lines.join("\n");
        let start = locate_lines(&text, &block.review_lines).expect("块应能定位");
        let replacement = if accepted {
            &block.new_lines
        } else {
            &block.orig_lines
        };
        lines.splice(
            start..start + block.review_lines.len(),
            replacement.iter().cloned(),
        );
    }
    let mut out = lines.join("\n");
    if before.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[test]
fn review_layout_inserts_added_lines_in_place() {
    let layout = review_layout(&note(), &changed(5, "改过的第 6 行"));
    // 原文行在上、AI 新行在下 —— 编辑区里的上下对照
    assert!(
        layout.text.contains("第 6 行\n改过的第 6 行"),
        "新行应就地插在原文行下方：{:?}",
        layout.text
    );
}

#[test]
fn review_layout_pairs_adjacent_del_and_add() {
    let layout = review_layout(&note(), &changed(5, "改过的第 6 行"));
    let rows = &layout.blocks[0].rows;
    let del = rows.iter().find(|r| r.kind == ReviewKind::Del).unwrap();
    let add = rows.iter().find(|r| r.kind == ReviewKind::Add).unwrap();
    assert!(del.paired && add.paired, "一删一增应算“改”：{rows:?}");
    assert_eq!(del.orig_no, Some(6));
    assert_eq!(del.new_no, None);
    assert_eq!(add.orig_no, None, "新增行没有原文行号");
    assert_eq!(add.new_no, Some(6));
}

#[test]
fn review_layout_leaves_pure_insert_unpaired() {
    let after = note().replace("第 3 行\n", "第 3 行\n插进来的一行\n");
    let layout = review_layout(&note(), &after);
    let add = layout.blocks[0]
        .rows
        .iter()
        .find(|r| r.kind == ReviewKind::Add)
        .expect("应有一行新增");
    assert!(!add.paired, "孤立的增不该标成“改”");
}

#[test]
fn review_layout_leaves_pure_delete_unpaired() {
    let after = note().replace("第 3 行\n", "");
    let layout = review_layout(&note(), &after);
    let del = layout.blocks[0]
        .rows
        .iter()
        .find(|r| r.kind == ReviewKind::Del)
        .expect("应有一行删除");
    assert!(!del.paired, "孤立的删不该标成“改”");
}

#[test]
fn accepting_every_block_yields_the_ai_version() {
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
    assert_eq!(resolve_all(&note(), &after, true), after);
}

#[test]
fn rejecting_every_block_yields_the_original() {
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
    assert_eq!(resolve_all(&note(), &after, false), note());
}

#[test]
fn review_layout_splits_distant_changes() {
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
    let layout = review_layout(&note(), &after);
    assert_eq!(
        layout.blocks.len(),
        2,
        "隔得远应是两块：{:?}",
        layout.blocks
    );
    assert_eq!(layout.blocks[0].start_line, 2);
    assert_eq!(layout.blocks[1].start_line, 10);
}

#[test]
fn locate_lines_finds_unique_start() {
    let layout = review_layout(&note(), &changed(5, "改过的第 6 行"));
    let start = locate_lines(&layout.text, &layout.blocks[0].review_lines);
    assert_eq!(start, Some(3), "第 6 行在评审态里是第 4 行（0-based 3）");
}

#[test]
fn locate_lines_rejects_ambiguous_and_missing() {
    let lines = vec!["重复".to_string()];
    assert_eq!(locate_lines("重复\n重复\n", &lines), None, "两处应为 None");
    assert_eq!(locate_lines("别的\n", &lines), None);
    assert_eq!(locate_lines("重复\n", &[]), None, "空锚点直接 None");
}
