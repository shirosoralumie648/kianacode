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

fn config(provider: &str, base_url: &str, api_key: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        provider: Some(provider.to_owned()),
        model: Some("fixture-model".to_owned()),
        base_url: Some(base_url.to_owned()),
        api_key: api_key.map(str::to_owned),
    }
}

fn error_code(config: ProviderConfig) -> String {
    match ProviderGateway::from_env(config) {
        Ok(_) => panic!("configuration should fail closed"),
        Err(error) => error.code,
    }
}

#[test]
fn invalid_auth_header_never_panics_and_missing_secret_fails_closed() {
    let _env = EnvRestore::new(&["KIANA_STREAMING", "KIANA_MODEL_MAX_CONCURRENCY"]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", "2");
    assert_eq!(
        error_code(config(
            "ollama",
            "http://127.0.0.1:22114",
            Some("bad\nheader")
        )),
        "model_credential_header_invalid"
    );
    assert_eq!(
        error_code(config("anthropic", "https://api.anthropic.com", None)),
        "model_credential_unavailable"
    );
}

#[test]
fn endpoint_userinfo_query_fragment_and_non_tls_are_rejected_but_loopback_is_allowed() {
    let _env = EnvRestore::new(&["KIANA_STREAMING", "KIANA_MODEL_MAX_CONCURRENCY"]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", "2");
    for url in [
        "http://user:pass@127.0.0.1:22114",
        "http://127.0.0.1:22114?token=secret",
        "http://127.0.0.1:22114#fragment",
        "http://203.0.113.9:22114",
    ] {
        assert!(matches!(
            ProviderGateway::from_env(config("ollama", url, None)),
            Err(_)
        ));
    }
    let gateway = ProviderGateway::from_env(config("ollama", "http://127.0.0.1:22114", None));
    assert!(
        gateway.is_ok(),
        "registered loopback endpoint should be accepted"
    );
}
