//! 凭据解析

use crate::core::ai::keychain::{KEY_ENV_VAR, MemorySecretStore, SecretStore, resolve_api_key};
use crate::core::config::{AiProviderKind, KeySource, ModelEntry};

fn model(kind: AiProviderKind, source: KeySource) -> ModelEntry {
    ModelEntry {
        id: "m".into(),
        kind,
        api_key_source: source,
        ..Default::default()
    }
}

fn cloud(source: KeySource) -> ModelEntry {
    model(AiProviderKind::OpenAiCompatible, source)
}

#[test]
fn 本地_ollama_不需要_key() {
    let model = model(AiProviderKind::Ollama, KeySource::Keychain);
    let store = MemorySecretStore::new();
    store.set("m", "sk-1").unwrap();
    assert_eq!(resolve_api_key(&model, &store), None);
}

#[test]
fn keychain_来源从_store_取() {
    let model = cloud(KeySource::Keychain);
    let store = MemorySecretStore::new();
    store.set("m", "sk-1").unwrap();
    assert_eq!(resolve_api_key(&model, &store).as_deref(), Some("sk-1"));
}

#[test]
fn env_来源读环境变量() {
    let model = cloud(KeySource::Env);
    let store = MemorySecretStore::new();
    // edition 2024 起 set_var / remove_var 是 unsafe
    unsafe { std::env::set_var(KEY_ENV_VAR, "sk-env") };
    assert_eq!(resolve_api_key(&model, &store).as_deref(), Some("sk-env"));
    unsafe { std::env::remove_var(KEY_ENV_VAR) };
}

#[test]
fn 明文来源读模型条目上的_key() {
    let mut model = cloud(KeySource::Plain);
    model.plain_key = "sk-plain".into();
    let store = MemorySecretStore::new();
    assert_eq!(resolve_api_key(&model, &store).as_deref(), Some("sk-plain"));
}

#[test]
fn 明文没填就当没有_key() {
    let model = cloud(KeySource::Plain);
    let store = MemorySecretStore::new();
    assert_eq!(resolve_api_key(&model, &store), None);
}

#[test]
fn 云模型的_key_解析不受默认模型影响() {
    // 这是修掉的 bug：以前用"全局默认模型是不是 Ollama"来短路，
    // 默认模型是本地时，云模型一个 key 都取不到
    let cloud_model = cloud(KeySource::Keychain);
    let store = MemorySecretStore::new();
    store.set("m", "sk-cloud").unwrap();
    assert_eq!(
        resolve_api_key(&cloud_model, &store).as_deref(),
        Some("sk-cloud"),
        "按请求模型判断，与全局默认无关"
    );
}
