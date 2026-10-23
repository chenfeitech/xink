//! Markdown 代码块语法高亮（基于 syntect）
//!
//! gpui-base 的 TextView 不包含高亮器，需由应用通过 `code_block_highlighter` 提供。
//! 这里把 syntect 的高亮结果转成「字节范围 → 样式」，交给 TextView 着色。
//!
//! **配色与编辑区同源**：统一预置主题（`core::theme`），不再用 syntect 内置主题集。
//! 两边体系不同——syntect 认 TextMate scope，预置主题用语义名（`keyword`、`string`…），
//! 中间的映射见 [`SCOPE_MAP`]。统一方案见
//! `docs/editor/unified-highlight-config-design.md`。
//!
//! 高亮结果必须缓存：TextView 每次渲染都会回调高亮器（滚动、切换标签都会触发），
//! 不缓存的话 syntect 会被反复调用，是界面卡顿的主要来源之一。

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};

use gpui_kit::Hsla;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::core::theme;

/// 代码块高亮片段的样式
///
/// 以前只带颜色（`Hsla`），粗体 / 斜体 / 下划线被丢掉了；现在把 syntect 的
/// `font_style` 位一并带出去，由 UI 层转成 `gpui::HighlightStyle`。
///
/// `color` 为 `None` 表示"主题没给这个 token 配色，但有字重 / 斜体"，
/// UI 层应保持正文色、只应用样式位。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanStyle {
    /// 前景色；`None` = 沿用正文色
    pub color: Option<Hsla>,
    /// 粗体（主题里 `font_weight ≥ 600` 或 `emphasis.strong`）
    pub bold: bool,
    /// 斜体
    pub italic: bool,
    /// 下划线
    pub underline: bool,
}

type Span = (Range<usize>, SpanStyle);

/// (语言, 代码哈希, 主题名, 明暗)。用哈希而非整个代码块，避免缓存撑大内存。
///
/// 主题名是 `&'static str`（`normalize_theme_name` 的返回值），明暗单独成位，
/// 因此构造 key 不再需要 `format!` 出一个字符串——这个回调每帧对每个可见
/// 代码块都会跑一次。
type CacheKey = (String, u64, &'static str, bool);
/// 高亮结果共享：命中缓存时只克隆一次 `Arc`，不搬整个 `Vec<Span>`
type CachedSpans = Arc<Vec<Span>>;

/// SyntaxSet 构建开销大，全局共享一份
static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();
static CACHE: OnceLock<Mutex<HashMap<CacheKey, CachedSpans>>> = OnceLock::new();
/// 由预置主题构造出的 syntect 主题；按 (主题名, 明暗) 缓存，避免每次渲染都解析 JSON
type ThemeCache = Mutex<HashMap<(String, bool), Arc<Theme>>>;

static THEMES: OnceLock<ThemeCache> = OnceLock::new();

/// 缓存上限，超出直接清空（笔记里的代码块数量有限，简单淘汰足够）
const MAX_CACHE_ENTRIES: usize = 256;

/// 统一主题的语义名 → syntect 的 TextMate scope 选择器
///
/// 这是一张**人工维护**的映射表（设计文档 §9 风险 1）：写错的表现是"预览区某一类
/// 不着色"，既不会编译报错也不会运行报错，只能靠人工验收发现。
/// 多个选择器用逗号分隔，任一命中即应用；syntect 按命中长度打分，更具体的优先。
/// 表里没有的语义名（`hint` / `predictive` / `primary` 等）退化为"用名字本身当
/// scope"，匹配不到就不着色。
const SCOPE_MAP: &[(&str, &str)] = &[
    ("attribute", "entity.other.attribute-name"),
    ("boolean", "constant.language.boolean"),
    ("comment", "comment"),
    (
        "comment.doc",
        "comment.line.documentation, comment.block.documentation",
    ),
    ("constant", "constant"),
    ("constructor", "entity.name.function.constructor"),
    ("embedded", "meta.embedded"),
    ("emphasis", "markup.italic"),
    ("emphasis.strong", "markup.bold"),
    (
        "enum",
        "entity.name.enum, entity.name.type.enum, support.type.enum",
    ),
    ("function", "entity.name.function, support.function"),
    // `storage.type.function` 是 Sublime 系语法给 Rust `fn` 的 scope，
    // tree-sitter 那边把它当关键字；为了让两端一致，显式归到 keyword。
    // 它比 `type` 的 `storage.type` 多一段，syntect 打分时优先命中这里。
    ("keyword", "keyword, storage.type.function"),
    ("label", "entity.name.label"),
    ("link_text", "string.other.link, markup.underline.link"),
    ("link_uri", "markup.underline.link"),
    ("number", "constant.numeric"),
    ("operator", "keyword.operator"),
    ("preproc", "meta.preprocessor"),
    (
        "property",
        "support.type.property-name, variable.other.property, meta.object-literal.key",
    ),
    ("punctuation", "punctuation"),
    ("punctuation.bracket", "punctuation.section, meta.brace"),
    (
        "punctuation.delimiter",
        "punctuation.separator, punctuation.terminator",
    ),
    ("punctuation.list_marker", "punctuation.definition.list"),
    ("punctuation.special", "punctuation.special"),
    ("string", "string"),
    ("string.escape", "constant.character.escape"),
    ("string.regex", "string.regexp"),
    ("string.special", "string.special"),
    ("string.special.symbol", "constant.other.symbol"),
    ("tag", "entity.name.tag"),
    (
        "tag.doctype",
        "entity.name.tag.doctype, meta.tag.sgml.doctype",
    ),
    ("text.code.span", "markup.inline.raw"),
    ("text.literal", "markup.raw.block, markup.fenced_code.block"),
    ("title", "entity.name.section, entity.name"),
    ("type", "entity.name.type, support.type, storage.type"),
    ("variable", "variable"),
    ("variable.special", "variable.language, variable.special"),
    ("variant", "entity.name.variant, entity.name.type.variant"),
];

pub struct Highlighter {
    syntax_set: SyntaxSet,
}

impl Highlighter {
    fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
        }
    }

    /// 按语言名或扩展名找语法；都找不到就退回纯文本
    fn syntax(&self, lang: &str) -> &SyntaxReference {
        self.syntax_set
            .find_syntax_by_token(lang)
            .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }
}

