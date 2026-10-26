//! 只读工具：执行、错误处理、结果裁剪与只读性

use std::path::{Path, PathBuf};

use super::{Tool, ToolCall, ToolHost, execute, specs};
use crate::core::index::LinkRef;
use crate::core::search::Hit;

/// 内存实现：不碰磁盘，用来断言工具的输出格式
struct MemHost {
    root: PathBuf,
    notes: Vec<(PathBuf, String)>,
    links: Vec<LinkRef>,
}

fn title_of(content: &str) -> String {
    content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("无标题")
        .trim_start_matches('#')
        .trim()
        .to_string()
}

impl ToolHost for MemHost {
    fn read_note(&self, path: &Path) -> Option<String> {
        let full = self.root.join(path);
        self.notes
            .iter()
            .find(|(p, _)| *p == full || p.ends_with(path))
            .map(|(_, c)| c.clone())
    }

    fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let mut out = Vec::new();
        for (path, content) in &self.notes {
            for (line_no, line) in content.lines().enumerate() {
                if line.contains(query) {
                    out.push(Hit {
                        path: path.clone(),
                        line: line_no,
                        text: line.to_string(),
                    });
                    if out.len() >= limit {
                        return out;
                    }
                }
            }
        }
        out
    }

    fn tags(&self) -> Vec<(String, usize)> {
        vec![("rust".into(), 3), ("笔记".into(), 1)]
    }

    fn backlinks(&self, path: &Path) -> Vec<LinkRef> {
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string());
        self.links
            .iter()
            .filter(|link| Some(&link.target) == stem.as_ref())
            .cloned()
            .collect()
    }

    fn list_notes(&self, limit: usize) -> Vec<(PathBuf, String)> {
        self.notes
            .iter()
            .take(limit)
            .map(|(path, content)| (path.clone(), title_of(content)))
            .collect()
    }

    fn root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

/// 只读 host：只实现读盘，用来验证工具不会改动磁盘
struct DiskHost {
    root: PathBuf,
}

impl ToolHost for DiskHost {
    fn read_note(&self, path: &Path) -> Option<String> {
        let full = self.root.join(path);
        std::fs::read_to_string(&full)
            .or_else(|_| std::fs::read_to_string(path))
            .ok()
    }
    fn search(&self, _query: &str, _limit: usize) -> Vec<Hit> {
        Vec::new()
    }
    fn tags(&self) -> Vec<(String, usize)> {
        Vec::new()
    }
    fn backlinks(&self, _path: &Path) -> Vec<LinkRef> {
        Vec::new()
    }
    fn list_notes(&self, _limit: usize) -> Vec<(PathBuf, String)> {
        Vec::new()
    }
    fn root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
}

fn host() -> MemHost {
    let root = PathBuf::from("/vault");
    MemHost {
        notes: vec![
            (root.join("a.md"), "# 标题A\n\n正文里提到 rust。\n".into()),
            (
                root.join("sub/b.md"),
                "# 标题B\n\n## 小节\n另一篇。\n".into(),
            ),
        ],
        links: vec![LinkRef {
            from: root.join("sub/b.md"),
            line: 2,
            target: "a".into(),
        }],
        root,
    }
}

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        name: name.into(),
        arguments: arguments.into(),
    }
}

