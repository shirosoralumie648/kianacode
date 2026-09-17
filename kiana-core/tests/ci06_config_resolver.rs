#[test]
fn config_resolver_has_a_trust_and_revision_fence() {
    let resolver = include_str!("../../kiana-provider/src/resolver.rs");
    let provider = include_str!("../../kiana-provider/src/config.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/identity_contracts.rs");
    for marker in [
        "ConfigResolver",
        "WorkspaceConfig",
        "WorkspaceProfile",
        "WORKSPACE_CONFIG_SCHEMA",
        "MAX_WORKSPACE_CONFIG_BYTES",
        "config_workspace_untrusted",
        "config_workspace_schema_unsupported",
        "deny_unknown_fields",
        "validate_endpoint",
        "validate_env_ref",
        "ConfigSnapshot::new",
        "config_revision",
        "project_trust_revision",
        "configuration_snapshot",
        "config::snapshot",
        "config_resolver_has_a_trust_and_revision_fence",
    ] {
        assert!(
            resolver.contains(marker)
                || provider.contains(marker)
                || gateway.contains(marker)
                || domain.contains(marker),
            "config resolver boundary marker missing: {marker}"
        );
    }
    assert!(resolver.contains("if !project_trusted"));
    assert!(resolver.contains("serde_json::from_str(raw)"));
    assert!(resolver.contains("reqwest::Url::parse"));
    assert!(domain.contains("pub struct ConfigSnapshot"));
    assert!(!resolver.contains("ModelClient"));
    assert!(!resolver.contains("CapabilityBroker"));
}
