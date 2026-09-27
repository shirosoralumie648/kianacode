#[test]
fn dep17_daemon_capacity_route_only_delegates_to_core() {
    let source = include_str!("../src/deployment_capacity.rs");
    for marker in [
        "CapacityInput",
        "CapacityReport",
        "evaluate_capacity",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-17 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore",
        "ProjectionStorePort",
        "WorkflowQueueStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
        "http::StatusCode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-17 route crossed forbidden boundary: {forbidden}"
        );
    }
}
