//! 键位下发的测试（动作名 → gpui `KeyBinding`）
//!
//! 注意：这里**不能**用 `use super::*`——本模块顶层有 `use gpui_kit::*;`，
//! 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性并导致宏展开递归。

use std::collections::BTreeMap;

use gpui_kit::Action;

use super::{Save, key_bindings};
use crate::core::keymap::BINDINGS;

/// 键串的显示形态（macOS 上是 ⌘⇧S 这类符号，因此断言都用本函数换算，
/// 不直接写 "cmd-shift-s"）
fn display_of(keystroke: &str) -> String {
    let probe = gpui_kit::KeyBinding::new(keystroke, Save, None);
    probe.keystrokes().first().unwrap().to_string()
}

/// 每条绑定的键串（`KeybindingKeystroke` 实现了 `Display`）
fn keystrokes(bindings: &[gpui_kit::KeyBinding]) -> Vec<String> {
    bindings
        .iter()
        .map(|binding| binding.keystrokes().first().unwrap().to_string())
        .collect()
}

fn keystroke_for(bindings: &[gpui_kit::KeyBinding], action: &str) -> Option<String> {
    bindings
        .iter()
        .find(|binding| binding.action().name().ends_with(action))
        .map(|binding| binding.keystrokes().first().unwrap().to_string())
}

/// 不配 keymap 时：条数与默认键位都与接线前的硬编码表一致
#[test]
fn default_bindings_cover_every_action_with_a_default() {
    let expected: Vec<_> = BINDINGS
        .iter()
        .filter(|binding| !binding.default.is_empty())
        .collect();
    let bindings = key_bindings(&BTreeMap::new());

    assert_eq!(
        bindings.len(),
        expected.len(),
        "默认绑定条数变了：映射表里可能漏了某个 action"
    );
    for binding in expected {
        assert_eq!(
            keystroke_for(&bindings, binding.action).as_deref(),
            Some(display_of(binding.default).as_str()),
            "{} 的默认键位与表不一致",
            binding.id
        );
    }
}

#[test]
fn override_reaches_the_key_binding() {
    let mut overrides = BTreeMap::new();
    overrides.insert("Save".to_string(), "cmd-shift-s".to_string());

    let bindings = key_bindings(&overrides);
    assert_eq!(
        keystroke_for(&bindings, "Save").as_deref(),
        Some(display_of("cmd-shift-s").as_str())
    );
    assert!(
        !keystrokes(&bindings)
            .iter()
            .any(|k| k == &display_of("cmd-s")),
        "cmd-s 应随覆盖一起释放"
    );
}

#[test]
fn every_binding_keeps_its_context() {
    let bindings = key_bindings(&BTreeMap::new());
    assert_eq!(
        bindings
            .iter()
            .filter(|binding| binding.predicate().is_some())
            .count(),
        1,
        "只有 RenameItem 带 FilesPanel 上下文"
    );
    assert_eq!(
        keystroke_for(&bindings, "RenameItem").as_deref(),
        Some(display_of("f2").as_str())
    );
    // 确认 `Save.name()` 这类 action 名带命名空间前缀，上面用 ends_with 是对的
    assert!(Save.name().ends_with("Save"));
}
