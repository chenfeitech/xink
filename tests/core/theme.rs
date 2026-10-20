use super::*;

#[test]
fn preset_json_covers_every_preset_in_both_appearances() {
    for name in PRESET_THEMES {
        for dark in [false, true] {
            let json = preset_json(name, dark)
                .unwrap_or_else(|| panic!("预置主题 {name}（dark={dark}）缺少 JSON 资源"));
            assert!(!json.trim().is_empty(), "预置主题 {name} 的 JSON 为空");
        }
    }
}

#[test]
fn preset_json_is_valid_highlight_theme_json() {
    // 预置主题在 `ui::theme` 里被反序列化成 `HighlightTheme`，这里只做 JSON 形状校验：
    // `core` 层不能引 gpui 类型，只能断言"有 name + style.syntax 且能解析成对象"。
    for name in PRESET_THEMES {
        for dark in [false, true] {
            let json = preset_json(name, dark).unwrap();
            let value: serde_json::Value =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("{name} 不是合法 JSON: {e}"));
            let syntax = value
                .get("style")
                .and_then(|style| style.get("syntax"))
                .and_then(|syntax| syntax.as_object())
                .unwrap_or_else(|| panic!("{name} 缺少 style.syntax 对象"));
            assert!(!syntax.is_empty(), "{name} 的语法配色为空");
        }
    }
}

#[test]
fn normalize_theme_name_accepts_known_and_falls_back() {
    for name in PRESET_THEMES {
        assert_eq!(normalize_theme_name(name), name);
    }
    assert_eq!(normalize_theme_name("no-such-theme"), DEFAULT_THEME);
    assert_eq!(normalize_theme_name(""), DEFAULT_THEME);
}

#[test]
fn dark_and_light_variants_are_not_identical() {
    // 每个预置主题都必须真有明暗两套配色，否则"切明暗两端同时变"无从谈起
    for name in PRESET_THEMES {
        let dark = syntax_of(name, true);
        let light = syntax_of(name, false);
        assert_ne!(
            dark, light,
            "预置主题 {name} 的明暗变体配色完全相同，等于没做两套"
        );
    }
}

#[test]
fn legacy_names_map_to_the_matching_preset() {
    // ocean / solarized 就是从同名 syntect 主题转来的，旧值应精确落回它们
    assert_eq!(migrate_code_theme("base16-ocean.dark"), "ocean");
    assert_eq!(migrate_code_theme("base16-ocean.light"), "ocean");
    assert_eq!(migrate_code_theme("Solarized (dark)"), "solarized");
    assert_eq!(migrate_code_theme("Solarized (light)"), "solarized");
    // 没有对应预置主题的旧值退回默认
    assert_eq!(migrate_code_theme("InspiredGitHub"), DEFAULT_THEME);
}

/// 取某个预置主题的 `style.syntax` 表，供上面比对
fn syntax_of(name: &str, dark: bool) -> serde_json::Map<String, serde_json::Value> {
    let json = preset_json(name, dark).unwrap();
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    value["style"]["syntax"]
        .as_object()
        .expect("缺少 style.syntax")
        .clone()
}

#[test]
fn migrate_code_theme_always_returns_a_preset() {
    // 旧值是 syntect 主题名，映射失败也必须落到一个存在的预置主题上
    for old in [
        "auto",
        "base16-ocean.dark",
        "base16-eighties.dark",
        "InspiredGitHub",
        "Solarized (light)",
        "完全不认识的主题",
    ] {
        let mapped = migrate_code_theme(old);
        assert!(
            PRESET_THEMES.contains(&mapped),
            "迁移结果 {mapped} 不是预置主题"
        );
    }
}
