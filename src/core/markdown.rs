//! Markdown 预处理
//!
//! gpui-base 出于安全考虑，不会把 scheme-less 的字符串隐式解析为本地文件
//! （避免文档通过相对路径偷偷读取磁盘）。因此笔记里的相对图片路径
//! 必须由应用自己补全成可加载的 URL。

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use regex::Regex;

use crate::core::config::ImageDisplay;

static IMAGE_LINK: OnceLock<Regex> = OnceLock::new();

fn image_link_re() -> &'static Regex {
    // 允许 URL 内含空格（截图文件名很常见），到第一个 `)` 结束
    IMAGE_LINK
        .get_or_init(|| Regex::new(r#"!\[[^\]]*\]\(([^)]+)\)"#).expect("图片链接的正则应能编译"))
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

/// 图片缓存条目：`(修改时间, 文件长度, data URL)`。
///
/// 抽成别名是为了让下面的缓存类型不必写成一长串嵌套泛型。
///
/// data URL 用 `Arc<str>` 共享：一张 5MB 的图编码后约 6.7MB，
/// 命中时若按 `String` 克隆就是每次预览刷新都搬几 MB。
type ImageCacheEntry = (SystemTime, u64, Arc<str>);

/// 图片转 data URL 的缓存：避免每次刷新预览都重新读盘与编码。
///
/// 键是图片绝对路径，值是 `(修改时间, 文件长度, data URL)`；
/// 用 mtime + 长度作为内容指纹，足够可靠且比全文哈希便宜。
static IMAGE_DATA_URLS: OnceLock<Mutex<HashMap<PathBuf, ImageCacheEntry>>> = OnceLock::new();

/// 单张图片转 data URL 的体积上限（5 MB）。
///
/// data URL 比原图膨胀约 1/3，且整段塞进 Markdown 文本交给解析器，
/// 超限的图直接跳过（退回 file:// 形式，加载不出来），也好过拖垮解析与内存。
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// 把 `file://...` 形式的 URL 还原成本地路径（供点击图片后定位原图）
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    strip_file_scheme(url).map(PathBuf::from)
}

/// 扩展名 → data URL 的 MIME 类型
fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => return None,
    };
    Some(mime)
}

/// 标准 base64 编码（图片转 data URL 只需要这一小段，不为此引入依赖）
fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() / 3 * 4 + 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 把本地图片编码成 `data:` URL；读不到、格式不支持或体积超限则返回 None。
///
/// **为什么必须转成 data URL**：gpui-base 会把 Markdown 里的图片 URL 原样交给
/// `cx.http_client()`（gpui-pre `elements/img.rs:635`），而桌面端默认装的是
/// `NullHttpClient`（gpui-pre `app.rs:175-181`），于是 `file://` 与裸路径
/// **都加载不出来**——这正是笔记里图片显示为空白 / 破损的原因。
/// 框架唯一内联支持的来源是 `data:` URL（gpui-base text/utils.rs）。
fn image_data_url(path: &Path) -> Option<Arc<str>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let key = path.to_path_buf();

    // 命中缓存且指纹未变则直接复用
    {
        let cache = IMAGE_DATA_URLS.get_or_init(Default::default).lock().ok()?;
        if let Some((cached_mtime, cached_len, url)) = cache.get(&key)
            && *cached_mtime == mtime
            && *cached_len == meta.len()
        {
            return Some(url.clone());
        }
    }

    let bytes = std::fs::read(path).ok()?;
    let url: Arc<str> =
        Arc::from(format!("data:{};base64,{}", mime_for(path)?, encode_base64(&bytes)).as_str());

    if let Ok(mut cache) = IMAGE_DATA_URLS.get_or_init(Default::default).lock() {
        cache.insert(key, (mtime, meta.len(), url.clone()));
    }
    Some(url)
}

/// 图片渲染选项（由预览配置下发，避免 core 层反向依赖运行时状态）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageOptions {
    /// 是否内联渲染图片；`Hidden` 时退化为纯文本链接
    pub display: ImageDisplay,
    /// 图片宽度占预览区宽度的百分比（20–100）
    pub width_percent: u32,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            display: ImageDisplay::FitWidth,
            width_percent: 100,
        }
    }
}

