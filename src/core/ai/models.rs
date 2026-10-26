//! Ollama 模型发现：`GET /api/tags` 的解析与缓存
//!
//! 只解析**我们用到的字段**，其余一律忽略——各家往 tags 里塞的字段不一致，
//! 全量建模只会带来无谓的兼容负担。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::AiError;
use crate::core::config::ModelEntry;

/// 一个被发现的模型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub name: String,
    #[serde(default)]
    pub size: Option<u64>,
    /// 实测：并非所有模型都给 `details.context_length`（本机 `qwen3.5:4b` 就没有）
    #[serde(default)]
    pub context_length: Option<u32>,
    /// `completion` / `tools` / `thinking` / `vision`
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl DiscoveredModel {
    /// 声明支持工具调用。**不等于真的能用**——连接测试才是判据（见设计 §4.3）。
    pub fn supports_tools(&self) -> bool {
        self.capabilities.iter().any(|c| c == "tools")
    }

    pub fn supports_thinking(&self) -> bool {
        self.capabilities.iter().any(|c| c == "thinking")
    }
}

/// 解析 `/api/tags` 的响应
pub fn parse_tags(json: &str) -> Result<Vec<DiscoveredModel>, AiError> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| AiError::Protocol(format!("tags 不是 JSON: {e}")))?;
    let arr = value
        .get("models")
        .and_then(|m| m.as_array())
        .ok_or_else(|| AiError::Protocol("tags 缺少 models 字段".into()))?;

    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        out.push(DiscoveredModel {
            name: name.to_string(),
            size: item.get("size").and_then(|v| v.as_u64()),
            context_length: item
                .pointer("/details/context_length")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            capabilities: item
                .get("capabilities")
                .and_then(|v| v.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 模型列表缓存：服务没启动时也能展示上次的结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelCache {
    /// Unix 毫秒
    pub fetched_at_ms: u64,
    pub base_url: String,
    pub models: Vec<DiscoveredModel>,
}

impl ModelCache {
    pub fn load(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), AiError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AiError::Io(e.to_string()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| AiError::Io(e.to_string()))?;
        fs::write(path, text).map_err(|e| AiError::Io(e.to_string()))
    }

    /// 缓存年龄（秒）；时钟异常时按 u64::MAX 处理（即视为过期）
    pub fn age_secs(&self) -> u64 {
        now_ms().saturating_sub(self.fetched_at_ms) / 1000
    }

    /// 把发现结果并入已配置的模型列表。
    ///
    /// 只更新**能力类字段**（上下文上限、能力列表），绝不覆盖用户改过的参数——
    /// 否则每次「拉取」都会把用户调好的 num_ctx / temperature 冲掉。
    /// 返回新增的模型数量。
    pub fn merge_into(&self, entries: &mut Vec<ModelEntry>) -> usize {
        let mut added = 0;
        for found in &self.models {
            match entries.iter_mut().find(|e| e.id == found.name) {
                Some(entry) => {
                    entry.context_limit = found.context_length;
                    entry.capabilities = found.capabilities.clone();
                }
                None => {
                    entries.push(ModelEntry::discovered(found));
                    added += 1;
                }
            }
        }
        added
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/models.rs"]
mod tests;
