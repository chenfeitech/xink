//! 库级长任务（M3-P1）：标签改写规则、计划生成、任务落盘与续跑

use std::path::{Path, PathBuf};

use super::{
    ItemState, TagOutcome, Task, TaskItem, TaskKind, TaskStatus, add_tag_text, apply_text, list,
    load, new_id, plan, remove_tag_text, retag_text, save,
};
use crate::core::ai::tasks::progress_md;

fn tmp(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("xink-session-{tag}"))
}

fn dir(tag: &str) -> PathBuf {
    let path = tmp(tag);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn changed(outcome: TagOutcome) -> String {
    match outcome {
        TagOutcome::Changed(text) => text,
        other => panic!("期望 Changed，实际 {other:?}"),
    }
}

#[test]
fn retag_replaces_the_whole_token_only() {
    let text = "#AI 和 #AI助手 都提到 #AI\n";
    let out = changed(retag_text(text, "AI", "人工智能"));
    assert_eq!(out, "#人工智能 和 #AI助手 都提到 #人工智能\n");
}

#[test]
fn retag_leaves_headings_alone() {
    let text = "# 标题\n\n## 小节\n";
    assert!(matches!(
        retag_text(text, "标题", "别的"),
        TagOutcome::Unchanged
    ));
}

#[test]
fn retag_ignores_code_fences() {
    let text = "正文 #AI\n\n```c\n#define AI 1\n```\n\n还是 #AI\n";
    let out = changed(retag_text(text, "AI", "人工智能"));
    assert!(out.contains("#define AI 1"), "代码块不该被改：{out}");
    assert_eq!(out.matches("#人工智能").count(), 2);
}

#[test]
fn retag_handles_front_matter_inline_list() {
    let text = "---\ntags: [AI, 笔记]\n---\n\n正文\n";
    let out = changed(retag_text(text, "AI", "人工智能"));
    assert!(out.contains("tags: [人工智能, 笔记]"), "{out}");
}

#[test]
fn retag_handles_front_matter_dash_list() {
    let text = "---\ntags:\n  - AI\n  - 笔记\n---\n\n正文\n";
    let out = changed(retag_text(text, "AI", "人工智能"));
    assert!(out.contains("- 人工智能"), "{out}");
    assert!(out.contains("- 笔记"), "{out}");
}

#[test]
fn retag_skips_when_the_target_already_exists() {
    let text = "#AI\n";
    assert!(matches!(
        retag_text(text, "AI", "AI"),
        TagOutcome::Skipped(_)
    ));
}

#[test]
fn retag_returns_unchanged_when_absent() {
    assert!(matches!(
        retag_text("#别的\n", "AI", "人工智能"),
        TagOutcome::Unchanged
    ));
}

#[test]
fn add_tag_appends_to_front_matter_when_present() {
    let text = "---\ntags: [笔记]\n---\n\n正文\n";
    let out = changed(add_tag_text(text, "AI"));
    assert!(out.contains("tags: [笔记, AI]"), "{out}");
}

#[test]
fn add_tag_creates_the_key_when_front_matter_has_none() {
    let text = "---\ntitle: 标题\n---\n\n正文\n";
    let out = changed(add_tag_text(text, "AI"));
    assert!(out.contains("tags: [AI]"), "{out}");
    assert!(out.contains("title: 标题"), "原有字段不能丢：{out}");
}

#[test]
fn add_tag_appends_a_line_when_there_is_no_front_matter() {
    let out = changed(add_tag_text("正文一行\n", "AI"));
    assert_eq!(out, "正文一行\n\n#AI\n");
}

#[test]
fn add_tag_skips_when_already_there() {
    assert!(matches!(
        add_tag_text("#AI\n", "AI"),
        TagOutcome::Skipped(_)
    ));
}

#[test]
fn remove_tag_drops_the_line_when_nothing_is_left() {
    let out = changed(remove_tag_text("标题\n\n#AI\n\n正文\n", "AI"));
    assert!(!out.contains("AI"), "{out}");
    assert!(out.contains("正文"));
    // 不留空行
    assert!(!out.contains("\n\n\n"), "{out:?}");
}

#[test]
fn remove_tag_empties_the_front_matter_key() {
    let out = changed(remove_tag_text("---\ntags: [AI]\n---\n\n正文\n", "AI"));
    assert!(!out.contains("tags:"), "删空后整行去掉：{out}");
    assert!(out.contains("正文"));
}

#[test]
fn remove_tag_returns_unchanged_when_absent() {
    assert!(matches!(
        remove_tag_text("正文\n", "AI"),
        TagOutcome::Unchanged
    ));
}

#[test]
fn apply_text_dispatches_by_kind() {
    assert!(matches!(
        apply_text(TaskKind::Retag, "#AI\n", "AI", "人工智能"),
        TagOutcome::Changed(_)
    ));
    assert!(matches!(
        apply_text(TaskKind::AddTag, "正文\n", "AI", ""),
        TagOutcome::Changed(_)
    ));
    assert!(matches!(
        apply_text(TaskKind::RemoveTag, "#AI\n", "AI", ""),
        TagOutcome::Changed(_)
    ));
}

#[test]
fn plan_marks_unreadable_files_as_failed_not_panic() {
    let items = plan(
        TaskKind::Retag,
        "AI",
        "人工智能",
        &[PathBuf::from("/不存在/x.md")],
    );
    assert_eq!(items.len(), 1);
    assert!(
        matches!(items[0].state, ItemState::Failed(_)),
        "{:?}",
        items[0].state
    );
}

#[test]
fn plan_only_lists_notes_that_would_change() {
    let root = dir("plan");
    let hit = root.join("hit.md");
    let miss = root.join("miss.md");
    std::fs::write(&hit, "#AI\n").unwrap();
    std::fs::write(&miss, "#别的\n").unwrap();

    let items = plan(TaskKind::Retag, "AI", "人工智能", &[hit, miss]);
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0].state, ItemState::Pending));
    assert!(
        matches!(&items[1].state, ItemState::Skipped(reason) if reason.contains("没有这个标签")),
        "{:?}",
        items[1].state
    );
    // 待处理那条要有可展示的预览
    assert!(
        items[0]
            .preview
            .iter()
            .any(|(before, after)| before == "#AI" && after == "#人工智能"),
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn has_tag_looks_at_body_and_front_matter_but_not_code() {
    assert!(super::has_tag("#AI\n", "AI"));
    assert!(super::has_tag("---\ntags: [AI]\n---\n", "AI"));
    assert!(super::has_tag("---\ntags:\n  - AI\n---\n", "AI"));
    assert!(!super::has_tag("#别的\n", "AI"));
    // 代码块里的 `#AI` 不是标签：否则 `批量删除` 会误删示例代码
    assert!(!super::has_tag("```c\n#AI\n```\n", "AI"));
    assert!(super::has_tag("```c\n#AI\n```\n\n#AI\n", "AI"));
    // 整 token 才算：`#AI助手` 里没有 `#AI`
    assert!(!super::has_tag("#AI助手\n", "AI"));
    assert!(!super::has_tag("#AI\n", ""), "空标签名不该命中任何东西");
}

#[test]
fn task_json_round_trips() {
    let root = dir("roundtrip");
    let mut task = Task::new(
        new_id(TaskKind::Retag, "AI"),
        TaskKind::Retag,
        "AI",
        "人工智能",
    );
    task.items = vec![TaskItem {
        path: root.join("a.md"),
        preview: vec![("#AI".into(), "#人工智能".into())],
        state: ItemState::Done,
    }];
    task.cursor = 1;
    task.status = TaskStatus::Paused;

    save(&root, &task).expect("落盘成功");
    let back = load(&root, &task.id).expect("读回成功");
    assert_eq!(back, task, "字段与状态要一模一样");
    assert!(
        super::task_dir(&root, &task.id)
            .join("progress.md")
            .exists()
    );

    let all = list(&root);
    assert_eq!(all.len(), 1, "能列出库内的任务");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn next_batch_skips_finished_items() {
    let mut task = Task::new("t".into(), TaskKind::Retag, "AI", "人工智能");
    let item = |state: ItemState| TaskItem {
        path: PathBuf::from("a.md"),
        preview: Vec::new(),
        state,
    };
    task.items = vec![
        item(ItemState::Done),
        item(ItemState::Pending),
        item(ItemState::Skipped("x".into())),
        item(ItemState::Pending),
    ];
    task.cursor = 1;

    let batch = task.next_batch(10);
    assert_eq!(batch, vec![1, 3], "已完成与跳过的不重复给");
    assert_eq!(task.pending(), 2);
    assert!(!task.is_finished());
    assert_eq!(task.stats(), (1, 1, 0));
}

#[test]
fn progress_md_reports_the_counts() {
    let mut task = Task::new("t".into(), TaskKind::RemoveTag, "AI", "");
    task.items = vec![
        TaskItem {
            path: PathBuf::from("/vault/a.md"),
            preview: Vec::new(),
            state: ItemState::Done,
        },
        TaskItem {
            path: PathBuf::from("/vault/b.md"),
            preview: Vec::new(),
            state: ItemState::Failed("读不到".into()),
        },
    ];
    let md = progress_md(&task);
    assert!(md.contains("已完成 1"), "{md}");
    assert!(md.contains("失败 1"), "{md}");
    assert!(md.contains("a.md"), "{md}");
}

#[test]
fn tasks_live_under_the_vaults_xink_dir() {
    let root = Path::new("/vault");
    let dir = super::tasks_root(root);
    assert!(dir.ends_with(".xink/agent/tasks"), "{dir:?}");
    assert!(
        crate::core::media::ai_excluded(&dir),
        "必须走 AI 专属目录判据"
    );
}
