#[test]
fn dep13_daemon_shutdown_route_only_delegates_to_core() {
    let source = include_str!("../src/deployment_shutdown.rs");
    for marker in [
        "ShutdownInput",
        "ShutdownReport",
        "kiana_core::evaluate_shutdown",
    ] {
        assert!(
            source.contains(marker),
            "DEP-13 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-13 daemon shutdown route crossed forbidden boundary: {forbidden}"
        );
    }
}
