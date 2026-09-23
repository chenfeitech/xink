//! Markdown 预处理
//!
//! gpui-base 出于安全考虑，不会把 scheme-less 的字符串隐式解析为本地文件
//! （避免文档通过相对路径偷偷读取磁盘）。因此笔记里的相对图片路径
//! 必须由应用自己补全成可加载的 URL。

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

static IMAGE_LINK: OnceLock<Regex> = OnceLock::new();

fn image_link_re() -> &'static Regex {
    // 允许 URL 内含空格（截图文件名很常见），到第一个 `)` 结束
    IMAGE_LINK
        .get_or_init(|| Regex::new(r#"!\[[^\]]*\]\(([^)]+)\)"#).expect("图片链接的正则应能编译"))
}

/// 不需要补全的 URL：远程、内嵌、已经是绝对形式
fn is_absolute_url(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
        || url.starts_with("file://")
        || url.starts_with('/')
}

/// 一行是否为分隔线（`---` / `***` / `___`，三个及以上且只允许空格）
fn is_thematic_break(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let first = trimmed.chars().next().unwrap_or_default();
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    trimmed.chars().all(|c| c == first)
}

/// 交给预览解析器前的防御性预处理。
///
/// gpui-base 的预览解析（底层的 `markdown` crate）在遇到「段落 / `---` / `---` /
/// 段落 / `---`」这类连续分隔线与段落交替的输入时会 panic（`Cannot push to
/// non-parent`）。解析发生在后台任务里，panic 会让整个进程 abort，用户表现为应用闪退。
///
/// 这里在解析前给**连续的分隔线之间补一个空行**：既让解析器避开该缺陷，
/// 又保留了两个元素本身（分隔线仍是分隔线，setext 标题仍是标题），
/// 比直接删掉其中一行更贴近原文语义。
pub fn sanitize_for_preview(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut prev_was_break = false;

    for line in md.lines() {
        let is_break = is_thematic_break(line);
        if is_break && prev_was_break {
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
        prev_was_break = is_break;
    }

    // 保留原始结尾：原本没有换行则不补，避免无意义改动
    if !md.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// 把 Markdown 中的相对图片路径改写为基于笔记所在目录的 file:// URL
pub fn resolve_image_urls(md: &str, note_dir: &Path) -> String {
    let re = image_link_re();
    re.replace_all(md, |caps: &regex::Captures| {
        let whole = caps[0].to_string();
        let inner = caps[1].to_string();
        // Markdown 允许 `![alt](url "title")`，URL 与 title 之间以空格分隔
        let url = match inner.find('"') {
            Some(idx) => inner[..idx].trim_end(),
            None => inner.trim_end(),
        };
        if url.is_empty() || is_absolute_url(url) {
            return whole;
        }
        let resolved = note_dir.join(url);
        let file_url = format!("file://{}", encode_path(&resolved));
        whole.replace(&url, &file_url)
    })
    .into_owned()
}

/// 路径的最小 percent-encoding：保留 URL 安全字符，其余按字节转义（中文也会被编码）
fn encode_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests;
