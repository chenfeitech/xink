use super::*;

#[test]
fn highlights_known_language() {
    let spans = highlight_with_theme("rust", "fn main() { let x = 1; }", DEFAULT_DARK_THEME);
    assert!(!spans.is_empty(), "Rust 代码应产出高亮片段");

    let code = "fn main() { let x = 1; }";
    for (range, _) in &spans {
        assert!(range.start <= range.end);
        assert!(range.end <= code.len(), "范围越界: {range:?}");
    }
}

#[test]
fn cache_returns_same_result() {
    let code = "fn main() {}";
    let a = highlight_with_theme("rust", code, DEFAULT_DARK_THEME);
    let b = highlight_with_theme("rust", code, DEFAULT_DARK_THEME);
    assert_eq!(a.len(), b.len(), "缓存命中应返回相同结果");
}

#[test]
fn unknown_language_falls_back_without_panic() {
    let spans = highlight_with_theme(
        "not-a-real-language",
        "plain text here",
        DEFAULT_LIGHT_THEME,
    );
    let _ = spans;
}

#[test]
fn dark_and_light_theme_both_work() {
    assert!(!highlight_with_theme("json", "{\"a\": 1}", DEFAULT_DARK_THEME).is_empty());
    assert!(!highlight_with_theme("json", "{\"a\": 1}", DEFAULT_LIGHT_THEME).is_empty());
}

#[test]
fn unknown_theme_falls_back_instead_of_panicking() {
    // 配置里可能填了不存在的主题名，必须退回默认而不是在渲染路径上 panic
    let spans = highlight_with_theme("rust", "fn main() {}", "no-such-theme");
    assert!(!spans.is_empty(), "未知主题应退回默认主题");
}

#[test]
fn theme_names_include_both_defaults() {
    let names = theme_names();
    assert!(
        names.iter().any(|n| n == DEFAULT_DARK_THEME),
        "主题列表应包含默认深色主题: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == DEFAULT_LIGHT_THEME),
        "主题列表应包含默认浅色主题: {names:?}"
    );
}
