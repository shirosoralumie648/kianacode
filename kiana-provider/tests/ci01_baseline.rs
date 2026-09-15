use kiana_domain::{redact_text, redact_value};
use kiana_provider::{ProviderConfig, ProviderGateway};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, OnceLock};

const ENV_KEYS: &[&str] = &[
    "KIANA_PROVIDER",
    "KIANA_MODEL",
    "KIANA_OLLAMA_MODEL",
    "KIANA_OLLAMA_BASE_URL",
    "KIANA_MODEL_PROFILES_JSON",
    "KIANA_STREAMING",
    "KIANA_MODEL_MAX_CONCURRENCY",
    "OPENAI_API_KEY",
    "CI01_PROFILE_KEY",
];
const SENTINEL: &str = "ci01-api-key-sentinel";

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct EnvSnapshot(Vec<(&'static str, Option<OsString>)>);

impl EnvSnapshot {
    fn capture() -> Self {
        let values = ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        for key in ENV_KEYS {
            std::env::remove_var(key);
        }
        Self(values)
    }

    fn set(key: &'static str, value: &str) {
        std::env::set_var(key, value);
    }
}

impl Drop for EnvSnapshot {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn route<'a>(catalog: &'a Value, profile: &str) -> &'a Value {
    catalog["connections"]
        .as_array()
        .expect("catalog connections array")
        .iter()
        .find(|connection| connection["route"]["profile"] == profile)
        .map(|connection| &connection["route"])
        .expect("profile route")
}

#[test]
fn explicit_provider_config_wins_over_environment_and_builtin_defaults() {
    let _lock = env_lock();
    let _env = EnvSnapshot::capture();
    EnvSnapshot::set("KIANA_PROVIDER", "ollama");
    EnvSnapshot::set("KIANA_OLLAMA_MODEL", "environment-model");
    EnvSnapshot::set("KIANA_OLLAMA_BASE_URL", "http://localhost:11434");

    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("explicit-model".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: Some(SENTINEL.to_owned()),
    })
    .expect("explicit local provider config should be accepted");
    let catalog = gateway.catalog();
    let route = route(&catalog, "default");

    assert_eq!(route["provider_id"], "ollama");
    assert_eq!(route["model_id"], "explicit-model");
    assert_eq!(route["streaming"], true);
    assert!(!catalog.to_string().contains(SENTINEL));
    assert!(!format!(
        "{:?}",
        ProviderConfig {
            provider: Some("ollama".to_owned()),
            model: Some("explicit-model".to_owned()),
            base_url: Some("http://127.0.0.1:22114".to_owned()),
            api_key: Some(SENTINEL.to_owned()),
        }
    )
    .contains(SENTINEL));
}

#[test]
fn profile_fixture_preserves_profile_precedence_and_rejects_unknown_fields() {
    let _lock = env_lock();
    let _env = EnvSnapshot::capture();
    EnvSnapshot::set("KIANA_PROVIDER", "ollama");
    EnvSnapshot::set("KIANA_OLLAMA_MODEL", "environment-default");
    EnvSnapshot::set(
        "KIANA_MODEL_PROFILES_JSON",
        r#"{"planning":{"provider":"ollama","model":"profile-model","base_url":"http://127.0.0.1:22115","inherit_default":false}}"#,
    );

    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("explicit-default".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    })
    .expect("profile fixture should parse");
    let catalog = gateway.catalog();
    let planning = route(&catalog, "planning");
    assert_eq!(planning["model_id"], "profile-model");
    assert_eq!(planning["profile"], "planning");

    EnvSnapshot::set(
        "KIANA_MODEL_PROFILES_JSON",
        r#"{"planning":{"provider":"ollama","model":"profile-model","unknown":true}}"#,
    );
    let error = match ProviderGateway::from_env(ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("explicit-default".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    }) {
        Ok(_) => panic!("unknown profile fields must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "model_profile_config_invalid");
}

#[test]
fn channel_fixture_redacts_secret_sentinel_without_redacting_secret_refs() {
    let _lock = env_lock();
    let _env = EnvSnapshot::capture();
    let channels = [
        (
            "debug",
            format!(
                "{:?}",
                ProviderConfig {
                    provider: Some("ollama".to_owned()),
                    model: None,
                    base_url: None,
                    api_key: Some(SENTINEL.to_owned()),
                }
            ),
        ),
        ("error", format!("provider_failed token={SENTINEL}")),
        (
            "event",
            json!({"api_key": SENTINEL, "secret_ref": "vault://ci01"}).to_string(),
        ),
        (
            "receipt",
            json!({"error": format!("Authorization: Bearer {SENTINEL}")}).to_string(),
        ),
        ("argv", format!("--api_key={SENTINEL}")),
        ("env", format!("OPENAI_API_KEY={SENTINEL}")),
        ("cache", json!({"secret": SENTINEL}).to_string()),
    ];

    for (channel, raw) in channels {
        let redacted = serde_json::from_str::<Value>(&raw)
            .map(|value| redact_value(&value).to_string())
            .unwrap_or_else(|_| redact_text(&raw));
        assert!(
            !redacted.contains(SENTINEL),
            "secret sentinel survived {channel} redaction"
        );
    }
    let ref_value = redact_value(&json!({"secret_ref": "vault://ci01"}));
    assert_eq!(ref_value["secret_ref"], "vault://ci01");
}
