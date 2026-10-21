use super::*;
use std::time::{Duration, Instant};

// ── 块划分 ────────────────────────────────────────────────────

#[test]
fn every_top_level_block_gets_a_span() {
    // 标题 / 段落 / 围栏代码块 / 列表 / 分隔线 各自是一个顶层块
    let source = "# 标题\n\n第一段\n\n```rust\nfn main() {}\n```\n\n- a\n- b\n\n---\n\n结尾\n";
    let spans = block_spans(source);
    assert_eq!(spans.len(), 6, "块划分: {spans:?}");
    assert_eq!(spans[0].start_line, 0, "标题在第 0 行");
    assert_eq!(spans[1].start_line, 2, "第一段在第 2 行");
    assert_eq!(spans[2].start_line, 4, "代码块从第 4 行开始");
    assert_eq!(spans[2].end_line, 6, "代码块到第 6 行结束（含收尾围栏）");
    assert_eq!(
        (spans[3].start_line, spans[3].end_line),
        // 注意 mdast 的 position 会把块尾的空行也算进去（这里是第 10 行）
        (8, 10),
        "列表两项算一个块"
    );
    assert_eq!(spans[4].start_line, 11, "分隔线单独一个块");
    assert_eq!(spans[5].start_line, 13, "结尾段落在第 13 行");
}

#[test]
fn fenced_code_inside_a_fence_is_not_a_block() {
    // 围栏里的 `#` 不是标题，不能算新块
    let source = "```\n# 不是标题\n- 也不是列表\n```\n";
    let spans = block_spans(source);
    assert_eq!(spans.len(), 1, "整个围栏是一个块: {spans:?}");
    assert_eq!(spans[0].end_line, 3);
}

#[test]
fn nested_list_is_one_block() {
    // 嵌套列表属于同一个 list 节点，不能按项拆
    let source = "- a\n  - b\n    - c\n";
    assert_eq!(block_spans(source).len(), 1);
}

#[test]
fn empty_source_has_no_blocks() {
    assert!(block_spans("").is_empty());
    assert!(block_spans("\n\n\n").is_empty());
}

#[test]
fn blockquote_counts_as_one_block() {
    let spans = block_spans("> 引用第一行\n> 引用第二行\n");
    assert_eq!(spans.len(), 1, "整段引用是一个块: {spans:?}");
    assert_eq!(spans[0].end_line, 1);
}

// ── 锚点换算 ──────────────────────────────────────────────────

fn table_of(source: &str) -> AnchorTable {
    let lines = source.lines().count().max(1);
    AnchorTable::from_source(source, lines)
}

#[test]
fn locate_maps_line_to_block_index() {
    let table = table_of("# 标题\n\n段落一\n\n段落二\n");
    // 块：0=标题(0..0) 1=段落一(2..2) 2=段落二(4..4)
    assert_eq!(table.len(), 3);
    assert_eq!(table.locate(0.0), (0, 0.0));
    assert_eq!(table.locate(2.0), (1, 0.0));
    assert_eq!(table.locate(4.0), (2, 0.0));
}

#[test]
fn locate_interpolates_inside_a_block() {
    // 一个跨多行的代码块：块内按比例插值
    let table = table_of("```\na\nb\nc\nd\n```\n");
    assert_eq!(table.len(), 1);
    let (ix, frac) = table.locate(3.0); // 第 3 行 = 块的第 3/6
    assert_eq!(ix, 0);
    assert!((frac - 0.5).abs() < 0.01, "块内比例应约为 0.5: {frac}");
}

#[test]
fn line_of_is_the_inverse_of_locate() {
    let table = table_of("# 标题\n\n段落一\n\n```py\nx = 1\n```\n\n结尾\n");
    for line in [0.0, 1.0, 2.5, 4.0, 5.0, 8.0] {
        let (ix, frac) = table.locate(line);
        let back = table.line_of(ix, frac);
        assert!(
            (back - line).abs() <= 1.0,
            "往返误差应在一行内: {line} -> {back}"
        );
    }
}

#[test]
fn out_of_range_lines_are_clamped() {
    let table = table_of("# 标题\n\n段落\n");
    // 超过文档末尾：落到最后一块，不越界
    let (ix, frac) = table.locate(999.0);
    assert_eq!(ix, table.len() - 1);
    assert!(frac <= 1.0);
    // 负数行号：夹到 0
    assert_eq!(table.locate(-5.0), (0, 0.0));
}

#[test]
fn empty_table_is_safe() {
    let table = AnchorTable::default();
    assert!(table.is_empty());
    assert_eq!(table.locate(3.0), (0, 0.0));
    assert_eq!(table.line_of(0, 0.5), 1.0, "空表不应返回垃圾行号");
}

// ── 锚点种类 ──────────────────────────────────────────────────

