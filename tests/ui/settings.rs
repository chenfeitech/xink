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

use super::{ADVANCED_IDS, FieldKind, FieldTier, SettingsTab, all_fields};
use super::{capability_summary, compact_num, ctx_options, truncate_chars, vendor_of};
use crate::core::config::{AiProviderKind, AppConfig, ModelEntry};

/// 上下文窗口的档位必须按模型上限过滤
///
/// 不过滤的话，选了超限的档会被 `sanitize()` 悄悄夹回
/// （`config.rs` 的 `AI_NUM_CTX` 校正）：界面显示一个数、生效的是另一个，
/// 比不列这一档更糟。
#[test]
fn ctx_options_filters_steps_above_the_model_limit() {
    // 模型没报上限（`/api/tags` 未必给 `context_length`）：全列，由 sanitize 兜底
    assert_eq!(ctx_options(None).len(), 6);
    assert_eq!(ctx_options(Some(4096)), vec![4096]);
    assert_eq!(ctx_options(Some(16384)), vec![4096, 8192, 16384]);
}

#[test]
fn ctx_options_stays_usable_below_the_smallest_step() {
    // 上限低于最小档：至少给一档，否则下拉是空的、没法选
    assert_eq!(ctx_options(Some(2048)), vec![2048]);
    for cap in [512, 2048, 4096, 100_000] {
        let options = ctx_options(Some(cap));
        assert!(!options.is_empty(), "cap={cap} 至少要有一档");
        assert!(options.iter().all(|v| *v <= cap), "cap={cap}：{options:?}");
    }
}

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
    // M2（Skills）已落地：`ai.skills_enabled` 不再是死开关，标记已摘掉。
    let known: HashSet<&str> = HashSet::new();

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

/// 收进「高级」的项必须与白名单逐条对上
///
/// 分级表在需求文档 §3.1，白名单在 `ADVANCED_IDS`。两边任一处改动而另一处没跟，
/// 就会出现"文档说收起来了、界面上还摊着"这类偏差，所以用测试锁死。
#[test]
fn advanced_fields_match_the_documented_whitelist() {
    let marked: HashSet<&str> = all_fields()
        .iter()
        .filter(|field| field.tier == FieldTier::Advanced)
        .map(|field| field.id)
        .collect();
    let documented: HashSet<&str> = ADVANCED_IDS.iter().copied().collect();

    // 写错 id 的表现是"这一项永远收不进高级"，集合比较抓不到，单独查一次
    for id in ADVANCED_IDS {
        assert!(
            all_fields().iter().any(|field| field.id == *id),
            "白名单里的 {id} 在描述表里不存在（id 写错了？）"
        );
    }
    assert_eq!(
        marked, documented,
        "高级项与 ADVANCED_IDS 不一致：改了一边忘了另一边"
    );
}

/// 快捷键独立成分类：16 项都在「快捷键」，且「编辑器」里不再混着它们
#[test]
fn keymap_fields_live_in_their_own_tab() {
    let keymap_count = all_fields()
        .iter()
        .filter(|field| field.id.starts_with("editor.keymap."))
        .count();
    assert!(keymap_count > 0, "快捷键项不应为空（绑定表没接上？）");

    for field in all_fields() {
        if field.id.starts_with("editor.keymap.") {
            assert!(
                field.tab == SettingsTab::Keymap,
                "{} 应归到「快捷键」分类",
                field.id
            );
        } else {
            assert!(
                field.tab != SettingsTab::Keymap,
                "{} 不是快捷键却被放进「快捷键」分类",
                field.id
            );
        }
    }
}

/// 说明 ≤20 字：长说明进 `TIPS`（悬停可见），主视图信息密度才压得下来
#[test]
fn descriptions_are_short_enough() {
    // 一次列全部，省得改一个跑一次
    let too_long: Vec<String> = all_fields()
        .iter()
        .filter(|field| field.desc.chars().count() > 20)
        .map(|field| format!("{}({}字)", field.id, field.desc.chars().count()))
        .collect();
    assert!(
        too_long.is_empty(),
        "说明超过 20 字：{}",
        too_long.join("、")
    );
}

