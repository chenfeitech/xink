//! 凭据解析

use crate::core::ai::keychain::{KEY_ENV_VAR, MemorySecretStore, SecretStore, resolve_api_key};
use crate::core::config::{AiConfig, AiProviderKind, KeySource, ModelEntry};

fn cfg_with(kind: AiProviderKind, source: KeySource) -> AiConfig {
    let model = ModelEntry {
        id: "m".into(),
        kind,
        api_key_source: source,
        ..Default::default()
    };
    AiConfig {
        enabled: true,
        models: vec![model],
        default_model: "m".into(),
        ..Default::default()
    }
}

#[test]
fn 本地_ollama_不需要_key() {
    let cfg = cfg_with(AiProviderKind::Ollama, KeySource::Keychain);
    let store = MemorySecretStore::new();
    assert_eq!(resolve_api_key(&cfg, "m", &store), None);
}

#[test]
fn keychain_来源从_store_取() {
    let cfg = cfg_with(AiProviderKind::OpenAiCompatible, KeySource::Keychain);
    let store = MemorySecretStore::new();
    store.set("m", "sk-1").unwrap();
    assert_eq!(resolve_api_key(&cfg, "m", &store).as_deref(), Some("sk-1"));
}

#[test]
fn env_来源读环境变量() {
    let cfg = cfg_with(AiProviderKind::OpenAiCompatible, KeySource::Env);
    let store = MemorySecretStore::new();
    // edition 2024 起 set_var / remove_var 是 unsafe
    unsafe { std::env::set_var(KEY_ENV_VAR, "sk-env") };
    assert_eq!(
        resolve_api_key(&cfg, "m", &store).as_deref(),
        Some("sk-env")
    );
    unsafe { std::env::remove_var(KEY_ENV_VAR) };
}
