#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn dep12_admission_reducer_is_pure_and_explicit() {
    let domain = include_str!("../../kiana-domain/src/deployment_admission.rs");
    let core = include_str!("../src/deployment_admission.rs");
    let daemon = include_str!("../../kiana-daemon/src/deployment_admission.rs");
    for marker in [
        "MaintenanceWindow",
        "DeploymentAdmissionInput",
        "DeploymentAdmissionDecision",
        "maintenance_window_expired",
        "draining_no_new_admission",
        "admission_maintenance_conflict",
        "admission_lease_conflict",
        "allow_new_work",
        "allow_existing_work",
        "evaluate_admission",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || daemon.contains(marker),
            "DEP-12 marker missing: {marker}"
        );
    }
    let domain_code = source_guard::production_source(domain);
    let core_code = source_guard::production_source(core);
    let daemon_code = source_guard::production_source(daemon);
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "tokio::",
        "EventStore",
        "CapabilityBroker",
        "KianaHarness",
        "http::StatusCode",
        "StatusCode::OK",
        "extend_window",
    ] {
        assert!(
            !domain_code.contains(forbidden)
                && !core_code.contains(forbidden)
                && !daemon_code.contains(forbidden),
            "DEP-12 admission path crossed forbidden boundary: {forbidden}"
        );
    }
}
