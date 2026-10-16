#[path = "../../kiana-core/tests/support/source_guard.rs"]
mod source_guard;

#[test]
fn dep11_daemon_health_route_only_delegates_to_core() {
    let source = source_guard::production_source(include_str!("../src/health_aggregation.rs"));
    for marker in [
        "HealthAggregationInput",
        "HealthAggregationReport",
        "kiana_core::aggregate_health",
        "input.probe = probe",
    ] {
        assert!(
            source.contains(marker),
            "DEP-11 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "EventStore",
        "CapabilityBroker",
        "ProviderGateway",
        "KianaHarness",
        "tokio::",
        "http::StatusCode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-11 daemon route crossed a forbidden boundary: {forbidden}"
        );
    }
}
