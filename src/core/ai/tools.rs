//! 只读工具：让模型能"看到"库里其它笔记
//!
//! 设计：`docs/ai/ai-assistant-design.md` §3.4。M1 只做只读工具；
//! 写工具（`propose_edit` / `create_note`）属于 M3。
//!
//! 工具要读索引与磁盘，但 `core` 不能碰 `Entity<NoteIndex>`（规则 02），
//! 因此把"库"抽成 [`ToolHost`]：UI 侧用 `AppState` 实现，测试用内存实现——
//! 与 `keychain::SecretStore` 同构，领域层因此可单测。

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::core::ai::skills::{self, Skill};
use crate::core::index::LinkRef;
use crate::core::markdown::parse_facts;
use crate::core::search::Hit;

/// 工具能看到的库
///
/// 全部是**只读**操作：这一层不提供任何写入口，写工具留给 M3 单独设计。
pub trait ToolHost {
    /// 读一篇笔记的正文；读不到（不存在 / 非 UTF-8 / 无权限）返回 `None`
    fn read_note(&self, path: &Path) -> Option<String>;
    /// 全文检索
    fn search(&self, query: &str, limit: usize) -> Vec<Hit>;
    /// 库内标签及引用次数
    fn tags(&self) -> Vec<(String, usize)>;
    /// 引用了 `path` 的位置
    fn backlinks(&self, path: &Path) -> Vec<LinkRef>;
    /// 大纲（标题列表）
    ///
    /// 默认从正文解析；实现方若已有索引里的 `DocFacts` 可覆写，省一次读盘。
    fn outline(&self, path: &Path) -> Vec<String> {
        let Some(text) = self.read_note(path) else {
            return Vec::new();
        };
        parse_facts(&text)
            .headings
            .iter()
            .map(|h| {
                format!(
                    "{}{}",
                    "  ".repeat((h.level.saturating_sub(1)) as usize),
                    h.text
                )
            })
            .collect()
    }
    /// 笔记清单：`(路径, 标题)`
    fn list_notes(&self, limit: usize) -> Vec<(PathBuf, String)>;
    /// 库根，用来把路径显示成相对路径（短、省 token）
    fn root(&self) -> Option<PathBuf>;
    /// 按名字取一个 skill（M2：`read_skill_resource` 要用它的目录）
    ///
    /// 默认没有 skill——测试宿主不关心这件事，只有 UI 侧的宿主会去
    /// 发现库内 / 用户级两级目录。
    fn skill(&self, _name: &str) -> Option<Skill> {
        None
    }
}

/// 内置工具
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// 读一篇笔记的正文
    ReadNote,
    /// 全文检索
    SearchNotes,
    /// 库内标签
    ListTags,
    /// 反向链接
    ListBacklinks,
    /// 某篇笔记的大纲
    Outline,
    /// 笔记清单
    ListNotes,
    /// 读 skill 自带的资源（`references/` 等）
    ///
    /// 只在 skills 总开关打开时才进工具表（见 [`specs`]）：关掉时模型
    /// 不该看到这个能力。
    ReadSkillResource,
}

impl Tool {
    pub fn all() -> [Tool; 7] {
        [
            Tool::ReadNote,
            Tool::SearchNotes,
            Tool::ListTags,
            Tool::ListBacklinks,
            Tool::Outline,
            Tool::ListNotes,
            Tool::ReadSkillResource,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::ReadNote => "read_note",
            Tool::SearchNotes => "search_notes",
            Tool::ListTags => "list_tags",
            Tool::ListBacklinks => "list_backlinks",
            Tool::Outline => "outline",
            Tool::ListNotes => "list_notes",
            Tool::ReadSkillResource => "read_skill_resource",
        }
    }

    /// 模型给出的名字 → 工具；不认识就 `None`（由 `execute` 回灌错误文本）
    pub fn parse(name: &str) -> Option<Tool> {
        Tool::all().into_iter().find(|t| t.name() == name)
    }

    fn description(self) -> &'static str {
        match self {
            Tool::ReadNote => "读取指定笔记的正文（给出完整路径）",
            Tool::SearchNotes => "按关键词全文检索笔记，返回命中行",
            Tool::ListTags => "列出库内标签及其使用次数",
            Tool::ListBacklinks => "列出引用了指定笔记的位置（反向链接）",
            Tool::Outline => "列出指定笔记的标题大纲",
            Tool::ListNotes => "列出库内笔记（路径与标题）",
            Tool::ReadSkillResource => {
                "读取技能自带的资源文件（如 references/），只能读该技能目录内"
            }
        }
    }

    fn parameters(self) -> Value {
        match self {
            Tool::ReadNote | Tool::Outline | Tool::ListBacklinks => json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "笔记路径（相对库根或绝对路径）" }
                },
                "required": ["path"]
            }),
            Tool::SearchNotes => json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "检索关键词" },
                    "limit": { "type": "integer", "description": "最多返回几条，默认 10" }
                },
                "required": ["query"]
            }),
            Tool::ListTags | Tool::ListNotes => json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "最多返回几条，默认 30" }
                }
            }),
            Tool::ReadSkillResource => json!({
                "type": "object",
                "properties": {
                    "skill": { "type": "string", "description": "技能名（SKILL.md 里的 name）" },
                    "path": {
                        "type": "string",
                        "description": "技能目录内的相对路径，如 references/a.md"
                    }
                },
                "required": ["skill", "path"]
            }),
        }
    }

    /// 交给 provider 的 JSON
    ///
    /// Ollama 与 OpenAI 兼容端点都是这个形状（`ollama::probe` 里实测过），
    /// 因此只出一份；两边的差异在 `arguments` 是对象还是字符串，由解析方处理。
    pub fn schema(self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name(),
                "description": self.description(),
                "parameters": self.parameters(),
            }
        })
    }
}

