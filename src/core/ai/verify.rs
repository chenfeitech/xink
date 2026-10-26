//! 结构校验：AI 改完之后，这篇笔记还"结构合法"吗
//!
//! 设计：`docs/ai/ai-assistant-design.md` §3.2。校验只在 loop 收尾时跑一次，
//! **结果只提示、不阻断**（设计 Q5）——库里确实可能还没有那篇笔记，
//! "双链目标不存在"对草稿来说是正常的。
//!
//! 三条：双链可解析、frontmatter 合法、frontmatter 的 `tags` 结构合法。

use crate::core::datafile::{DataKind, parse as parse_data};
use crate::core::index::Resolution;
use crate::core::markdown::parse_facts;

/// 校验要用到的索引能力（UI 侧用 `NoteIndex` 实现，测试用内存实现）
pub trait LinkIndex {
    /// 解析一个双链目标
    fn resolve(&self, target: &str) -> Resolution;
}

/// 问题类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// 双链目标不存在或有歧义
    WikiLink,
    /// frontmatter 不是合法 YAML
    Frontmatter,
    /// frontmatter 的 `tags` 字段结构不对
    Tag,
}

/// 一个校验问题
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyIssue {
    pub kind: IssueKind,
    /// 1-based 行号；`0` 表示不属于某一行（整篇）
    pub line: usize,
    pub detail: String,
}

impl VerifyIssue {
    /// 给模型回灌用的一行文本
    pub fn as_feedback(&self) -> String {
        match self.line {
            0 => format!("[{}] {}", kind_name(self.kind), self.detail),
            line => format!("[{}] 第 {line} 行：{}", kind_name(self.kind), self.detail),
        }
    }
}

fn kind_name(kind: IssueKind) -> &'static str {
    match kind {
        IssueKind::WikiLink => "双链",
        IssueKind::Frontmatter => "frontmatter",
        IssueKind::Tag => "标签",
    }
}

/// 校验一篇笔记（AI 生成结果应用**之前**的候选文本）
///
/// `index` 为 `None`（没打开库）时跳过双链校验：没有库就谈不上"目标不存在"。
pub fn check(text: &str, index: Option<&dyn LinkIndex>) -> Vec<VerifyIssue> {
    let mut issues = Vec::new();
    let facts = parse_facts(text);

    if let Some(index) = index {
        for link in &facts.wiki_links {
            match index.resolve(&link.target) {
                Resolution::Hit(_) => {}
                Resolution::Missing => issues.push(VerifyIssue {
                    kind: IssueKind::WikiLink,
                    line: link.line + 1,
                    detail: format!("[[{}]] 指向的笔记不存在", link.target),
                }),
                Resolution::Ambiguous(paths) => issues.push(VerifyIssue {
                    kind: IssueKind::WikiLink,
                    line: link.line + 1,
                    detail: format!(
                        "[[{}]] 有 {} 篇同名笔记，歧义（{}）",
                        link.target,
                        paths.len(),
                        paths
                            .iter()
                            .take(3)
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join("、")
                    ),
                }),
            }
        }
    }

    match frontmatter(text) {
        // 有开头没有结尾：`---` 起手却没闭合，多半是模型写坏了
        Err(line) => issues.push(VerifyIssue {
            kind: IssueKind::Frontmatter,
            line,
            detail: "frontmatter 以 --- 开始但没有闭合".into(),
        }),
        Ok(Some((yaml, line))) => {
            let parsed = parse_data(&yaml, DataKind::Yaml, 2);
            if !parsed.is_ok() {
                issues.push(VerifyIssue {
                    kind: IssueKind::Frontmatter,
                    line,
                    detail: match parsed.error() {
                        Some(err) => format!("frontmatter 不是合法 YAML：{}", err.message),
                        None => "frontmatter 不是合法 YAML".into(),
                    },
                });
            } else {
                issues.extend(tag_issues(&yaml, line));
            }
        }
        // 没有 frontmatter：完全合法，什么都不做
        Ok(None) => {}
    }

    issues
}

/// 取 frontmatter 的 YAML 正文与起始行（1-based）
///
/// `Err(line)` 表示有开头没闭合；`Ok(None)` 表示这篇没有 frontmatter。
fn frontmatter(text: &str) -> Result<Option<(String, usize)>, usize> {
    let mut lines = text.lines().enumerate();
    // 第一行必须是 `---`
    let Some((_, first)) = lines.next() else {
        return Ok(None);
    };
    if first.trim() != "---" {
        return Ok(None);
    }
    let start = 2;
    let mut body = Vec::new();
    for (idx, line) in lines {
        if line.trim() == "---" {
            return Ok(Some((body.join("\n"), start)));
        }
        body.push(line);
        // 前 50 行还没闭合，基本可以断定不是 frontmatter（正文里的分隔线）
        if idx > 50 {
            return Err(start);
        }
    }
    Err(start)
}

/// `tags` 字段必须是字符串列表
///
/// 借 `serde_json::Value` 看结构：`noyalib` 是 serde 反序列化器，
/// 直接解成 JSON 的通用值即可，不必依赖它自己的 `Value` 变体名。
fn tag_issues(yaml: &str, line: usize) -> Vec<VerifyIssue> {
    let Ok(value) = noyalib::from_str::<serde_json::Value>(yaml) else {
        // 解析不过在上一步已经报过了，这里不重复
        return Vec::new();
    };
    let Some(tags) = value.get("tags") else {
        return Vec::new();
    };
    match tags {
        serde_json::Value::Array(items) => {
            let bad = items.iter().filter(|item| !item.is_string()).count();
            if bad > 0 {
                return vec![VerifyIssue {
                    kind: IssueKind::Tag,
                    line,
                    detail: format!("tags 里有 {bad} 项不是字符串"),
                }];
            }
            Vec::new()
        }
        // `tags: 单个标签` 也认（单标签很常见），不提示
        serde_json::Value::String(_) => Vec::new(),
        other => vec![VerifyIssue {
            kind: IssueKind::Tag,
            line,
            detail: format!("tags 应是列表，现在是 {}", kind_of(other)),
        }],
    }
}

fn kind_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "空",
        serde_json::Value::Bool(_) => "布尔",
        serde_json::Value::Number(_) => "数字",
        serde_json::Value::String(_) => "字符串",
        serde_json::Value::Array(_) => "列表",
        serde_json::Value::Object(_) => "映射",
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/verify.rs"]
mod tests;
