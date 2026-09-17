use kiana_domain::{json_digest, ModelError};
use kiana_provider::{ConfigResolver, ProviderConfig, ProviderGateway};
use serde_json::json;

fn valid_workspace() -> String {
    json!({
        "schema": "kiana.provider-config.v1",
        "version": 1,
        "provider": "ollama",
        "model": "fixture-model",
        "base_url": "http://127.0.0.1:22114",
        "api_key_env": "OLLAMA_OPTIONAL_KEY",
        "profiles": {
            "planning": {
                "provider": "ollama",
                "model": "planning-model",
                "base_url": "http://localhost:22115",
                "inherit_default": false
            }
        }
    })
    .to_string()
}

fn code(result: Result<kiana_provider::WorkspaceConfig, ModelError>) -> String {
    result.unwrap_err().code
}

#[test]
fn config_resolver_rejects_untrusted_and_invalid_workspace_inputs() {
    let raw = valid_workspace();
    assert_eq!(
        code(ConfigResolver::parse_workspace(&raw, false)),
        "config_workspace_untrusted"
    );

    let mut unknown = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    unknown["unexpected"] = json!(true);
    assert_eq!(
        code(ConfigResolver::parse_workspace(&unknown.to_string(), true)),
        "config_workspace_invalid"
    );

    let mut major = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    major["version"] = json!(2);
    assert_eq!(
        code(ConfigResolver::parse_workspace(&major.to_string(), true)),
        "config_workspace_schema_unsupported"
    );

    let mut endpoint = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    endpoint["base_url"] = json!("http://user:pass@example.invalid/v1?token=raw");
    assert_eq!(
        code(ConfigResolver::parse_workspace(&endpoint.to_string(), true)),
        "config_endpoint_credentials_or_query_denied"
    );

    let mut profile = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    profile["profiles"]["future"] = json!({
        "provider": "ollama",
        "model": "future-model"
    });
    assert_eq!(
        code(ConfigResolver::parse_workspace(&profile.to_string(), true)),
        "config_profile_invalid"
    );

    let oversized = "x".repeat(kiana_provider::MAX_WORKSPACE_CONFIG_BYTES + 1);
    assert_eq!(
        code(ConfigResolver::parse_workspace(&oversized, true)),
        "config_workspace_too_large"
    );
}

#[test]
fn config_resolver_produces_a_canonical_secret_free_snapshot() {
    let trust = json_digest(&json!("trusted-project"));
    let snapshot = ConfigResolver::workspace_snapshot(&valid_workspace(), true, &trust)
        .expect("trusted workspace snapshot");
    snapshot.validate().expect("snapshot validates");
    let encoded = serde_json::to_string(&snapshot).expect("snapshot serializes");
    assert!(!encoded.contains("raw"));
    assert!(encoded.contains("api_key_env"));
    assert_eq!(
        snapshot.config_revision,
        json_digest(&snapshot.effective_non_secret_config)
    );

    let config = ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("resolver-model".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    };
    let first = ProviderGateway::from_env(config.clone()).expect("gateway via resolver");
    let second = ProviderGateway::from_env(config).expect("same gateway via resolver");
    assert_eq!(
        first.configuration_snapshot().unwrap(),
        second.configuration_snapshot().unwrap()
    );
}
