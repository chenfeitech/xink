//! 凭据存放
//!
//! 默认走 macOS Keychain（`security` 命令行），**不新增 crate**；
//! 抽象成 trait 是为了让 `core/ai` 的其它部分能在测试里用内存实现。

use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use crate::core::config::{AiConfig, KeySource, ModelEntry};

/// Keychain 里固定的 service 名
const SERVICE: &str = "xink";

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Option<String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, account: &str);
}

/// macOS Keychain：`security add-generic-password` / `find-generic-password`
pub struct MacKeychain;

impl SecretStore for MacKeychain {
    fn get(&self, account: &str) -> Option<String> {
        let out = Command::new("security")
            .args(["find-generic-password", "-s", SERVICE, "-a", account, "-w"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        let out = Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                SERVICE,
                "-a",
                account,
                "-w",
                secret,
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    fn delete(&self, account: &str) {
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", SERVICE, "-a", account])
            .output();
    }
}

/// 测试用：进程内存储
#[derive(Default)]
pub struct MemorySecretStore(Mutex<HashMap<String, String>>);

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, account: &str) -> Option<String> {
        self.0.lock().ok()?.get(account).cloned()
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|e| e.to_string())?
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(account);
        }
    }
}

/// 环境变量来源的默认变量名
pub const KEY_ENV_VAR: &str = "XINK_AI_API_KEY";

/// 按**发起请求的那个模型**解析 API key
///
/// 传整条 `ModelEntry` 而不是只传 id：是否需要 key、从哪取，都是模型自己的
/// 属性。以前传 id 再用全局默认模型判断，导致"默认模型是 Ollama 时云模型
/// 全部取不到 key"。
pub fn resolve_api_key(model: &ModelEntry, store: &dyn SecretStore) -> Option<String> {
    if !AiConfig::needs_key(model) {
        // 本地 Ollama 不需要 key
        return None;
    }
    match model.api_key_source {
        KeySource::Keychain => store.get(&model.id).filter(|s| !s.is_empty()),
        KeySource::Env => std::env::var(KEY_ENV_VAR).ok().filter(|s| !s.is_empty()),
        KeySource::Plain => {
            let key = model.plain_key.trim();
            (!key.is_empty()).then(|| key.to_string())
        }
    }
}

// 单元测试文件在仓库根 `tests/` 下，目录结构镜像 `src/`
#[cfg(test)]
#[path = "../../../tests/core/ai/keychain.rs"]
mod tests;
