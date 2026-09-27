#[test]
fn swarm_routing_reuses_the_single_execution_spine() {
    let domain = include_str!("../../kiana-domain/src/swarm_routing.rs");
    let core = include_str!("../src/swarm_routing.rs");
    for marker in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "correlation_id",
        "causation_event_id",
        "direct_runner_route",
        "direct_provider_route",
        "validate_child_route",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-08 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore",
        "dispatch_provider",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-08 route contract must not execute effects: {forbidden}"
        );
    }
}
