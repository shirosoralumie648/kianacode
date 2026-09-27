#[test]
fn dep18_daemon_incident_route_only_delegates_to_core() {
    let source = include_str!("../src/deployment_incident.rs");
    for marker in [
        "IncidentInput",
        "IncidentReport",
        "evaluate_incident",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-18 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::process",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
        "send_alert",
        "http::StatusCode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-18 route crossed forbidden boundary: {forbidden}"
        );
    }
}
