//! 凭据解析

use crate::core::ai::keychain::{resolve_api_key, MemorySecretStore, SecretStore, KEY_ENV_VAR};
use crate::core::config::{AiConfig, AiProviderKind, KeySource, ModelEntry};

fn cfg_with(kind: AiProviderKind, source: KeySource) -> AiConfig {
    let mut cfg = AiConfig::default();
    cfg.enabled = true;
    let mut model = ModelEntry::default();
    model.id = "m".into();
    model.kind = kind;
    model.api_key_source = source;
    cfg.models.push(model);
    cfg.default_model = "m".into();
    cfg
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
    assert_eq!(resolve_api_key(&cfg, "m", &store).as_deref(), Some("sk-env"));
    unsafe { std::env::remove_var(KEY_ENV_VAR) };
}
