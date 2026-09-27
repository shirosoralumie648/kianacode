#[test]
fn dep14_daemon_observability_route_only_validates() {
    let source = include_str!("../src/deployment_observability.rs");
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
