//! `/api/tags` 解析与缓存

use crate::core::ai::models::{parse_tags, DiscoveredModel, ModelCache};
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
    let mut mine = ModelEntry::default();
    mine.id = "qwen3.5:0.8b".into();
    mine.params.num_ctx = 4096; // 用户改过
    cfg.models.push(mine);

    assert_eq!(cache.merge_into(&mut cfg.models), 0, "已存在的不应新增");
    assert_eq!(cfg.models[0].context_limit, Some(262144));
    assert_eq!(cfg.models[0].params.num_ctx, 4096, "用户参数不能被覆盖");
}
