//! Agent Skills：`SKILL.md` 的发现、解析与资源定位
//!
//! 设计见 `docs/ai/ai-skills-design.md` §3.1（需求 A5.1–A5.7）。
//!
//! 两条约定：
//! 1. **发现与注入都由 xink 完成**，不依赖任何 CLI 的自动发现
//!    （HTTP 与 CLI provider 下行为一致）。
//! 2. **不引 YAML 依赖**：front-matter 只需要 `name` / `description` 两个键，
//!    自己扫一遍 `key: value` 就够了。

use std::path::{Component, Path, PathBuf};

use crate::core::media::AI_DIR_NAME;

/// skill 文件名
pub const SKILL_FILE: &str = "SKILL.md";
/// 单个 skill 正文的字符上限：再长就把上下文吃光了
const MAX_BODY_CHARS: usize = 32_000;

/// skill 来自哪里
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// 库内 `<vault>/.xink/skills/<name>/`
    Vault,
    /// 用户级 `~/Library/Application Support/xink/skills/<name>/`
    User,
}

impl SkillSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Vault => "库内",
            Self::User => "用户级",
        }
    }
}

/// 一个可用的 skill
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// front-matter: `name`（必填）
    pub name: String,
    /// front-matter: `description`（必填）
    pub description: String,
    /// `SKILL.md` 所在目录：`references/` 等在它下面
    pub dir: PathBuf,
    /// 去掉 front-matter 之后的正文
    pub body: String,
    pub source: SkillSource,
}

/// 被跳过的 skill 及原因（设置界面要列出来，不能默默消失）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillIssue {
    pub path: PathBuf,
    pub reason: String,
}

/// 库内 skills 目录：`<vault>/.xink/skills`
pub fn vault_skills_dir(vault_root: &Path) -> PathBuf {
    vault_root.join(AI_DIR_NAME).join("skills")
}

/// 用户级 skills 目录：`<app_data>/skills`
pub fn user_skills_dir(app_data: &Path) -> PathBuf {
    app_data.join("skills")
}

/// 发现两级目录里的 skill
///
/// 同名时**库内优先**；缺 `name` / `description`、front-matter 不闭合、
/// 正文读不出来的一律进 `issues`，不进列表。
pub fn discover(vault_root: Option<&Path>, user_dir: &Path) -> (Vec<Skill>, Vec<SkillIssue>) {
    let mut skills: Vec<Skill> = Vec::new();
    let mut issues: Vec<SkillIssue> = Vec::new();

    // 先扫用户级，再扫库内：库内的同名项会覆盖用户级（同名库内优先）
    let mut dirs = vec![(user_dir.to_path_buf(), SkillSource::User)];
    if let Some(root) = vault_root {
        dirs.push((vault_skills_dir(root), SkillSource::Vault));
    }

    for (root, source) in dirs {
        for dir in child_dirs(&root) {
            let path = dir.join(SKILL_FILE);
            if !path.is_file() {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => match parse(&text, source) {
                    Ok(mut skill) => {
                        skill.dir = dir.clone();
                        // 同名的用户级 skill 被库内的顶掉
                        skills.retain(|s| s.name != skill.name);
                        skills.push(skill);
                    }
                    Err(reason) => issues.push(SkillIssue {
                        path: path.clone(),
                        reason,
                    }),
                },
                Err(err) => issues.push(SkillIssue {
                    path,
                    reason: format!("读取失败：{err}"),
                }),
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    (skills, issues)
}

/// 解析 front-matter + 正文
fn parse(text: &str, source: SkillSource) -> Result<Skill, String> {
    let text = text.trim_start_matches('\u{feff}');
    let rest = text
        .strip_prefix("---")
        .ok_or("缺少 front-matter（`---` 开头）")?;
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let Some(end) = find_front_matter_end(rest) else {
        return Err("front-matter 没有闭合".into());
    };
    let front = &rest[..end];
    // `end` 指向闭合行 `---` 的开头，正文从它的下一行开始（吃掉空行）
    let tail = &rest[end..];
    let body = match tail.find('\n') {
        Some(ix) => &tail[ix + 1..],
        None => "",
    };
    let body = body.trim_start();

    let mut name = None;
    let mut description = None;
    for line in front.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        match key.trim() {
            "name" => name = Some(value),
            "description" => description = Some(value),
            _ => {}
        }
    }

    let name = name.filter(|v| !v.trim().is_empty()).ok_or("缺少 name")?;
    let description = description
        .filter(|v| !v.trim().is_empty())
        .ok_or("缺少 description")?;

    let body: String = body.chars().take(MAX_BODY_CHARS).collect();
    Ok(Skill {
        name,
        description,
        dir: PathBuf::new(),
        body,
        source,
    })
}

/// front-matter 结束行的下标（相对 `rest` 的字节偏移）
fn find_front_matter_end(rest: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in rest.lines() {
        if line.trim() == "---" {
            return Some(offset);
        }
        offset += line.len() + 1;
    }
    None
}

fn child_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    out.sort();
    out
}

/// 命令面板 / 上下文用的清单：`- name：description`
pub fn catalog(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::from("可用技能：\n");
    for skill in skills {
        out.push_str(&format!("- {}：{}\n", skill.name, skill.description));
    }
    out
}

/// 按名字取一个启用的 skill
pub fn find<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    skills.iter().find(|skill| skill.name == name)
}

/// 过滤掉被停用的（配置里的 `disabled_skills`）
pub fn enabled<'a>(skills: &'a [Skill], disabled: &[String]) -> Vec<&'a Skill> {
    skills
        .iter()
        .filter(|skill| !disabled.iter().any(|name| name == &skill.name))
        .collect()
}

/// 定位 skill 的附带资源
///
/// **只能读 skill 目录内**：`..`、绝对路径、Windows 盘符一律拒绝，
/// 免得一个 skill 把笔记库外的文件读走。
pub fn resource_path(skill: &Skill, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        return None;
    }
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(skill.dir.join(rel_path))
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/skills.rs"]
mod tests;