pub fn highlighter() -> &'static Highlighter {
    HIGHLIGHTER.get_or_init(Highlighter::new)
}

fn cache() -> &'static Mutex<HashMap<CacheKey, CachedSpans>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hash_code(code: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

/// 预置主题名 + 明暗 → 缓存 key（换主题 / 换明暗都必须失效重算）
fn theme_key(name: &str, dark: bool) -> String {
    format!("{name}.{}", if dark { "dark" } else { "light" })
}

/// 取出（必要时构造）统一主题对应的 syntect 主题
///
/// 主题名未知名会被 [`theme::normalize_theme_name`] 校正；JSON 缺失或解析失败时
/// 退化为"没有任何配色"，预览区代码块只按正文色显示——绝不在渲染路径上 panic。
pub fn syntect_theme(name: &str, dark: bool) -> Arc<Theme> {
    let name = theme::normalize_theme_name(name);
    let key = (name.to_string(), dark);

    let themes = THEMES.get_or_init(Default::default);
    if let Ok(cache) = themes.lock()
        && let Some(hit) = cache.get(&key)
    {
        return hit.clone();
    }

    let built = Arc::new(build_theme(name, dark));

    if let Ok(mut cache) = themes.lock() {
        cache.insert(key, built.clone());
    }

    built
}

/// 把预置主题 JSON 的 `style.syntax` 段翻译成 syntect 的 `Theme`
fn build_theme(name: &str, dark: bool) -> Theme {
    let mut scopes: Vec<ThemeItem> = Vec::new();

    if let Some(json) = theme::preset_json(name, dark)
        && let Ok(parsed) = serde_json::from_str::<PresetFile>(json)
    {
        for (semantic, style) in &parsed.style.syntax {
            let selectors = scopes_for(semantic);
            let Ok(parsed_selectors) = ScopeSelectors::from_str(selectors) else {
                continue;
            };
            scopes.push(ThemeItem {
                scope: parsed_selectors,
                style: to_modifier(style),
            });
        }
    }

    Theme {
        name: Some(theme_key(name, dark)),
        author: None,
        // 关键点：`settings.foreground` 是"没匹配到任何规则的 token"的兜底色，
        // syntect 在它为 None 时会用**黑色**（深色主题下等于看不见）。这里放一个
        // 全透明色，让 `to_hsla` 过滤掉这些片段，交给 TextView 用正文色渲染。
        settings: ThemeSettings {
            foreground: Some(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            }),
            ..Default::default()
        },
        scopes,
    }
}

/// 语义名 → scope 选择器串；表里没有就用名字本身（多数语义名本身就是合法 scope）
fn scopes_for(semantic: &str) -> &str {
    SCOPE_MAP
        .iter()
        .find(|(name, _)| *name == semantic)
        .map(|(_, selectors)| *selectors)
        .unwrap_or(semantic)
}