/// 取 `![alt](url)` 里的 alt 文本
fn alt_of(whole: &str) -> &str {
    whole
        .strip_prefix("![")
        .and_then(|s| s.split_once(']'))
        .map(|(alt, _)| alt)
        .unwrap_or("")
}

/// 把 Markdown 中的图片路径改写为预览里真正能加载的形式
///
/// - 本地图片（相对路径 / 绝对路径 / `file://`）→ 转 `data:` URL，否则显示不出来；
/// - 渲染成内联 `<img>` 并用 `style="width: N%"` 指定宽度：
///   框架支持内联 HTML 的 width（px / %），不指定的话图文同行时图片会被压成
///   0.75 行高，显得特别小；
/// - 外面包一层指向自身的链接 `[<img …>](file://…)`：框架没有图片点击回调，
///   只有链接回调，包一层才能在点击时打开大图；
/// - `Hidden` 时去掉 `!` 退化为纯文本链接，需要时点击打开；
/// - 远程 `http(s)://` 与已是 `data:` 的保持原样。
pub fn resolve_image_urls(md: &str, note_dir: &Path, opts: ImageOptions) -> String {
    let re = image_link_re();
    re.replace_all(md, |caps: &regex::Captures| {
        let whole = caps[0].to_string();
        let inner = caps[1].to_string();
        // Markdown 允许 `![alt](url "title")`，URL 与 title 之间以空格分隔
        let url = match inner.find('"') {
            Some(idx) => inner[..idx].trim_end(),
            None => inner.trim_end(),
        };
        if url.is_empty() || url.starts_with("data:") || is_remote_url(url) {
            return whole;
        }

        // 还原成本地路径：`file://` 需解码，其余按绝对/相对路径解析
        let path = match strip_file_scheme(url) {
            Some(p) => PathBuf::from(p),
            None if url.starts_with('/') => PathBuf::from(url),
            None => note_dir.join(url),
        };
        let file_url = format!("file://{}", encode_path(&path));

        // 不显示图片：退化成一条纯文本链接（点击仍可打开预览）
        if opts.display == ImageDisplay::Hidden {
            let link_only = whole.strip_prefix('!').unwrap_or(&whole);
            return replace_last(link_only, url, &file_url);
        }

        // 读不到 / 格式不支持 / 超限时退回 file://，至少链接指向正确位置。
        // 命中缓存时拿到的是共享的 Arc：整段 data URL 不再逐次拷贝
        let src: Arc<str> = match image_data_url(&path) {
            Some(url) => url,
            None => Arc::from(file_url.as_str()),
        };
        let width = opts.width_percent.clamp(20, 100);
        let alt = alt_of(&whole);
        let img_tag = format!("<img src=\"{src}\" alt=\"{alt}\" style=\"width: {width}%\">");

        // 图片包一层指向自身的链接，点击即可放大预览
        format!("[{}]({})", img_tag, file_url)
    })
    .into_owned()
}

/// 只替换 `src` 中**最后一次**出现的 `from`
///
/// URL 位于 `](` 之后，而 alt 文本里可能出现同样的字符串
/// （例如 `![a.png](a.png)`），用 `replace` 会连 alt 一起改写。
fn replace_last(src: &str, from: &str, to: &str) -> String {
    match src.rfind(from) {
        Some(pos) => format!("{}{}{}", &src[..pos], to, &src[pos + from.len()..]),
        None => src.to_string(),
    }
}

/// 远程链接：当前不改写（桌面端未安装 HTTP 客户端，远程图由框架自行处理）
fn is_remote_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// 去掉 `file://` 前缀并做 percent 解码
fn strip_file_scheme(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    Some(decode_percent(rest))
}

