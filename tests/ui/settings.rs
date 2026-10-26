//! 设置界面描述表（`all_fields`）的纯逻辑测试
//!
//! 注意：这里**不能**用 `use super::*`——本模块顶层有 `use gpui_kit::*;`，
//! 其中的 gpui `test` 宏会遮蔽内置 `#[test]` 属性并导致宏展开递归。
//!
//! 覆盖目标：`docs/config/config-wiring-design.md` §1 要求的
//! 「配置 → UI 行为改变」防退化——设置表是元数据驱动的，
//! 新增一项 = 表里加一行，若取值/写入/重置三者对不上，
//! 界面上会出现"值改了但 ⟲ 不出现"或"点了 ⟲ 没恢复"这类静默错误。

use std::collections::HashSet;

use super::{FieldKind, all_fields};
use crate::core::config::AppConfig;

/// 描述表里的每一段命名空间（`id` 的第一段）
///
/// 与 `AppConfig` 的顶层段保持一致；新增配置段时同步这里。
const SECTIONS: [&str; 5] = ["basic", "highlight", "editor", "preview", "ai"];

fn section_of(id: &str) -> &str {
    id.split('.').next().unwrap_or("")
}

#[test]
fn field_ids_are_unique() {
    let mut seen: HashSet<&str> = HashSet::new();
    for field in all_fields() {
        assert!(
            seen.insert(field.id),
            "设置项 id 重复：{}（重置按钮与搜索定位都按 id 索引）",
            field.id
        );
    }
}

#[test]
fn ids_are_namespaced_by_config_section() {
    for field in all_fields() {
        let section = section_of(field.id);
        assert!(
            SECTIONS.contains(&section),
            "设置项 {} 的 id 第一段 {section:?} 不是已知配置段",
            field.id
        );
    }
}

#[test]
fn field_metadata_is_complete() {
    for field in all_fields() {
        assert!(!field.title.is_empty(), "{} 缺少标题", field.id);
        assert!(!field.desc.is_empty(), "{} 缺少说明", field.id);
        assert!(!field.group.is_empty(), "{} 缺少分组", field.id);
    }
}

#[test]
fn every_field_starts_at_default() {
    let cfg = AppConfig::default();
    for field in all_fields() {
        assert!(
            (field.is_default)(&cfg),
            "{} 在默认配置下就显示 ⟲（取值与默认值判定不一致）",
            field.id
        );
    }
}

#[test]
fn reset_is_idempotent() {
    let mut cfg = AppConfig::default();
    for field in all_fields() {
        (field.reset)(&mut cfg);
        assert!(
            (field.is_default)(&cfg),
            "{} 重置后仍未回到默认值",
            field.id
        );
    }
}

#[test]
fn text_fields_change_config_and_reset_back() {
    let mut cfg = AppConfig::default();
    for field in all_fields() {
        if field.kind != FieldKind::Text {
            continue;
        }
        (field.text_set)(&mut cfg, "xink-test-value".into());
        assert!(
            !(field.is_default)(&cfg),
            "{} 改值后仍未标记非默认（界面不会出现 ⟲）",
            field.id
        );
        (field.reset)(&mut cfg);
        assert!((field.is_default)(&cfg), "{} 重置后未恢复默认", field.id);
    }
}

#[test]
fn number_fields_change_config_and_reset_back() {
    let mut cfg = AppConfig::default();
    for field in all_fields() {
        if field.kind != FieldKind::Number {
            continue;
        }
        let before = (field.value)(&cfg);
        (field.set_value)(&mut cfg, before + 1.0);
        assert!(
            !(field.is_default)(&cfg),
            "{} 改值后仍未标记非默认（界面不会出现 ⟲）",
            field.id
        );
        (field.reset)(&mut cfg);
        assert!((field.is_default)(&cfg), "{} 重置后未恢复默认", field.id);
    }
}

/// "标了暂未生效"的项必须逐项登记在这里，不许悄悄增加
///
/// `docs/INDEX.md` §10 记录的死配置只剩 `editor.keymap`，而它连设置项
/// 入口都没有（不在这张表里）。表里每多一个 ⚠ 项，就意味着又出现一项
/// "存了不生效"的配置，需同步 §10 与 `config-wiring-design.md`。
#[test]
fn unavailable_fields_match_the_documented_set() {
    // 只登记到 M2（Skills）为止：这两项随 M2 落地后应从表里摘掉标记。
    let known: HashSet<&str> = ["ai.skills_enabled"].into_iter().collect();

    let marked: HashSet<&str> = all_fields()
        .iter()
        .filter(|field| field.unavailable.is_some())
        .map(|field| field.id)
        .collect();

    assert_eq!(
        marked, known,
        "⚠ 暂未生效项发生变化：需同步 docs/INDEX.md §10 与 config-wiring-design.md"
    );
}
