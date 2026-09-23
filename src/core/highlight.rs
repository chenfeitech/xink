//! Markdown 代码块语法高亮（基于 syntect）
//!
//! gpui-base 的 TextView 不包含高亮器，需由应用通过 `code_block_highlighter` 提供。
//! 这里把 syntect 的高亮结果转成「字节范围 → 颜色」，交给 TextView 着色。
//!
//! 高亮结果必须缓存：TextView 每次渲染都会回调高亮器（滚动、切换标签都会触发），
//! 不缓存的话 syntect 会被反复调用，是界面卡顿的主要来源之一。

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Mutex, OnceLock};

use gpui_kit::Hsla;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

type Span = (Range<usize>, Hsla);

/// (语言, 代码哈希, 主题名)。用哈希而非整个代码块，避免缓存撑大内存。
type CacheKey = (String, u64, String);

/// SyntaxSet 构建开销大，全局共享一份
static HIGHLIGHTER: OnceLock<Highlighter> = OnceLock::new();
static CACHE: OnceLock<Mutex<HashMap<CacheKey, Vec<Span>>>> = OnceLock::new();

/// 缓存上限，超出直接清空（笔记里的代码块数量有限，简单淘汰足够）
const MAX_CACHE_ENTRIES: usize = 256;

pub struct Highlighter {
    syntax_set: SyntaxSet,
    /// 完整主题集，供按名选择；只保留引用所需的两个默认主题避免重复查找
    theme_set: ThemeSet,
    /// 主题名缺失时的兜底主题
    dark: Theme,
}

/// 默认深色 / 浅色主题名。配置里填了不存在的名字时退回这两个。
pub const DEFAULT_DARK_THEME: &str = "base16-ocean.dark";
pub const DEFAULT_LIGHT_THEME: &str = "base16-ocean.light";

impl Highlighter {
    fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        Self {
            // 用 get 而不是下标：主题缺失时退回默认值，绝不在渲染路径上 panic
            dark: theme_set
                .themes
                .get(DEFAULT_DARK_THEME)
                .cloned()
                .unwrap_or_default(),
            syntax_set,
            theme_set,
        }
    }

    /// 按名字取主题；名字不存在时退回深色默认主题
    fn theme(&self, name: &str) -> &Theme {
        self.theme_set.themes.get(name).unwrap_or(&self.dark)
    }

    /// 按语言名或扩展名找语法；都找不到就退回纯文本
    fn syntax(&self, lang: &str) -> &SyntaxReference {
        self.syntax_set
            .find_syntax_by_token(lang)
            .or_else(|| self.syntax_set.find_syntax_by_extension(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
    }
}

/// 按明暗返回默认主题名
pub fn default_theme(is_dark: bool) -> &'static str {
    if is_dark {
        DEFAULT_DARK_THEME
    } else {
        DEFAULT_LIGHT_THEME
    }
}

/// 可用的代码高亮主题名（供设置界面选择）
pub fn theme_names() -> Vec<String> {
    let mut names: Vec<String> = highlighter().theme_set.themes.keys().cloned().collect();
    names.sort();
    names
}

pub fn highlighter() -> &'static Highlighter {
    HIGHLIGHTER.get_or_init(Highlighter::new)
}

fn cache() -> &'static Mutex<HashMap<CacheKey, Vec<Span>>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hash_code(code: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

/// 高亮一段代码，返回 (字节范围, 颜色) 列表。
/// 范围相对于传入的 `code`，可直接交给 TextView。
/// 按指定主题名高亮；主题名不存在时退回默认深色主题。
pub fn highlight_with_theme(lang: &str, code: &str, theme: &str) -> Vec<Span> {
    let key: CacheKey = (lang.to_string(), hash_code(code), theme.to_string());

    if let Ok(cache) = cache().lock()
        && let Some(hit) = cache.get(&key)
    {
        return hit.clone();
    }

    let spans = compute(lang, code, theme);

    if let Ok(mut cache) = cache().lock() {
        if cache.len() >= MAX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(key, spans.clone());
    }

    spans
}

fn compute(lang: &str, code: &str, theme_name: &str) -> Vec<Span> {
    let h = highlighter();
    let syntax = h.syntax(lang);
    let theme = h.theme(theme_name);
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut out = Vec::new();
    let mut offset = 0usize;

    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, &h.syntax_set) {
            Ok(ranges) => {
                for (style, text) in ranges {
                    let start = offset;
                    offset += text.len();
                    if let Some(color) = to_hsla(style.foreground) {
                        out.push((start..offset, color));
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

#[cfg(test)]
mod tests;
