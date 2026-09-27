#[test]
fn dep12_daemon_route_only_delegates_admission_to_core() {
    let source = include_str!("../src/deployment_admission.rs");
    for marker in [
        "DeploymentAdmissionInput",
        "DeploymentAdmissionDecision",
        "kiana_core::evaluate_admission",
        "PortError::Failed",
    ] {
        assert!(
            source.contains(marker),
            "DEP-12 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::",
        "http::StatusCode",
    ] {
        assert!(
            !source.contains(forbidden),
            "DEP-12 daemon admission route crossed forbidden boundary: {forbidden}"
        );
    }
}
