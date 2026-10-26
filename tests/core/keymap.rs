//! 快捷键绑定表与覆盖解析的测试

use std::collections::{BTreeMap, BTreeSet};

use super::{BINDINGS, Resolved, is_valid, normalize, override_of, resolve, sanitize};

fn overrides_of(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn keystroke_of(resolved: &[Resolved], action: &str) -> Option<String> {
    resolved
        .iter()
        .find(|r| r.action == action)
        .map(|r| r.keystroke.clone())
}

#[test]
fn binding_table_is_consistent() {
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    let mut actions: BTreeSet<&str> = BTreeSet::new();
    for binding in BINDINGS {
        assert!(ids.insert(binding.id), "id 重复：{}", binding.id);
        assert!(
            actions.insert(binding.action),
            "action 重复：{}",
            binding.action
        );
        assert!(
            binding.id.starts_with("editor.keymap."),
            "{} 的 id 应以 editor.keymap. 开头",
            binding.id
        );
        assert!(!binding.label.is_empty(), "{} 缺少标题", binding.id);
        assert!(!binding.desc.is_empty(), "{} 缺少说明", binding.id);
        assert!(
            binding.default.is_empty() || is_valid(binding.default),
            "{} 的默认键位 {} 解析不过",
            binding.id,
            binding.default
        );
    }
}

/// 接线本身零行为变化：不配 keymap 时输出必须与接线前的硬编码表一致
#[test]
fn defaults_match_the_hardcoded_table() {
    let resolved = resolve(&BTreeMap::new());
    let with_default: Vec<_> = BINDINGS.iter().filter(|b| !b.default.is_empty()).collect();

    assert_eq!(
        resolved.len(),
        with_default.len(),
        "默认绑定条数发生变化（接线前的硬编码表是 15 条）"
    );
    for binding in with_default {
        assert_eq!(
            keystroke_of(&resolved, binding.action).as_deref(),
            Some(binding.default),
            "{} 的默认键位与表不一致",
            binding.id
        );
    }
}

#[test]
fn override_replaces_default() {
    let resolved = resolve(&overrides_of(&[("Save", "cmd-shift-s")]));
    assert_eq!(
        keystroke_of(&resolved, "Save").as_deref(),
        Some("cmd-shift-s")
    );
    // 默认键位被释放：不再有别的动作占着 cmd-s
    assert!(
        !resolved.iter().any(|r| r.keystroke == "cmd-s"),
        "cmd-s 应随覆盖一起释放"
    );
}

#[test]
fn empty_override_falls_back_to_default() {
    let resolved = resolve(&overrides_of(&[("Save", "   ")]));
    assert_eq!(keystroke_of(&resolved, "Save").as_deref(), Some("cmd-s"));
}

#[test]
fn invalid_keystroke_is_ignored() {
    let resolved = resolve(&overrides_of(&[("Save", "cmd-"), ("OpenSearch", "shift-")]));
    assert_eq!(keystroke_of(&resolved, "Save").as_deref(), Some("cmd-s"));
    assert_eq!(
        keystroke_of(&resolved, "OpenSearch").as_deref(),
        Some("cmd-shift-f")
    );
}

#[test]
fn unknown_action_is_ignored() {
    let resolved = resolve(&overrides_of(&[("NotAnAction", "cmd-j")]));
    assert!(
        !resolved.iter().any(|r| r.action == "NotAnAction"),
        "未知 action 不应产生绑定"
    );
}

#[test]
fn normalize_trims_and_lowercases() {
    assert_eq!(normalize(" cmd-S "), "cmd-s");
    assert_eq!(normalize("CMD-SHIFT-X"), "cmd-shift-x");
    assert_eq!(normalize("cmd-k   cmd-s"), "cmd-k cmd-s");
    assert_eq!(
        override_of(&overrides_of(&[("Save", "CMD-S")]), "Save"),
        "cmd-s"
    );
}

#[test]
fn duplicate_keystroke_keeps_the_earlier() {
    // NewNote 在表里靠前，Save 抢它的键位应失败并回退默认
    let resolved = resolve(&overrides_of(&[("Save", "cmd-n")]));
    assert_eq!(keystroke_of(&resolved, "Save").as_deref(), Some("cmd-s"));
    assert_eq!(keystroke_of(&resolved, "NewNote").as_deref(), Some("cmd-n"));
}

#[test]
fn sanitize_drops_empty_invalid_and_unknown_entries() {
    let mut map = overrides_of(&[
        ("Save", " CMD-SHIFT-S "),
        ("OpenSearch", "shift-"),
        ("NotAnAction", "cmd-j"),
        ("ToggleTheme", ""),
    ]);
    sanitize(&mut map);

    assert_eq!(map.get("Save").map(String::as_str), Some("cmd-shift-s"));
    assert!(!map.contains_key("OpenSearch"), "非法键串应被删除");
    assert!(!map.contains_key("NotAnAction"), "未知 action 应被删除");
    assert!(!map.contains_key("ToggleTheme"), "空值应被删除");
}

#[test]
fn is_valid_rejects_broken_keystrokes() {
    assert!(is_valid("cmd-s"));
    assert!(is_valid("cmd-,"));
    assert!(is_valid("f2"));
    assert!(!is_valid(""));
    assert!(!is_valid("   "));
    assert!(!is_valid("cmd-"));
}
