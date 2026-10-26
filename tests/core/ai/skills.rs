//! SKILL.md 发现 / 解析 / 资源定位的测试

use std::path::{Path, PathBuf};

use super::{SkillSource, catalog, discover, enabled, find, resource_path, vault_skills_dir};

/// 建一个临时 skills 目录（每次用不同名字，避免并发撞车）
fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xink-skills-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, skill: &str, content: &str) -> PathBuf {
    let path = dir.join(skill).join("SKILL.md");
    std::fs::create_dir_all(dir.join(skill)).unwrap();
    std::fs::write(&path, content).unwrap();
    path
}

const OK: &str = "---\nname: demo\ndescription: 一个演示技能\n---\n\n正文第一行\n正文第二行\n";

#[test]
fn parses_front_matter_name_and_description() {
    let dir = tmp("parse");
    write(&dir, "demo", OK);
    let (skills, issues) = discover(None, &dir);

    assert_eq!(skills.len(), 1, "应解析出 1 个：{issues:?}");
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(skills[0].name, "demo");
    assert_eq!(skills[0].description, "一个演示技能");
    assert!(
        skills[0].body.starts_with("正文第一行"),
        "正文不该含 front-matter：{:?}",
        skills[0].body
    );
    assert!(!skills[0].body.contains("---"));
    assert_eq!(skills[0].source, SkillSource::User);
    assert_eq!(skills[0].dir, dir.join("demo"));
}

#[test]
fn missing_required_field_is_reported() {
    let dir = tmp("missing");
    write(&dir, "no-desc", "---\nname: no-desc\n---\n\n正文\n");
    write(&dir, "no-name", "---\ndescription: 没有名字\n---\n\n正文\n");
    let (skills, issues) = discover(None, &dir);

    assert!(skills.is_empty(), "缺必填字段的不该进列表：{skills:?}");
    assert_eq!(issues.len(), 2);
    assert!(issues.iter().any(|i| i.reason.contains("description")));
    assert!(issues.iter().any(|i| i.reason.contains("name")));
}

#[test]
fn unclosed_front_matter_is_reported() {
    let dir = tmp("unclosed");
    write(&dir, "broken", "---\nname: broken\n\n正文\n");
    let (skills, issues) = discover(None, &dir);
    assert!(skills.is_empty());
    assert!(
        issues.iter().any(|i| i.reason.contains("闭合")),
        "{issues:?}"
    );
}

#[test]
fn directory_without_skill_file_is_ignored() {
    let dir = tmp("noskill");
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let (skills, issues) = discover(None, &dir);
    assert!(skills.is_empty(), "{skills:?}");
    assert!(issues.is_empty(), "没有 SKILL.md 不算错误：{issues:?}");
}

#[test]
fn vault_skill_wins_over_user_skill() {
    let user = tmp("user-dup");
    let vault = tmp("vault-dup");
    write(
        &user,
        "demo",
        "---\nname: demo\ndescription: 用户级的\n---\n\nU\n",
    );
    write(
        &vault_skills_dir(&vault),
        "demo",
        "---\nname: demo\ndescription: 库内的\n---\n\nV\n",
    );

    let (skills, _) = discover(Some(&vault), &user);
    assert_eq!(skills.len(), 1, "同名应只留一个：{skills:?}");
    assert_eq!(skills[0].source, SkillSource::Vault);
    assert_eq!(skills[0].description, "库内的");
}

#[test]
fn vault_skills_dir_is_under_the_ai_dir() {
    assert_eq!(
        vault_skills_dir(std::path::Path::new("/v")),
        PathBuf::from("/v/.xink/skills")
    );
}

#[test]
fn disabled_skills_are_filtered_out() {
    let dir = tmp("disabled");
    write(&dir, "a", "---\nname: a\ndescription: A\n---\n\na\n");
    write(&dir, "b", "---\nname: b\ndescription: B\n---\n\nb\n");
    let (skills, _) = discover(None, &dir);

    let disabled = vec!["a".to_string()];
    let live = enabled(&skills, &disabled);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].name, "b");
    assert!(find(&skills, "a").is_some(), "find 不区分启停");
    assert!(find(&skills, "nope").is_none());
}

#[test]
fn resource_path_rejects_escaping_paths() {
    let dir = tmp("resource");
    write(&dir, "demo", OK);
    let (skills, _) = discover(None, &dir);
    let skill = &skills[0];

    assert_eq!(
        resource_path(skill, "references/a.md"),
        Some(skill.dir.join("references/a.md"))
    );
    assert_eq!(
        resource_path(skill, "../outside.md"),
        None,
        "不许跳出 skill 目录"
    );
    assert_eq!(resource_path(skill, "/etc/passwd"), None, "不许绝对路径");
    assert_eq!(resource_path(skill, "a/../../b"), None);
    assert_eq!(resource_path(skill, "  "), None, "空路径");
}

#[test]
fn catalog_lists_name_and_description() {
    let dir = tmp("catalog");
    write(&dir, "demo", OK);
    let (skills, _) = discover(None, &dir);
    let text = catalog(&skills);
    assert!(text.contains("demo"), "{text}");
    assert!(text.contains("一个演示技能"), "{text}");
    assert!(catalog(&[]).is_empty());
}
