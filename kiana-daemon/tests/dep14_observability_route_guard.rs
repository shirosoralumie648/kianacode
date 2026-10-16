#[path = "../../kiana-core/tests/support/source_guard.rs"]
mod source_guard;

#[test]
fn dep14_daemon_observability_route_only_validates() {
    // The production view removes comments, so doc comments that *name* forbidden effects
    // cannot be mistaken for real calls.
    let source =
        source_guard::production_source(include_str!("../src/deployment_observability.rs"));
    for marker in [
        "LifecycleEvidenceBundle",
        "kiana_core::validate_deployment_observability",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-14 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "EventStore",
        "CapabilityBroker",
        "ProviderGateway",
        "tokio::",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-14 route effect boundary crossed: {forbidden}"
        );
    }
}
