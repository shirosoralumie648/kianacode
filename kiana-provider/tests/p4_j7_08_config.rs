use kiana_domain::{ProviderConfigSource, ProviderSelectionMode};
use kiana_provider::{ProviderConfig, ProviderGateway};
use serde_json::json;
use std::ffi::OsString;

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new(keys: &[&'static str]) -> Self {
        let values = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for key in keys {
            std::env::remove_var(key);
        }
        Self(values)
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn config() -> ProviderConfig {
    ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("default-model".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    }
}

#[test]
fn profile_snapshot_is_secret_free_and_changes_revision_on_route_change() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", "2");
    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        r#"{"planning":{"provider":"ollama","model":"plan-v1","base_url":"http://127.0.0.1:22115"}}"#,
    );
    let first = ProviderGateway::from_env(config()).unwrap();
    let snapshot = first.configuration_snapshot().unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.selection_mode, ProviderSelectionMode::Live);
    let planning = snapshot
        .profiles
        .iter()
        .find(|profile| profile.profile == "planning")
        .unwrap();
    assert_eq!(planning.source, ProviderConfigSource::Profile);
    assert!(planning.credential_ref.is_none());
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains("api_key"));

    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        r#"{"planning":{"provider":"ollama","model":"plan-v2","base_url":"http://127.0.0.1:22115"}}"#,
    );
    let second = ProviderGateway::from_env(config()).unwrap();
    assert_ne!(
        first.configuration_snapshot().unwrap().snapshot_digest,
        second.configuration_snapshot().unwrap().snapshot_digest
    );
}

#[test]
fn unknown_profile_and_invalid_streaming_policy_fail_closed() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        json!({"future": {"provider":"ollama","model":"x"}}).to_string(),
    );
    let error = match ProviderGateway::from_env(config()) {
        Ok(_) => panic!("unknown profile must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "model_profile_unknown");

    std::env::remove_var("KIANA_MODEL_PROFILES_JSON");
    std::env::set_var("KIANA_STREAMING", "sometimes");
    let error = match ProviderGateway::from_env(config()) {
        Ok(_) => panic!("invalid streaming policy must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "model_streaming_policy_invalid");
}