/// 最小 percent-decoding（与 `encode_path` 对应）
fn decode_percent(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 单个十六进制字符的值
fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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

// ── 文档事实解析 ─────────────────────────────────────────────
//
// 设计见 `docs/markdown-facts-design.md`。要点：只提取"文档里有什么"
// （标题 / 链接 / wikilink / 标签）并带上行号，供大纲、双链、标签与索引共用；
// 不做渲染，因此不引入 Markdown 解析 crate；代码块与行内代码内的内容一律不算。

/// 一篇笔记里解析出的事实
#[derive(Debug, Default, Clone, PartialEq, Eq)]
// T6 索引 / T8 大纲 / T9 双链 / T11 标签会用到这批结构与 `parse_facts`。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct DocFacts {
    /// ATX 标题
    pub headings: Vec<Heading>,
    /// 行内链接（含图片链接）
    pub links: Vec<Link>,
    /// 双链 `[[...]]`
    pub wiki_links: Vec<WikiLink>,
    /// 标签（正文 `#tag` 与 frontmatter 的 `tags:`）
    pub tags: Vec<Tag>,
}

/// 标题
#[derive(Debug, Clone, PartialEq, Eq)]
// T6 索引 / T8 大纲 / T9 双链 / T11 标签会用到这批结构与 `parse_facts`。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct Heading {
    /// 1–6
    pub level: u8,
    /// 去掉 `#` 与闭合 `#` 后的文本
    pub text: String,
    /// 0-based 行号
    pub line: usize,
}

/// 行内链接
#[derive(Debug, Clone, PartialEq, Eq)]
// T6 索引 / T8 大纲 / T9 双链 / T11 标签会用到这批结构与 `parse_facts`。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct Link {
    pub target: String,
    pub text: String,
    pub line: usize,
}

/// 双链
#[derive(Debug, Clone, PartialEq, Eq)]
// T6 索引 / T8 大纲 / T9 双链 / T11 标签会用到这批结构与 `parse_facts`。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct WikiLink {
    /// 目标笔记名 / 路径
    pub target: String,
    /// `[[目标|别名]]` 里的别名
    pub alias: Option<String>,
    /// `[[目标#锚点]]` 里的锚点
    pub anchor: Option<String>,
    pub line: usize,
}

/// 标签（不含前导 `#`）
#[derive(Debug, Clone, PartialEq, Eq)]
// T6 索引 / T8 大纲 / T9 双链 / T11 标签会用到这批结构与 `parse_facts`。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub struct Tag {
    pub name: String,
    pub line: usize,
}

/// 解析一篇笔记的事实。
///
/// 逐行扫描，维护两个状态：围栏代码块（``` / ~~~）与 frontmatter。
/// 行内先抹掉反引号包裹的代码片段，再提取链接、双链与标签。
// T6 索引 / T8 大纲 / T9 双链 / T11 标签会用到这批结构与 `parse_facts`。
// 现阶段只有测试在用，故显式豁免死代码告警——不是"没用上"，是"还没接上"。
#[allow(dead_code)]
pub fn parse_facts(text: &str) -> DocFacts {
    let mut facts = DocFacts::default();
    let mut fence: Option<String> = None;
    let mut in_frontmatter = false;

    for (line, raw) in text.lines().enumerate() {
        // frontmatter：只在文件开头，由 `---` 包围
        if line == 0 {
            if raw.trim() == "---" {
                in_frontmatter = true;
                continue;
            }
        } else if in_frontmatter {
            if raw.trim() == "---" {
                in_frontmatter = false;
            } else {
                collect_frontmatter_tags(raw, line, &mut facts.tags);
            }
            continue;
        }

        // 围栏代码块：块内一律不算事实
        if let Some(marker) = fence_marker(raw) {
            match &fence {
                Some(open) if open == &marker => fence = None,
                Some(_) => {}
                None => fence = Some(marker),
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }

        // 标题用原文（标题不可能在行内代码里），其余用抹掉行内代码的副本
        if let Some(heading) = parse_heading(raw, line) {
            facts.headings.push(heading);
        }
        let masked = mask_inline_code(raw);
        collect_links(&masked, line, &mut facts.links);
        collect_wiki_links(&masked, line, &mut facts.wiki_links);
        collect_tags(&masked, line, &mut facts.tags);
    }

    facts
}

/// 行是否开启 / 闭合围栏代码块，返回三字符标记（``` 或 ~~~）
fn fence_marker(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let head: String = trimmed.chars().take(3).collect();
    match head.as_str() {
        "```" | "~~~" => Some(head),
        _ => None,
    }
}

/// 解析 ATX 标题：`#` 后必须有空格，尾部闭合的 `#` 会被去掉
fn parse_heading(line: &str, line_no: usize) -> Option<Heading> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    // `#` 是单字节，按字节切分安全
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        // `#tag` 之类：不是标题（可能是标签）
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim();
    if text.is_empty() {
        return None;
    }
    Some(Heading {
        level: level as u8,
        text: text.to_string(),
        line: line_no,
    })
}