#[test]
fn line_and_ratio_are_not_interchangeable() {
    assert!(ScrollAnchor::Line(3.0).same_kind(ScrollAnchor::Line(9.0)));
    assert!(!ScrollAnchor::Line(0.5).same_kind(ScrollAnchor::Ratio(0.5)));
    assert_eq!(
        ScrollAnchor::Ratio(1.5).clamped(10.0),
        ScrollAnchor::Ratio(1.0)
    );
    assert_eq!(
        ScrollAnchor::Line(-1.0).clamped(10.0),
        ScrollAnchor::Line(0.0)
    );
    assert_eq!(
        ScrollAnchor::Line(99.0).clamped(10.0),
        ScrollAnchor::Line(10.0)
    );
}

// ── 方向锁 ────────────────────────────────────────────────────

#[test]
fn first_scroll_claims_the_lock() {
    let mut guard = SyncGuard::default();
    let now = Instant::now();
    assert!(guard.claim(Side::Editor, now), "首个滚动应抢到锁");
    // 抢到锁的是编辑区：预览区此刻"滚动"应被判为回显
    assert!(!guard.claim(Side::Preview, now));
}

#[test]
fn the_other_side_is_silenced_while_the_lock_is_held() {
    let mut guard = SyncGuard::default();
    let t0 = Instant::now();
    assert!(guard.claim(Side::Editor, t0));
    // 对端在同一瞬间"滚动"：这是回显，不该抢锁
    assert!(!guard.claim(Side::Preview, t0 + Duration::from_millis(16)));
    // 发起端继续滚：仍然有效
    assert!(guard.claim(Side::Editor, t0 + Duration::from_millis(32)));
}

#[test]
fn the_lock_expires_and_the_other_side_can_take_over() {
    let mut guard = SyncGuard::default();
    let t0 = Instant::now();
    assert!(guard.claim(Side::Editor, t0));
    // 200ms 后对端接管：模拟用户转而滚动预览
    let t1 = t0 + Duration::from_millis(250);
    assert!(guard.claim(Side::Preview, t1));
    // 锁已易主：编辑区此刻不能再推
    assert!(!guard.claim(Side::Editor, t1));
}

#[test]
fn echo_is_detected_with_the_given_tolerance() {
    let mut guard = SyncGuard::default();
    guard.expect(ScrollAnchor::Line(12.0));
    assert!(guard.is_echo(ScrollAnchor::Line(12.04), 0.1));
    assert!(!guard.is_echo(ScrollAnchor::Line(12.9), 0.1));
    // 种类不同不算回显
    assert!(!guard.is_echo(ScrollAnchor::Ratio(12.0), 0.1));
}

#[test]
fn without_an_expectation_nothing_is_an_echo() {
    let guard = SyncGuard::default();
    assert!(!guard.is_echo(ScrollAnchor::Line(0.0), 1.0));
}

#[test]
fn reset_clears_owner_and_expectation() {
    let mut guard = SyncGuard::default();
    let now = Instant::now();
    guard.claim(Side::Editor, now);
    guard.expect(ScrollAnchor::Line(5.0));
    guard.reset();
    // 重置后任何人都能重新抢锁，且旧的回显基准已失效
    assert!(guard.claim(Side::Preview, now));
    assert!(!guard.is_echo(ScrollAnchor::Line(5.0), 0.0));
}

// ── 用户手势静默窗口 ──────────────────────────────────────────

#[test]
fn gesture_opens_a_quiet_window() {
    let mut guard = GestureGuard::default();
    let t0 = Instant::now();
    assert!(!guard.is_quiet(t0), "没滚过就不该静默");
    guard.note_user_movement(t0);
    assert!(guard.is_quiet(t0 + Duration::from_millis(16)));
}

#[test]
fn quiet_window_expires() {
    let mut guard = GestureGuard::default();
    let t0 = Instant::now();
    guard.note_user_movement(t0);
    assert!(
        !guard.is_quiet(t0 + GestureGuard::QUIET_PERIOD),
        "静默窗口到期后应恢复同步写入"
    );
}

#[test]
fn continuous_gestures_keep_renewing_the_window() {
    // 触控板惯性：每 16ms 一个事件，窗口应一直续期
    let mut guard = GestureGuard::default();
    let mut now = Instant::now();
    for _ in 0..30 {
        guard.note_user_movement(now);
        now += Duration::from_millis(16);
    }
    assert!(guard.is_quiet(now), "连续手势期间应一直静默");
    assert!(!guard.is_quiet(now + GestureGuard::QUIET_PERIOD));
}

#[test]
fn reset_ends_the_quiet_window() {
    let mut guard = GestureGuard::default();
    let now = Instant::now();
    guard.note_user_movement(now);
    guard.reset();
    assert!(!guard.is_quiet(now));
}
