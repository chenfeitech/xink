use super::*;
use crate::core::theme;
use std::str::FromStr;
use syntect::highlighting::ScopeSelector;

const THEME: &str = "xink";

#[test]
fn highlights_known_language() {
    let spans = highlight("rust", "fn main() { let x = 1; }", THEME, true);
    assert!(!spans.is_empty(), "Rust 代码应产出高亮片段");

    let code = "fn main() { let x = 1; }";
    for (range, _) in spans.iter() {
        assert!(range.start <= range.end);
        assert!(range.end <= code.len(), "范围越界: {range:?}");
    }
}

#[test]
fn cache_returns_same_result() {
    let code = "fn main() {}";
    let a = highlight("rust", code, THEME, true);
    let b = highlight("rust", code, THEME, true);
    assert_eq!(a.len(), b.len(), "缓存命中应返回相同结果");
}

#[test]
fn unknown_language_falls_back_without_panic() {
    let spans = highlight("not-a-real-language", "plain text here", THEME, false);
    let _ = spans;
}

#[test]
fn dark_and_light_theme_both_work() {
    assert!(!highlight("json", "{\"a\": 1}", THEME, true).is_empty());
    assert!(!highlight("json", "{\"a\": 1}", THEME, false).is_empty());
}

#[test]
fn unknown_theme_falls_back_instead_of_panicking() {
    // 配置里可能填了不存在的主题名，必须校正成默认而不是在渲染路径上 panic
    let spans = highlight("rust", "fn main() {}", "no-such-theme", true);
    assert!(!spans.is_empty(), "未知主题应退回默认主题");
}

// ── 统一配色源 ────────────────────────────────────────────────

#[test]
fn syntect_theme_is_built_from_the_preset() {
    // 这条直接锁死"两端同源"：预览区的 keyword 颜色必须等于预置主题里的 keyword 颜色
    let theme = syntect_theme(THEME, true);
    let expected = preset_color(THEME, true, "keyword").expect("预置主题应有 keyword 配色");

    let wanted = ScopeSelector::from_str("keyword").unwrap();
    let item = theme
        .scopes
        .iter()
        .find(|item| item.scope.selectors.contains(&wanted))
        .expect("keyword 应被映射成一条 syntect 规则");

    let actual = item.style.foreground.expect("keyword 规则应有前景色");
    assert_eq!(
        (actual.r, actual.g, actual.b),
        expected,
        "预览区 keyword 颜色应与预置主题一致"
    );
}

#[test]
fn rust_keyword_gets_the_preset_keyword_color() {
    // 端到端验证映射表真的生效：Rust 的 `fn` 应拿到预置主题里 keyword 的颜色
    let (r, g, b) = preset_color(THEME, true, "keyword").expect("预置主题应有 keyword");
    let expected: Hsla = gpui_kit::rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into();

    let spans = highlight("rust", "fn main() {}", THEME, true);
    let hit = spans.iter().find(|(range, _)| range.start == 0);
    assert_eq!(
        hit.map(|(_, style)| style.color),
        Some(Some(expected)),
        "`fn` 应着 keyword 色，实际片段: {spans:?}"
    );
}

#[test]
fn dark_and_light_variants_differ() {
    // 明暗两个变体必须有各自的颜色，否则"切换明暗两端同时变"无从谈起
    let dark = preset_color(THEME, true, "string");
    let light = preset_color(THEME, false, "string");
    assert!(
        dark.is_some() && light.is_some(),
        "两个变体都应有 string 配色"
    );
    assert_ne!(dark, light, "明暗变体的 string 颜色不应相同");
}

#[test]
fn unstyled_tokens_are_dropped_instead_of_turning_black() {
    // syntect 在 `settings.foreground` 为空时会用黑色兜底，深色主题下等于看不见。
    // 这里验证兜底色是透明的：没配色的 token 不产出片段（交给 TextView 的正文色）。
    let theme = syntect_theme(THEME, true);
    assert_eq!(
        theme.settings.foreground.map(|c| c.a),
        Some(0),
        "未匹配 token 的兜底色必须是全透明"
    );
}

