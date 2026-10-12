//! Markdown 预处理
//!
//! gpui-base 出于安全考虑，不会把 scheme-less 的字符串隐式解析为本地文件
//! （避免文档通过相对路径偷偷读取磁盘）。因此笔记里的相对图片路径
//! 必须由应用自己补全成可加载的 URL。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
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

/// 图片转 data URL 的缓存：避免每次刷新预览都重新读盘与编码。
///
/// 键是图片绝对路径，值是 `(修改时间, 文件长度, data URL)`；
/// 用 mtime + 长度作为内容指纹，足够可靠且比全文哈希便宜。
static IMAGE_DATA_URLS: OnceLock<Mutex<HashMap<PathBuf, (SystemTime, u64, String)>>> =
    OnceLock::new();

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
fn image_data_url(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mtime = meta.modified().ok()?;
    let key = path.to_path_buf();

    // 命中缓存且指纹未变则直接复用
    {
        let cache = IMAGE_DATA_URLS.get_or_init(Default::default).lock().ok()?;
        if let Some((cached_mtime, cached_len, url)) = cache.get(&key) {
            if *cached_mtime == mtime && *cached_len == meta.len() {
                return Some(url.clone());
            }
        }
    }

    let bytes = std::fs::read(path).ok()?;
    let url = format!("data:{};base64,{}", mime_for(path)?, encode_base64(&bytes));

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

        // 读不到 / 格式不支持 / 超限时退回 file://，至少链接指向正确位置
        let src = image_data_url(&path).unwrap_or_else(|| file_url.clone());
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
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
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

#[cfg(test)]
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