/// 把行内代码（反引号包裹）替换成等长空格，后续扫描就不会误判其中的内容
fn mask_inline_code(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = chars.clone();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '`' {
            i += 1;
            continue;
        }
        let run = run_length(&chars, i, '`');
        // 找同样长度的闭合片段
        let mut j = i + run;
        let mut closed = false;
        while j < chars.len() {
            if chars[j] == '`' {
                let run2 = run_length(&chars, j, '`');
                if run2 == run {
                    for c in out.iter_mut().take(j + run2).skip(i) {
                        *c = ' ';
                    }
                    i = j + run2;
                    closed = true;
                    break;
                }
                j += run2;
            } else {
                j += 1;
            }
        }
        if !closed {
            // 未闭合：本行剩余部分都当作代码
            for c in out.iter_mut().skip(i) {
                *c = ' ';
            }
            break;
        }
    }

    out.into_iter().collect()
}

/// 从 `from` 起连续相同字符的个数
fn run_length(chars: &[char], from: usize, c: char) -> usize {
    let mut n = 0;
    while from + n < chars.len() && chars[from + n] == c {
        n += 1;
    }
    n
}

/// 提取 `[文本](目标)` 与 `![文本](目标)`
fn collect_links(line: &str, line_no: usize, out: &mut Vec<Link>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let lb = if bytes[i] == b'[' {
            Some(i)
        } else if bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            Some(i + 1)
        } else {
            None
        };

        let Some(lb) = lb else {
            i += 1;
            continue;
        };
        let Some(rb) = find_byte(bytes, lb + 1, b']') else {
            i += 1;
            continue;
        };
        if rb + 1 >= bytes.len() || bytes[rb + 1] != b'(' {
            i += 1;
            continue;
        }
        let Some(rp) = find_byte(bytes, rb + 2, b')') else {
            i += 1;
            continue;
        };

        let text = line[lb + 1..rb].trim().to_string();
        // 目标后面可能带标题：`[a](url "title")` → 取第一段
        let target = line[rb + 2..rp]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_matches(|c| c == '<' || c == '>')
            .to_string();
        if !target.is_empty() {
            out.push(Link {
                target,
                text,
                line: line_no,
            });
        }
        i = rp + 1;
    }
}

/// 提取 `[[目标]]` / `[[目标|别名]]` / `[[目标#锚点]]`
fn collect_wiki_links(line: &str, line_no: usize, out: &mut Vec<WikiLink>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != b'[' || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let Some(end) = line.get(i + 2..).and_then(|rest| rest.find("]]")) else {
            break;
        };
        let end = end + i + 2;
        if let Some(link) = parse_wiki_inner(&line[i + 2..end], line_no) {
            out.push(link);
        }
        i = end + 2;
    }
}

/// 解析双链内部：`目标` / `目标|别名` / `目标#锚点` / `目标#锚点|别名`
fn parse_wiki_inner(inner: &str, line_no: usize) -> Option<WikiLink> {
    let (left, alias) = match inner.split_once('|') {
        Some((left, alias)) => (left, Some(alias.trim().to_string())),
        None => (inner, None),
    };
    let alias = alias.filter(|text| !text.is_empty());

    let (target, anchor) = match left.split_once('#') {
        Some((target, anchor)) => (target, Some(anchor.trim().to_string())),
        None => (left, None),
    };
    let anchor = anchor.filter(|text| !text.is_empty());

    let target = target.trim().to_string();
    if target.is_empty() {
        return None;
    }
    Some(WikiLink {
        target,
        alias,
        anchor,
        line: line_no,
    })
}