#[test]
fn read_note_returns_content() {
    let result = execute(&call("read_note", r#"{"path":"a.md"}"#), &host(), 4000);
    assert!(result.output.contains("标题A"), "{:?}", result.output);
    assert!(result.output.contains("rust"));
}

#[test]
fn read_note_missing_file_is_text_not_error() {
    let result = execute(&call("read_note", r#"{"path":"nope.md"}"#), &host(), 4000);
    assert!(result.output.contains("读不到"), "{:?}", result.output);
}

#[test]
fn search_notes_formats_path_line_text() {
    // 行号对模型 / 用户都是 1-based（`Hit::line` 是 0-based）
    let result = execute(&call("search_notes", r#"{"query":"rust"}"#), &host(), 4000);
    assert_eq!(result.output, "a.md:3: 正文里提到 rust。");
}

#[test]
fn search_notes_no_hit_says_so() {
    let result = execute(
        &call("search_notes", r#"{"query":"不存在"}"#),
        &host(),
        4000,
    );
    assert!(result.output.contains("没有命中"), "{:?}", result.output);
}

#[test]
fn list_tags_and_notes() {
    let h = host();
    assert_eq!(
        execute(&call("list_tags", "{}"), &h, 4000).output,
        "#rust (3)\n#笔记 (1)"
    );
    let notes = execute(&call("list_notes", "{}"), &h, 4000).output;
    assert!(notes.contains("a.md — 标题A"), "{notes}");
    assert!(notes.contains("sub/b.md — 标题B"), "{notes}");
}

#[test]
fn list_backlinks_and_outline() {
    let h = host();
    let links = execute(&call("list_backlinks", r#"{"path":"a.md"}"#), &h, 4000).output;
    assert!(links.contains("sub/b.md:3: [[a]]"), "{links}");

    // outline 走 trait 默认实现：从正文解析标题并按层级缩进
    let outline = execute(&call("outline", r#"{"path":"sub/b.md"}"#), &h, 4000).output;
    assert_eq!(outline, "标题B\n  小节");
}

#[test]
fn empty_result_says_so() {
    let h = host();
    assert!(
        execute(&call("list_backlinks", r#"{"path":"zzz.md"}"#), &h, 4000)
            .output
            .contains("没有笔记引用")
    );
    // outline 的默认实现读不到时也给得出话
    assert!(
        execute(&call("outline", r#"{"path":"zzz.md"}"#), &h, 4000)
            .output
            .contains("读不到")
    );
}

#[test]
fn unknown_tool_lists_available() {
    let result = execute(&call("delete_note", "{}"), &host(), 4000);
    assert!(
        result.output.contains("没有这个工具：delete_note"),
        "{:?}",
        result.output
    );
    assert!(result.output.contains("read_note"), "{:?}", result.output);
    // 写工具在 M1 不存在，也不能被解析出来
    assert!(Tool::parse("propose_edit").is_none());
}

#[test]
fn broken_arguments_do_not_panic() {
    let h = host();
    // 不是 JSON
    let result = execute(&call("read_note", "not json"), &h, 4000);
    assert!(
        result.output.contains("缺少参数 path"),
        "{:?}",
        result.output
    );
    // 是 JSON 但字段类型不对
    let result = execute(&call("search_notes", r#"{"query":123}"#), &h, 4000);
    assert!(
        result.output.contains("缺少参数 query"),
        "{:?}",
        result.output
    );
    // 空参数
    let result = execute(&call("list_tags", ""), &h, 4000);
    assert!(result.output.contains("#rust"), "{:?}", result.output);
}

#[test]
fn long_output_is_clipped() {
    let result = execute(&call("read_note", r#"{"path":"a.md"}"#), &host(), 5);
    assert!(result.output.contains("已截断"), "{:?}", result.output);
    // 只留前 5 个字符，后面接截断说明
    assert!(result.output.starts_with("# 标题A"), "{:?}", result.output);
    assert!(
        result.output.contains("只回传前 5 个"),
        "{:?}",
        result.output
    );
}

#[test]
fn tools_never_touch_the_disk() {
    let dir = std::env::temp_dir().join("xink-tools-readonly");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.md");
    std::fs::write(&path, "# T\n\nbody\n").unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let before_text = std::fs::read_to_string(&path).unwrap();

    let h = DiskHost { root: dir.clone() };
    for tool in Tool::all() {
        let _ = execute(
            &call(tool.name(), r#"{"path":"note.md","query":"body"}"#),
            &h,
            4000,
        );
    }

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before_text,
        "内容被改了"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "mtime 被改了"
    );
    // 目录里也没多出文件
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn specs_cover_all_tools() {
    let specs = specs();
    assert_eq!(specs.len(), Tool::all().len());
    for spec in specs {
        let name = spec["function"]["name"].as_str().unwrap_or_default();
        assert!(
            Tool::parse(name).is_some(),
            "schema 里的名字必须能被解析回工具：{name}"
        );
    }
}
