use kiana_provider::{ProviderConfig, ProviderGateway};
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

fn config(api_key: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        provider: Some("openai".to_owned()),
        model: Some("gpt-4o-mini".to_owned()),
        base_url: Some("https://api.openai.com/v1".to_owned()),
        api_key: api_key.map(str::to_owned),
    }
}

#[test]
fn explicit_credential_is_ref_only_in_configuration_and_catalog() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    let gateway = ProviderGateway::from_env(config(Some("CI07_SECRET_SENTINEL")))
        .expect("explicit provider config");
    let snapshot = gateway.configuration_snapshot().expect("snapshot");
    let encoded = serde_json::to_string(&snapshot).expect("snapshot json");
    assert!(!encoded.contains("CI07_SECRET_SENTINEL"));
    assert!(snapshot.profiles.iter().all(|profile| profile
        .credential_ref
        .as_deref()
        .is_some_and(|value| { value.starts_with("sha256:") && value.len() == 71 })));
    let catalog = serde_json::to_string(&gateway.catalog()).expect("catalog json");
    assert!(!catalog.contains("CI07_SECRET_SENTINEL"));
}

#[test]
fn env_credential_reference_is_opaque_and_missing_resolution_fails_closed() {
    let _env = EnvRestore::new(&[
        "OPENAI_API_KEY",
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("OPENAI_API_KEY", "CI07_ENV_SECRET_SENTINEL");
    let gateway = ProviderGateway::from_env(config(None)).expect("env provider config");
    let snapshot = gateway.configuration_snapshot().expect("snapshot");
    assert!(!serde_json::to_string(&snapshot)
        .expect("snapshot json")
        .contains("CI07_ENV_SECRET_SENTINEL"));

    std::env::remove_var("OPENAI_API_KEY");
    let error = match ProviderGateway::from_env(config(None)) {
        Ok(_) => panic!("missing env secret must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "model_credential_unavailable");
}
