#[test]
fn dep15_daemon_route_only_delegates_to_core() {
    let source = include_str!("../src/ops_diagnostics.rs");
    for marker in [
        "OpsDiagnosticsInput",
        "OpsDiagnosticsReport",
        "evaluate_ops_diagnostics",
        "evaluate_ops_mode",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-15 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "ProviderGateway",
        "tokio::",
        "http::StatusCode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-15 route crossed forbidden boundary: {forbidden}"
        );
    }
}
