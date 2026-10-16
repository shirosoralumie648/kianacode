#[test]
fn config_resolver_is_the_only_product_parser() {
    let daemon = include_str!("../src/model_client.rs");
    let provider = include_str!("../../kiana-provider/src/resolver.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/config-credentials-identity-baseline.md");
    let fixture = include_str!("../../kiana-provider/tests/ci06_config_resolver.rs");
    for marker in [
        "ConfigResolver",
        "WORKSPACE_CONFIG_SCHEMA",
        "parse_workspace",
        "workspace_snapshot",
        "ConfigSnapshot",
        "ProviderConfigSnapshot",
        "configuration_revision",
        "contains_forbidden_provider_config_control",
        "config_workspace_untrusted",
        "config_workspace_schema_unsupported",
        "config_workspace_invalid",
        "config_endpoint_credentials_or_query_denied",
        "config_endpoint_requires_tls_or_loopback",
        "config_profile_invalid",
        "MAX_WORKSPACE_CONFIG_BYTES",
        "deny_unknown_fields",
        "legacy_fixtures",
        "config_resolver_produces_a_canonical_secret_free_snapshot",
    ] {
        assert!(
            daemon.contains(marker)
                || provider.contains(marker)
                || gateway.contains(marker)
                || baseline.contains(marker)
                || fixture.contains(marker),
            "config resolver marker missing: {marker}"
        );
    }
    assert!(gateway.contains("resolver::ConfigResolver::resolve(config)"));
    assert!(daemon.contains("#[cfg(test)]\nmod legacy_fixtures"));
    assert!(provider.contains("project_trusted"));
    assert!(provider.contains("parse_bounded_json(raw.as_bytes())"));
    assert!(provider.contains("serde_json::from_value(value)"));
    assert!(provider.contains("contains_forbidden_provider_config_control"));
    assert!(!gateway.contains("config::connections(config)"));
    assert!(!provider.contains("api_key: String"));
}

#[test]
fn local_model_config_debug_redacts_key_and_endpoint_credentials() {
    let config = kiana_daemon::LocalModelConfig {
        provider: Some("ollama".to_owned()),
        model: Some("local-diagnostic-model".to_owned()),
        api_key: Some("CI06_DEBUG_KEY_SENTINEL".to_owned()),
        base_url: Some(
            "https://CI06_USERINFO_SENTINEL@example.invalid/api?token=CI06_QUERY_SENTINEL#CI06_FRAGMENT_SENTINEL"
                .to_owned(),
        ),
    };

    for formatted in [format!("{config:?}"), format!("{config:#?}")] {
        for sentinel in [
            "CI06_DEBUG_KEY_SENTINEL",
            "CI06_USERINFO_SENTINEL",
            "CI06_QUERY_SENTINEL",
            "CI06_FRAGMENT_SENTINEL",
        ] {
            assert!(
                !formatted.contains(sentinel),
                "Debug must not expose a configured credential"
            );
        }
        assert!(formatted.contains("ollama"));
        assert!(formatted.contains("local-diagnostic-model"));
        assert!(formatted.contains("api_key"));
        assert!(formatted.contains("base_url"));
        assert!(formatted.contains("[REDACTED]"));
        assert!(!formatted.contains("https://"));
    }
}

#[test]
fn local_model_config_debug_preserves_unconfigured_options() {
    let config = kiana_daemon::LocalModelConfig::default();

    for formatted in [format!("{config:?}"), format!("{config:#?}")] {
        for field in ["provider", "model", "api_key", "base_url"] {
            assert!(
                formatted.contains(&format!("{field}: None")),
                "Debug must distinguish an unconfigured option"
            );
        }
        assert!(!formatted.contains("[REDACTED]"));
    }
}

#[test]
fn local_model_config_debug_marks_empty_sensitive_values_as_configured() {
    let config = kiana_daemon::LocalModelConfig {
        api_key: Some(String::new()),
        base_url: Some(String::new()),
        ..kiana_daemon::LocalModelConfig::default()
    };

    for formatted in [format!("{config:?}"), format!("{config:#?}")] {
        assert!(!formatted.contains("api_key: None"));
        assert!(!formatted.contains("base_url: None"));
        assert_eq!(formatted.matches("[REDACTED]").count(), 2);
    }
}
