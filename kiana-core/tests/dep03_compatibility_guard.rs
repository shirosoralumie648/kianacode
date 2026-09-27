#[test]
fn dep03_compatibility_matrix_stays_pure_and_explicit() {
    let domain = include_str!("../../kiana-domain/src/deployment_compatibility.rs");
    let core = include_str!("../src/deployment_compatibility.rs");
    for marker in [
        "DeploymentVersionAxes",
        "DeploymentCompatibilityMatrix",
        "protocol_version",
        "domain_schema",
        "store_format",
        "projection_version",
        "workflow_definition_digest",
        "provider_route_digest",
        "extension_digest",
        "config_revision",
        "authority_epoch",
        "data_epoch",
        "generation",
        "protocol_major_mismatch",
        "store_format_downgrade",
        "workflow_definition_drift",
        "validate_deployment_compatibility",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-03 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "apply_migration",
        "start_revision",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-03 compatibility path crossed an effect boundary: {forbidden}"
        );
    }
}