/// 提取正文里的 `#tag`
fn collect_tags(line: &str, line_no: usize, out: &mut Vec<Tag>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '#' || !tag_can_start_at(&chars, i) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < chars.len() && is_tag_char(chars[end]) {
            end += 1;
        }
        // 纯数字不是标签（避免把序号、编号当标签）；含中文/字母的照常收
        let all_digits = chars[start..end].iter().all(|c| c.is_ascii_digit());
        if end > start && !all_digits {
            out.push(Tag {
                name: chars[start..end].iter().collect(),
                line: line_no,
            });
        }
        i = end;
    }
}

/// `#` 前必须是行首或"分隔性"字符：否则 `a.md#top`、`#fff` 这类会被误判
fn tag_can_start_at(chars: &[char], i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).map(|ix| chars[ix]) else {
        return true;
    };
    if prev.is_whitespace() {
        return true;
    }
    matches!(
        prev,
        '(' | '[' | '{' | '"' | '\'' | '（' | '“' | '，' | '：' | '、'
    )
}

/// 标签可用字符：字母数字、下划线、连字符、斜杠（嵌套标签）与非 ASCII（中文）
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/') || !c.is_ascii()
}

/// 提取 frontmatter 里的 `tags: [a, b]` / `tags: a, b`
fn collect_frontmatter_tags(line: &str, line_no: usize, out: &mut Vec<Tag>) {
    let Some(rest) = line
        .trim_start()
        .strip_prefix("tags:")
        .or_else(|| line.trim_start().strip_prefix("tag:"))
    else {
        return;
    };
    let rest = rest.trim().trim_matches(|c| c == '[' || c == ']');
    for part in rest.split(',') {
        let name = part
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '#');
        if !name.is_empty() {
            out.push(Tag {
                name: name.to_string(),
                line: line_no,
            });
        }
    }
}

/// 把 `[[目标]]` 改写成预览可点的 Markdown 链接。
///
/// `resolve` 负责把目标文本解析成笔记路径（由索引提供）；解析不到就**原样保留**，
/// 免得点开一个并不存在的文件。
///
/// 显示文本：`[[目标|别名]]` 用别名，否则用目标本身。
/// 与 [`parse_facts`] 同一口径：围栏代码块内与行内代码内一律不动。
pub fn rewrite_wikilinks(md: &str, resolve: impl Fn(&str) -> Option<PathBuf>) -> String {
    let mut out = String::with_capacity(md.len());
    let mut fence: Option<String> = None;

    for raw in md.split_inclusive('\n') {
        let (line, newline) = match raw.strip_suffix('\n') {
            Some(line) => (line, true),
            None => (raw, false),
        };

        if let Some(marker) = fence_marker(line) {
            match &fence {
                Some(open) if open == &marker => fence = None,
                Some(_) => {}
                None => fence = Some(marker),
            }
            out.push_str(raw);
            continue;
        }
        if fence.is_some() {
            out.push_str(raw);
            continue;
        }

        out.push_str(&rewrite_line(line, &resolve));
        if newline {
            out.push('\n');
        }
    }
    out
}

