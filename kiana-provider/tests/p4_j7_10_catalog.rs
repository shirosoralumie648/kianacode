use kiana_domain::CapabilitySupport;
use kiana_provider::{ProviderConfig, ProviderGateway};
use std::ffi::OsString;

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new(keys: &[&'static str]) -> Self {
        let values = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
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
fn model_catalog_preserves_slashes_and_requires_connection_for_ambiguous_names() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", "2");
    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        r#"{
            "planning":{"provider":"ollama","model":"vendor/model:v1","base_url":"http://127.0.0.1:22115"},
            "executing":{"provider":"ollama","model":"vendor/model:v1","base_url":"http://127.0.0.1:22116"}
        }"#,
    );
    let gateway = ProviderGateway::from_env(config()).unwrap();
    let catalog = gateway.model_catalog().unwrap();
    catalog.validate(None).unwrap();
    assert_eq!(
        catalog
            .resolve("vendor/model:v1", Some("planning"), None)
            .unwrap()
            .connection_id,
        "planning"
    );
    assert_eq!(
        catalog.resolve("vendor/model:v1", None, None).unwrap_err(),
        "model_catalog_ambiguous"
    );
    let planning = catalog
        .resolve("vendor/model:v1", Some("planning"), None)
        .unwrap();
    assert_eq!(planning.capabilities.tools, CapabilitySupport::Unknown);
    assert!(!catalog
        .supports_tools("vendor/model:v1", "planning")
        .unwrap());
}
