//! `/api/tags` 解析与缓存

use crate::core::ai::models::{DiscoveredModel, ModelCache, now_ms, parse_tags};
use crate::core::config::{AiConfig, ModelEntry};

const TAGS: &str = r#"{"models":[
  {"name":"qwen3.5:0.8b","size":811856174,
   "details":{"context_length":262144,"family":"qwen35"},
   "capabilities":["completion","vision","tools","thinking"]},
  {"name":"qwen3.5:4b","details":{},"capabilities":["completion"]},
  {"name":""}
]}"#;

#[test]
fn 解析字段与能力() {
    let models = parse_tags(TAGS).unwrap();
    assert_eq!(models.len(), 2, "空名模型应被跳过");
    assert_eq!(models[0].name, "qwen3.5:0.8b");
    assert_eq!(models[0].context_length, Some(262144));
    assert!(models[0].supports_tools());
    assert!(models[0].supports_thinking());
}

#[test]
fn 缺失的_context_length_是_none() {
    // 实测：本机 qwen3.5:4b 就没有 details.context_length
    let models = parse_tags(TAGS).unwrap();
    assert_eq!(models[1].context_length, None);
    assert!(!models[1].supports_tools());
}

#[test]
fn 非_json_与缺字段都报错() {
    assert!(parse_tags("not json").is_err());
    assert!(parse_tags("{}").is_err());
}

#[test]
fn 合并进配置只更新能力字段() {
    let cache = ModelCache {
        fetched_at_ms: 1,
        base_url: "http://localhost:11434".into(),
        models: vec![DiscoveredModel {
            name: "qwen3.5:0.8b".into(),
            size: None,
            context_length: Some(262144),
            capabilities: vec!["tools".into()],
        }],
    };
    let mut cfg = AiConfig::default();
    let mut mine = ModelEntry {
        id: "qwen3.5:0.8b".into(),
        ..Default::default()
    };
    mine.params.num_ctx = Some(4096); // 用户改过
    cfg.models.push(mine);

    assert_eq!(cache.merge_into(&mut cfg.models), 0, "已存在的不应新增");
    assert_eq!(cfg.models[0].context_limit, Some(262144));
    assert_eq!(
        cfg.models[0].params.num_ctx,
        Some(4096),
        "用户参数不能被覆盖"
    );
    // 没手动改过：能力列表跟随 /api/tags
    assert_eq!(cfg.models[0].capabilities, vec!["tools".to_string()]);
}

/// 用户手动改过能力清单后，重新拉取不该把它冲掉
///
/// 否则每次「从 Ollama 拉取」都会把用户勾的能力重置回去（需求 R-4）。
#[test]
fn 手动改过的能力清单不被拉取覆盖() {
    let cache = ModelCache {
        fetched_at_ms: 1,
        base_url: "http://localhost:11434".into(),
        models: vec![DiscoveredModel {
            name: "qwen3.5:0.8b".into(),
            size: None,
            context_length: Some(262144),
            capabilities: vec!["completion".into(), "tools".into()],
        }],
    };
    let mut cfg = AiConfig::default();
    cfg.models.push(ModelEntry {
        id: "qwen3.5:0.8b".into(),
        capabilities: vec!["completion".into()],
        capabilities_pinned: true, // 用户手动删掉了 tools
        ..Default::default()
    });

    cache.merge_into(&mut cfg.models);

    assert_eq!(
        cfg.models[0].capabilities,
        vec!["completion".to_string()],
        "用户勾的清单不能被 /api/tags 覆盖"
    );
    // 上下文上限不属于"用户手动改过"的范畴，仍要更新
    assert_eq!(cfg.models[0].context_limit, Some(262144));
}

#[test]
fn cache_age_is_zero_for_a_future_timestamp() {
    // 时钟回拨 / 手改缓存文件：`saturating_sub` 不能下溢成一个天文数字
    let cache = ModelCache {
        fetched_at_ms: u64::MAX,
        base_url: String::new(),
        models: Vec::new(),
    };
    assert_eq!(cache.age_secs(), 0);

    let fresh = ModelCache {
        fetched_at_ms: now_ms(),
        ..cache
    };
    assert!(fresh.age_secs() <= 1, "刚抓的就该是 0 秒");
}

#[test]
fn cache_age_counts_full_seconds() {
    let cache = ModelCache {
        fetched_at_ms: now_ms() - 5_000,
        base_url: String::new(),
        models: Vec::new(),
    };
    assert_eq!(cache.age_secs(), 5);
}
