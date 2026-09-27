#[test]
fn dep10_daemon_route_only_composes_core_evidence() {
    let source = include_str!("../src/startup_coordinator.rs");
    for marker in [
        "StartupCoordinatorRequest",
        "StartupCoordinatorReport",
        "kiana_core::evaluate_startup",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-10 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
        "std::process",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-10 daemon route contains forbidden effect: {forbidden}"
        );
    }
}
