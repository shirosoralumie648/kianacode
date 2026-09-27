#[test]
fn dep14_observability_path_is_redacted_and_read_only() {
    let domain = include_str!("../../kiana-domain/src/deployment_observability.rs");
    let core = include_str!("../src/deployment_observability.rs");
    let daemon = include_str!("../../kiana-daemon/src/deployment_observability.rs");
    for marker in [
        "LifecycleMetricFact",
        "LifecycleEvidenceBundle",
        "operation_ref",
        "revision_digest",
        "source_cursor",
        "secret_free",
        "validate_deployment_observability",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "DEP-14 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStore::append",
        "CapabilityBroker",
        "ProviderGateway",
        "KianaHarness",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden) && !daemon.contains(forbidden),
            "DEP-14 effect boundary crossed: {forbidden}"
        );
    }
}
