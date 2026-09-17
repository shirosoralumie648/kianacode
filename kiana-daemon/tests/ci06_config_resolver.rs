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
    assert!(provider.contains("serde_json::from_str(raw)"));
    assert!(!gateway.contains("config::connections(config)"));
    assert!(!provider.contains("api_key: String"));
}