/// 改写单行里的双链。
///
/// 在"抹掉行内代码"的副本上定位，再按同样的字符下标取原文替换：
/// 代码里的 `[[..]]` 既不会被匹配，也不会破坏原文长度。
fn rewrite_line(line: &str, resolve: &impl Fn(&str) -> Option<PathBuf>) -> String {
    // 全程按**字符**下标工作：路径含中文时字节下标会错位
    let masked: Vec<char> = mask_inline_code(line).chars().collect();
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;

    while i < chars.len() {
        // 判定也在 masked 上做：位于行内代码里的 `[[` 已被抹成空格，
        // 否则会拿它去匹配后面（代码外）的 `]]`，把整段吞掉
        let is_open = masked[i] == '[' && masked.get(i + 1) == Some(&'[');
        let end = is_open
            .then(|| find_wiki_close(&masked, i + 2))
            .flatten()
            .map(|close| close + 2);

        let Some(end) = end else {
            out.push(chars[i]);
            i += 1;
            continue;
        };

        let inner: String = chars[i + 2..end - 2].iter().collect();
        let replacement = parse_wiki_inner(&inner, 0)
            .and_then(|link| resolve(&link.target))
            .map(|path| {
                let label = parse_wiki_inner(&inner, 0)
                    .and_then(|link| link.alias)
                    .unwrap_or_else(|| default_wiki_label(&inner));
                format!("[{}](file://{})", label, encode_path(&path))
            });

        match replacement {
            Some(text) => out.push_str(&text),
            // 解析不到：原样保留，不做任何"看起来能点"的暗示
            None => out.extend(chars[i..end].iter()),
        }
        i = end;
    }
    out
}

/// 从 `from` 起找双链的闭合 `]]`，返回第一个 `]` 的字符下标
fn find_wiki_close(chars: &[char], from: usize) -> Option<usize> {
    (from..chars.len().saturating_sub(1)).find(|&ix| chars[ix] == ']' && chars[ix + 1] == ']')
}

/// 双链的显示文本：没有别名时用目标部分（去掉 `#锚点`）
fn default_wiki_label(inner: &str) -> String {
    let head = inner.split('|').next().unwrap_or(inner);
    head.split('#').next().unwrap_or(head).trim().to_string()
}

fn find_byte(bytes: &[u8], from: usize, target: u8) -> Option<usize> {
    (from..bytes.len()).find(|&ix| bytes[ix] == target)
}

// ── 编辑区装饰 ────────────────────────────────────────────────
//
// 编辑区的 Markdown 是"源码全可见 + 加视觉样式"（不做 Obsidian 那种隐藏标记），
// 所以装饰只负责把行首标记弱化、把链接标出来，让结构一眼可辨。

/// 编辑区装饰的片段类型
///
/// 只描述"哪段字节该是什么语义"，**不含具体颜色**：颜色由 UI 层按主题决定，
/// 领域层不引用 gpui 的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// ATX 标题的 `#` 标记：淡化
    HeadingMarker,
    /// 列表符号（`-` / `*` / `+` / `1.` / `1)`）：淡化
    ListMarker,
    /// 块引用 `>`：上色
    QuoteMarker,
    /// `[文字](url)` 里的"文字"：下划线
    LinkText,
}

/// 一个待装饰的 **UTF-8 字节**范围
///
/// 用字节而非字符偏移：编辑控件的装饰接口就是字节范围，避免来回换算。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecorationSpan {
    pub range: Range<usize>,
    pub kind: SpanKind,
}

/// 扫描 Markdown，产出编辑区装饰所需的字节范围
///
/// 围栏 / frontmatter 的判据与 [`parse_facts`] 共用（见 [`fence_marker`]），
/// 但这里**只做定位**、不提取事实——消费者不同（装饰 vs 索引），分开更好测。
///
/// 围栏代码块与 frontmatter 内部一律不产出装饰：那里是代码 / YAML，
/// 给其中的 `#` 加淡化只会干扰阅读。
pub fn decoration_spans(text: &str) -> Vec<DecorationSpan> {
    let mut spans = Vec::new();
    let mut fence: Option<String> = None;
    let mut in_frontmatter = false;
    // split_inclusive 保留换行，才能把行内偏移换算成整篇的字节偏移
    let mut offset = 0;

    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        let base = offset;
        offset += raw.len();

        // frontmatter：只在文件开头，由 `---` 包围
        if index == 0 {
            if line.trim() == "---" {
                in_frontmatter = true;
                continue;
            }
        } else if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
            }
            continue;
        }

        if let Some(marker) = fence_marker(line) {
            match &fence {
                Some(open) if open == &marker => fence = None,
                Some(_) => {}
                None => fence = Some(marker),
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }

        collect_marker_spans(line, base, &mut spans);
        collect_link_spans(line, base, &mut spans);
    }

    spans
}

