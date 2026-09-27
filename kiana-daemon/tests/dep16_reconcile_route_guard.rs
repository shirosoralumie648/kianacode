#[test]
fn dep16_daemon_reconcile_route_only_delegates_to_core() {
    let source = include_str!("../src/deployment_reconcile.rs");
    for marker in [
        "ReconcileInput",
        "ReconcileReport",
        "evaluate_reconcile",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-16 daemon marker missing: {marker}"
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
            "DEP-16 route crossed forbidden boundary: {forbidden}"
        );
    }
}
