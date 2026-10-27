// 显式导入而非 `use super::*`：本文件顶部有 `use gpui_kit::*`，
// 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性，导致宏展开递归。
//
// 这里只测**不依赖 App 上下文**的纯函数：候选的收集与相对路径，
// 面板的交互（Dialog / Command）按设计文档走人工验收。
use std::path::{Path, PathBuf};

use super::{AiTarget, Candidate, collect_candidates, flatten};
use crate::core::config::AiConfig;
use crate::core::vault::FileNode;

fn node(path: &str, is_dir: bool) -> FileNode {
    FileNode {
        path: PathBuf::from(path),
        name: Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        is_dir,
        children: Vec::new(),
    }
}

fn collect(root: &str, tree: &FileNode) -> Vec<Candidate> {
    let mut out = Vec::new();
    flatten(Path::new(root), tree, &mut out);
    out
}

#[test]
fn flatten_collects_editable_files_only() {
    let root = "/vault";
    let mut dir = node("/vault/notes", true);
    dir.children = vec![
        node("/vault/notes/a.md", false),
        node("/vault/notes/config.json", false),
        node("/vault/notes/logo.png", false),
        node("/vault/notes/movie.mp4", false),
    ];

    let candidates = collect(root, &dir);
    let labels: Vec<&str> = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect();
    assert_eq!(labels, vec!["notes/a.md", "notes/config.json"]);
}

#[test]
fn label_is_relative_to_the_vault_root() {
    let mut dir = node("/vault/sub", true);
    dir.children = vec![node("/vault/sub/deep/note.md", false)];

    let candidates = collect("/vault", &dir);
    assert_eq!(candidates[0].label, "sub/deep/note.md");
    assert_eq!(candidates[0].path, PathBuf::from("/vault/sub/deep/note.md"));
}

#[test]
fn nested_directories_are_walked() {
    let mut inner = node("/vault/a/b", true);
    inner.children = vec![node("/vault/a/b/c.md", false)];
    let mut outer = node("/vault/a", true);
    outer.children = vec![inner, node("/vault/a/top.md", false)];

    assert_eq!(collect("/vault", &outer).len(), 2);
}

#[test]
fn empty_directory_yields_no_candidate() {
    assert!(collect("/vault", &node("/vault/empty", true)).is_empty());
}

#[test]
fn collect_candidates_scans_a_real_vault() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).expect("建子目录");
    std::fs::write(root.join("b.md"), "").unwrap();
    std::fs::write(root.join("notes/a.md"), "").unwrap();
    std::fs::write(root.join("notes/skip.png"), "").unwrap();
    // 隐藏文件与 .git 目录都不该进候选（与目录树一致）
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/config.json"), "{}").unwrap();

    let candidates = collect_candidates(root, &AiConfig::default()).expect("扫描成功");

    // 候选 = 库内文件 + 内置 AI 动作（需求 A9.1），整体按 label 排序
    let files: Vec<&str> = candidates
        .iter()
        .filter(|c| c.ai.is_none())
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(files, vec!["b.md", "notes/a.md"]);
    // 只数内置动作：用户级目录里可能真有 skill，数量不该被它带偏
    let actions: Vec<&str> = candidates
        .iter()
        .filter(|c| matches!(c.ai, Some(AiTarget::Action(_))))
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(actions.len(), crate::core::config::builtin_actions().len());
    assert!(
        actions.iter().all(|l| l.starts_with("AI: ")),
        "AI 动作要能用 `AI:` 前缀过滤出来：{actions:?}"
    );
    // AI 动作不是文件：确认后不该去打开路径
    assert!(
        candidates
            .iter()
            .filter(|c| c.ai.is_some())
            .all(|c| c.path.as_os_str().is_empty())
    );
}

#[test]
fn collect_candidates_lists_enabled_skills() {
    let dir = tempfile::tempdir().expect("创建临时目录");
    let root = dir.path();
    std::fs::write(root.join("a.md"), "").unwrap();
    let skill_dir = crate::core::ai::skills::vault_skills_dir(root).join("demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: 一个演示技能\n---\n\n正文\n",
    )
    .unwrap();

    let cfg = AiConfig::default();
    let candidates = collect_candidates(root, &cfg).expect("扫描成功");
    let skill = candidates
        .iter()
        .find(|c| matches!(&c.ai, Some(AiTarget::Skill(name)) if name == "demo"))
        .expect("库内 skill 该出现在候选里");
    assert_eq!(skill.label, "AI: demo", "与内置动作同一前缀才好过滤");
    assert_eq!(skill.hint.as_deref(), Some("一个演示技能"));
    assert!(
        skill.path.as_os_str().is_empty(),
        "skill 不是文件：确认后不该去打开路径"
    );
    // skill 住在 .xink 下：它自己不该作为"笔记"候选出现
    assert!(!candidates.iter().any(|c| c.label.contains("SKILL.md")));

    // 总开关关掉：skill 全消失，内置动作不受影响
    let off = AiConfig {
        skills_enabled: false,
        ..cfg.clone()
    };
    let candidates = collect_candidates(root, &off).expect("扫描成功");
    assert!(
        !candidates
            .iter()
            .any(|c| matches!(c.ai, Some(AiTarget::Skill(_))))
    );
    assert_eq!(
        candidates
            .iter()
            .filter(|c| matches!(c.ai, Some(AiTarget::Action(_))))
            .count(),
        crate::core::config::builtin_actions().len()
    );

    // 只停用 demo：只有它消失
    let disabled = AiConfig {
        disabled_skills: vec!["demo".to_string()],
        ..cfg.clone()
    };
    let candidates = collect_candidates(root, &disabled).expect("扫描成功");
    assert!(
        !candidates
            .iter()
            .any(|c| matches!(&c.ai, Some(AiTarget::Skill(name)) if name == "demo"))
    );
}