/// 行首的块级标记：`>`（可嵌套）、`#`、列表符号
fn collect_marker_spans(line: &str, base: usize, out: &mut Vec<DecorationSpan>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }

    // 块引用可嵌套（`> >`），每个 `>` 单独上色
    while i < bytes.len() && bytes[i] == b'>' {
        out.push(DecorationSpan {
            range: base + i..base + i + 1,
            kind: SpanKind::QuoteMarker,
        });
        i += 1;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
    }

    // 以 `#` 开头的行不会同时是列表，判完就可以收工
    if bytes.get(i) == Some(&b'#') {
        let start = i;
        while i < bytes.len() && bytes[i] == b'#' && i - start < 6 {
            i += 1;
        }
        // `#` 后必须是空格或行尾，`#hashtag` 不是标题
        if matches!(bytes.get(i), None | Some(b' ') | Some(b'\t')) {
            out.push(DecorationSpan {
                range: base + start..base + i,
                kind: SpanKind::HeadingMarker,
            });
        }
        return;
    }

    let start = i;
    if matches!(bytes.get(i), Some(b'-') | Some(b'*') | Some(b'+')) {
        // 符号后要有空格；否则可能是 `*斜体*` 或分隔线
        if matches!(bytes.get(i + 1), Some(b' ') | Some(b'\t')) {
            out.push(DecorationSpan {
                range: base + start..base + start + 1,
                kind: SpanKind::ListMarker,
            });
        }
        return;
    }

    if bytes.get(i).is_some_and(|b| b.is_ascii_digit()) {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if matches!(bytes.get(i), Some(b'.') | Some(b')'))
            && matches!(bytes.get(i + 1), Some(b' ') | Some(b'\t'))
        {
            out.push(DecorationSpan {
                range: base + start..base + i + 1,
                kind: SpanKind::ListMarker,
            });
        }
    }
}

/// 行内链接 `[文字](url)`：只装饰"文字"，方括号与 URL 保持原样
fn collect_link_spans(line: &str, base: usize, out: &mut Vec<DecorationSpan>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        // 必须找到 `](` 这对字符，避免把 `[a] [b]` 误判成链接
        let Some(close) = find_byte(bytes, i + 1, b']') else {
            break;
        };
        if bytes.get(close + 1) != Some(&b'(') {
            i += 1;
            continue;
        }
        let Some(url_end) = find_byte(bytes, close + 2, b')') else {
            break;
        };
        // `[](url)` 没有文字，零长度范围没有意义
        if close > i + 1 {
            out.push(DecorationSpan {
                range: base + i + 1..base + close,
                kind: SpanKind::LinkText,
            });
        }
        i = url_end + 1;
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/markdown.rs"]
mod tests;

// ── 引用其他文件 ─────────────────────────────────────────────

/// 计算从 `base_dir`（当前笔记所在目录）到 `target` 的相对路径。
///
/// 结果用 `/` 连接，便于直接写进 Markdown 链接；两级相等时返回目标文件名。
pub fn relative_path(base_dir: &Path, target: &Path) -> String {
    let base: Vec<_> = base_dir.components().collect();
    let tgt: Vec<_> = target.components().collect();

    let mut common = 0;
    while common < base.len() && common < tgt.len() && base[common] == tgt[common] {
        common += 1;
    }

    let mut parts: Vec<String> = vec!["..".to_string(); base.len() - common];
    for c in &tgt[common..] {
        parts.push(c.as_os_str().to_string_lossy().to_string());
    }

    if parts.is_empty() {
        return target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    parts.join("/")
}

/// 生成 Markdown 链接：图片用 `![]()`，其余用 `[]()`。
///
/// 路径含空格或括号时用尖括号包裹，否则 Markdown 会把链接截断。
pub fn markdown_link(name: &str, rel: &str, is_image: bool) -> String {
    let path = if rel.contains(' ') || rel.contains('(') || rel.contains(')') {
        format!("<{rel}>")
    } else {
        rel.to_string()
    };
    if is_image {
        format!("![{name}]({path})")
    } else {
        format!("[{name}]({path})")
    }
}
