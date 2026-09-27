#[test]
fn dep08_deployment_config_is_pure_and_secret_free() {
    let domain = include_str!("../../kiana-domain/src/deployment_config.rs");
    let core = include_str!("../src/deployment_config.rs");
    for marker in [
        "DeploymentConfigSourceKind",
        "DeploymentConfigSnapshot",
        "allowed_environment_keys",
        "project_trust_digest",
        "config_revision",
        "redacted_digest",
        "ConfigDiff",
        "ConfigChangeImpact",
        "restart_required",
        "migration_required",
        "contains_raw_secret",
        "validate_deployment_config_snapshot",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-08 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process::Command",
        "tokio::",
        "ProviderGateway",
        "CapabilityBroker",
        "EventStore::append",
        "dispatch_capability",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-08 config path crossed an effect boundary: {forbidden}"
        );
    }
}