/// 主题里的一条样式 → syntect 的 `StyleModifier`
fn to_modifier(style: &PresetThemeStyle) -> StyleModifier {
    let mut font_style = FontStyle::empty();
    match style.font_style.as_deref() {
        Some("italic") => font_style |= FontStyle::ITALIC,
        Some("underline") => font_style |= FontStyle::UNDERLINE,
        _ => {}
    }
    // syntect 只有 BOLD 一个位，半粗及以上都算粗体
    if style.font_weight.unwrap_or(0) >= 600 {
        font_style |= FontStyle::BOLD;
    }

    StyleModifier {
        foreground: style.color.as_deref().and_then(parse_color),
        background: None,
        font_style: Some(font_style),
    }
}

/// `#RRGGBB` / `#RRGGBBAA` → syntect 颜色；格式不对返回 `None`（该条不着色）
fn parse_color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#')?;
    // gpui 的 Hsla 序列化为 `#RRGGBB`（不透明时省略 alpha）
    let (rgb, alpha) = match hex.len() {
        6 => (hex, 255u8),
        8 => (&hex[..6], u8::from_str_radix(&hex[6..8], 16).ok()?),
        _ => return None,
    };
    Some(Color {
        r: u8::from_str_radix(&rgb[0..2], 16).ok()?,
        g: u8::from_str_radix(&rgb[2..4], 16).ok()?,
        b: u8::from_str_radix(&rgb[4..6], 16).ok()?,
        a: alpha,
    })
}

/// 预置主题 JSON 的形状：只取 `style.syntax`，其余字段忽略
#[derive(serde::Deserialize)]
struct PresetFile {
    #[serde(default)]
    style: PresetStyle,
}

#[derive(Default, serde::Deserialize)]
struct PresetStyle {
    #[serde(default)]
    syntax: HashMap<String, PresetThemeStyle>,
}

#[derive(serde::Deserialize)]
struct PresetThemeStyle {
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    font_style: Option<String>,
    #[serde(default)]
    font_weight: Option<u16>,
}

/// 高亮一段代码，返回 (字节范围, 样式) 列表。
/// 范围相对于传入的 `code`，可直接交给 TextView。
///
/// `theme` 是**统一预置主题名**（见 [`theme::PRESET_THEMES`]），不是 syntect 主题名；
/// `dark` 决定取该主题的深色还是浅色变体。
pub fn highlight(lang: &str, code: &str, theme: &str, dark: bool) -> CachedSpans {
    let key: CacheKey = (
        lang.to_string(),
        hash_code(code),
        theme::normalize_theme_name(theme),
        dark,
    );

    if let Ok(cache) = cache().lock()
        && let Some(hit) = cache.get(&key)
    {
        return hit.clone();
    }

    let spans: CachedSpans = Arc::new(compute(lang, code, theme, dark));

    if let Ok(mut cache) = cache().lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, spans.clone());
    }

    spans
}

fn compute(lang: &str, code: &str, theme_name: &str, dark: bool) -> Vec<Span> {
    let h = highlighter();
    let syntax = h.syntax(lang);
    let theme = syntect_theme(theme_name, dark);
    let mut highlighter = HighlightLines::new(syntax, &theme);

    let mut out = Vec::new();
    let mut offset = 0usize;

    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, &h.syntax_set) {
            Ok(ranges) => {
                for (style, text) in ranges {
                    let start = offset;
                    offset += text.len();
                    // 透明色 = 主题没给这个 token 配色（见 `build_theme` 的兜底）
                    let color = to_hsla(style.foreground);
                    let span = SpanStyle {
                        color,
                        bold: style.font_style.contains(FontStyle::BOLD),
                        italic: style.font_style.contains(FontStyle::ITALIC),
                        underline: style.font_style.contains(FontStyle::UNDERLINE),
                    };
                    // 既没颜色也没样式位的片段不用产出，交给 TextView 的正文样式
                    if color.is_some() || span.bold || span.italic || span.underline {
                        out.push((start..offset, span));
                    }
                }
            }
            Err(_) => offset += line.len(),
        }
    }

    out
}

fn to_hsla(c: syntect::highlighting::Color) -> Option<Hsla> {
    if c.a == 0 {
        return None;
    }
    let hex = ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32;
    Some(gpui_kit::rgb(hex).into())
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../tests/core/highlight.rs"]
mod tests;