/// 列表里的提示词只做预览：短的照原样，超长截断并加省略号
#[test]
fn truncate_chars_keeps_short_text_and_marks_long() {
    assert_eq!(truncate_chars("润色这一段", 100), "润色这一段");
    let long: String = std::iter::repeat_n('词', 120).collect();
    let out = truncate_chars(&long, 100);
    assert_eq!(out.chars().count(), 101);
    assert!(out.ends_with('…'));
}

/// 厂家归组只认 base_url 的域名：换厂家不用改数据结构，
/// 填了地址就自动落到对应区
#[test]
fn vendor_grouping_uses_base_url_host() {
    let model = |base_url: &str| ModelEntry {
        id: "m".into(),
        kind: AiProviderKind::OpenAiCompatible,
        base_url: base_url.into(),
        ..Default::default()
    };

    let (name, host) = vendor_of(&model("https://api.deepseek.com/v1"));
    assert_eq!(name, "deepseek");
    assert_eq!(host, "api.deepseek.com");

    // 带端口、大写、带路径都归一
    assert_eq!(
        vendor_of(&model("http://LOCALHOST:11434/v1")).0,
        "localhost"
    );
    // 没填地址 → 归不进任何厂家
    assert_eq!(vendor_of(&model("")).0, "其他");
    assert_eq!(vendor_of(&model("   ")).0, "其他");
}

/// 128000 → 128k：能力小字里塞原始数字太长
#[test]
fn compact_num_uses_k_and_m() {
    assert_eq!(compact_num(512), "512");
    assert_eq!(compact_num(128_000), "128k");
    assert_eq!(compact_num(2_000_000), "2M");
}

/// 信息小字：上下文 + 工具结论 + 停用状态
///
/// 能力列表不在这里了——它改成了 chip 行（每项悬停能看说明），
/// 挤在一行小字里做不到"说明"。这里只管可测的那部分文案。
#[test]
fn capability_summary_lists_context_and_tools() {
    let mut model = ModelEntry {
        id: "m".into(),
        capabilities: vec!["chat".into(), "tools".into()],
        context_limit: Some(128_000),
        ..Default::default()
    };
    // 声明有 tools 但没测过 → 待实测
    assert_eq!(capability_summary(&model), "上下文 128k · 工具调用待实测");

    model.tools_enabled = true;
    assert_eq!(capability_summary(&model), "上下文 128k · 工具调用已实测");

    model.capabilities = vec!["chat".into()];
    model.tools_enabled = false;
    model.enabled = false;
    assert_eq!(
        capability_summary(&model),
        "上下文 128k · 不支持工具调用 · 已停用"
    );
}

/// OpenAI 兼容端点没有能力来源：展示时要用推断值，不能留白
#[test]
fn capability_summary_uses_inferred_caps_for_compatible_endpoints() {
    let model = ModelEntry {
        id: "deepseek-chat".into(),
        kind: AiProviderKind::OpenAiCompatible,
        base_url: "https://api.deepseek.com".into(),
        // capabilities 空、也没手动改过 → 走 infer
        ..Default::default()
    };
    assert_eq!(
        capability_summary(&model),
        "工具调用待实测",
        "推断出 tools 就该显示「待实测」，而不是留白"
    );
}

/// 用户手动改过：以用户勾的为准，推断值不再介入
#[test]
fn capability_summary_prefers_pinned_caps() {
    let model = ModelEntry {
        id: "deepseek-chat".into(),
        kind: AiProviderKind::OpenAiCompatible,
        base_url: "https://api.deepseek.com".into(),
        capabilities: vec!["completion".into()],
        capabilities_pinned: true, // 用户手动删掉了 tools
        ..Default::default()
    };
    assert_eq!(
        capability_summary(&model),
        "不支持工具调用",
        "用户勾的清单优先于推断值"
    );
}

/// 排查用的 debug 项不给入口（配置字段保留，旧 `config.json` 里的值照旧生效）
#[test]
fn debug_only_field_has_no_settings_entry() {
    let present = all_fields()
        .iter()
        .any(|field| field.id == "preview.debug_scroll_sync");
    assert!(
        !present,
        "preview.debug_scroll_sync 是排查用的调试开关，不该出现在设置界面"
    );
}