/// 这一轮要带的工具 schema
///
/// `skills` 为 false 时不带 [`Tool::ReadSkillResource`]：库里一个 skill 都没有
/// （或用户关了总开关）时，多一个工具就是多一份"模型瞎调"的机会。
pub fn specs(skills: bool) -> Vec<Value> {
    Tool::all()
        .into_iter()
        .filter(|tool| skills || *tool != Tool::ReadSkillResource)
        .map(|tool| tool.schema())
        .collect()
}

/// 模型发起的一次工具调用
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    /// 参数：Ollama 给对象、OpenAI 兼容给字符串，这里统一按 JSON 文本处理
    pub arguments: String,
}

/// 工具执行结果（回灌给模型）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub name: String,
    pub output: String,
}

/// 执行一次工具调用
///
/// **不返回 `Result`**：工具名不对、参数坏掉、笔记不存在都不是"错误"，
/// 而是要让模型知道并自己纠正——因此一律转成文本回灌
/// （`docs/ai/ai-assistant-design.md` §6）。
pub fn execute(call: &ToolCall, host: &dyn ToolHost, max_chars: usize) -> ToolResult {
    let output = match Tool::parse(&call.name) {
        None => format!(
            "没有这个工具：{}。可用的工具：{}",
            call.name,
            Tool::all()
                .iter()
                .map(|t| t.name())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(tool) => run(tool, &call.arguments, host),
    };
    ToolResult {
        name: call.name.clone(),
        output: clip(&output, max_chars),
    }
}

fn run(tool: Tool, arguments: &str, host: &dyn ToolHost) -> String {
    // 参数不是合法 JSON 时按"没给参数"处理，别把解析错误抛给上层
    let args: Value = serde_json::from_str(arguments).unwrap_or(Value::Null);
    let text = |key: &str| -> String {
        args.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let limit = |default: usize| -> usize {
        args.get("limit")
            .and_then(|v| v.as_u64())
            .map_or(default, |n| (n as usize).clamp(1, 50))
    };

    match tool {
        Tool::ReadNote => {
            let path = text("path");
            if path.is_empty() {
                return "缺少参数 path".into();
            }
            match host.read_note(Path::new(&path)) {
                Some(content) => content,
                None => format!("读不到这篇笔记：{path}"),
            }
        }
        Tool::SearchNotes => {
            let query = text("query");
            if query.is_empty() {
                return "缺少参数 query".into();
            }
            let hits = host.search(&query, limit(10));
            if hits.is_empty() {
                return format!("没有命中“{query}”");
            }
            hits.iter()
                .map(|hit| {
                    format!(
                        "{}:{}: {}",
                        show(host, &hit.path),
                        hit.line + 1,
                        hit.text.trim()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Tool::ListTags => {
            let tags = host.tags();
            if tags.is_empty() {
                return "库里还没有标签".into();
            }
            tags.iter()
                .take(limit(30))
                .map(|(tag, n)| format!("#{tag} ({n})"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Tool::ListBacklinks => {
            let given = text("path");
            if given.is_empty() {
                return "缺少参数 path".into();
            }
            let links = host.backlinks(Path::new(&given));
            if links.is_empty() {
                return format!("没有笔记引用“{given}”");
            }
            links
                .iter()
                .map(|link| {
                    format!(
                        "{}:{}: [[{}]]",
                        show(host, &link.from),
                        link.line + 1,
                        link.target
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Tool::Outline => {
            let given = text("path");
            if given.is_empty() {
                return "缺少参数 path".into();
            }
            let outline = host.outline(Path::new(&given));
            if outline.is_empty() {
                return format!("“{given}”没有标题，或读不到这篇笔记");
            }
            outline.join("\n")
        }
        Tool::ListNotes => {
            let notes = host.list_notes(limit(30));
            if notes.is_empty() {
                return "库里还没有笔记".into();
            }
            notes
                .iter()
                .map(|(path, title)| format!("{} — {}", show(host, path), title))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Tool::ReadSkillResource => {
            let name = text("skill");
            let rel = text("path");
            if name.is_empty() || rel.is_empty() {
                return "缺少参数 skill 或 path".into();
            }
            let Some(skill) = host.skill(&name) else {
                return format!("没有这个技能：{name}");
            };
            // 越界路径由 `resource_path` 挡掉（`..`、绝对路径、盘符），
            // 一个 skill 不该把手伸到它自己的目录之外
            let Some(path) = skills::resource_path(&skill, &rel) else {
                return format!("不允许的资源路径：{rel}（只能读技能目录内的文件）");
            };
            match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(_) => format!("读不到技能资源：{rel}"),
            }
        }
    }
}

/// 路径显示成相对库根：短、省 token；剥不出来就退回绝对路径
fn show(host: &dyn ToolHost, path: &Path) -> String {
    match host.root() {
        Some(root) => path
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string_lossy().to_string()),
        None => path.to_string_lossy().to_string(),
    }
}

/// 裁到 `max_chars` 个字符（按字符数，不切断 UTF-8）
fn clip(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    let keep: String = text.chars().take(max_chars).collect();
    format!("{keep}\n…（已截断：共 {total} 字符，只回传前 {max_chars} 个）")
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/tools.rs"]
mod tests;