#[test]
fn common_tokens_are_colored() {
    // 映射表回归护栏：注释 / 字符串 / 数字 / 关键字这几类最常见，
    // 映射写错时它们会先"不着色"，所以逐个钉住颜色
    let code = "// 注释\nlet s = \"hi\";\nlet n = 42;\nreturn n;\n";
    let spans = highlight("rust", code, THEME, true);

    let expected = |semantic: &str| -> Hsla {
        let (r, g, b) = preset_color(THEME, true, semantic).unwrap();
        gpui_kit::rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
    };
    let color_of = |needle: &str| -> Option<Hsla> {
        let at = code.find(needle)?;
        spans
            .iter()
            .find(|(range, _)| range.start <= at && at < range.end)
            .and_then(|(_, style)| style.color)
    };

    assert_eq!(
        color_of("注释"),
        Some(expected("comment")),
        "注释应着 comment 色"
    );
    assert_eq!(
        color_of("hi"),
        Some(expected("string")),
        "字符串应着 string 色"
    );
    assert_eq!(
        color_of("42"),
        Some(expected("number")),
        "数字应着 number 色"
    );
    // 不用 `let`：Rust 的 Sublime 语法把 `let` 和 `u32` 都标成 `storage.type`，
    // 这个 scope 只能整类映射，详见 `SCOPE_MAP` 的注释
    assert_eq!(
        color_of("return"),
        Some(expected("keyword")),
        "关键字应着 keyword 色"
    );
}

#[test]
fn every_preset_colors_common_tokens() {
    // 每套预置主题都要能真的着色：主题里缺条目不会报错，只会静默不着色
    let code = "let s = \"hi\";\n";
    let at = code.find("hi").unwrap();
    for name in theme::PRESET_THEMES {
        let spans = highlight("rust", code, name, true);
        assert!(!spans.is_empty(), "预置主题 {name} 应产出高亮片段");
        let colored = spans
            .iter()
            .any(|(range, style)| range.start <= at && at < range.end && style.color.is_some());
        assert!(colored, "预置主题 {name} 应给字符串着色");
    }
}

#[test]
fn cache_key_covers_theme_and_appearance() {
    assert_ne!(theme_key("xink", true), theme_key("xink", false));
    assert_ne!(theme_key("xink", true), theme_key("other", true));
}

#[test]
fn emphasis_picks_up_italic_and_strong_picks_up_bold() {
    let theme = syntect_theme(THEME, true);
    let find = |selector: &str| {
        let sel = ScopeSelectors::from_str(selector).unwrap();
        theme
            .scopes
            .iter()
            .find(|item| item.scope == sel)
            .map(|item| item.style.font_style.unwrap_or_default())
    };
    // 预置主题里 emphasis 只有 font_style: italic、emphasis.strong 只有 font_weight
    assert!(
        find("markup.italic")
            .map(|s| s.contains(FontStyle::ITALIC))
            .unwrap_or(false),
        "emphasis 应带上斜体"
    );
    assert!(
        find("markup.bold")
            .map(|s| s.contains(FontStyle::BOLD))
            .unwrap_or(false),
        "emphasis.strong 应带上粗体"
    );
}

#[test]
fn bold_reaches_the_span_style() {
    // Markdown 的 **粗体** 在代码块里不常见，但语法里的 markup.bold 是有的；
    // 这里直接用 Markdown 源码验证 `SpanStyle.bold` 能被带出去。
    let spans = highlight("markdown", "**粗体**\n", THEME, true);
    assert!(
        spans.iter().any(|(_, style)| style.bold),
        "粗体片段应带出 bold 标志: {spans:?}"
    );
}

/// 从预置主题 JSON 里直接读某个语义名的颜色（RGB），供上面的断言比对
fn preset_color(name: &str, dark: bool, semantic: &str) -> Option<(u8, u8, u8)> {
    let json = theme::preset_json(name, dark)?;
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let hex = value
        .get("style")?
        .get("syntax")?
        .get(semantic)?
        .get("color")?
        .as_str()?;
    let hex = hex.strip_prefix('#')?;
    Some((
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

/// 渲染时每个可见代码块每帧都会回调高亮器，命中缓存不能把整份片段列表克隆出来
#[test]
fn cache_hits_share_the_same_spans() {
    let code = "fn shared() -> u32 { 42 }";
    let a = highlight("rust", code, THEME, true);
    let b = highlight("rust", code, THEME, true);

    assert!(
        Arc::ptr_eq(&a, &b),
        "命中缓存应复用同一份片段列表，而不是克隆一份新的"
    );
    assert!(!a.is_empty(), "Rust 代码应有高亮片段");
}

/// 明暗是 key 的一部分：换了明暗必须重算，不能复用暗色结果的片段
#[test]
fn dark_and_light_do_not_share_cache_entries() {
    let code = "let x = 1;";
    let dark = highlight("rust", code, THEME, true);
    let light = highlight("rust", code, THEME, false);

    assert!(!Arc::ptr_eq(&dark, &light), "明暗不同不应命中同一条缓存");
}
