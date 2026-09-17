#[test]
fn every_failure_class_has_an_incident_and_recovery() {
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let platform = include_str!("../src/platform.rs");
    let commands = include_str!("../src/commands.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/p2-k6-01-reliability-baseline.md");

    for marker in [
        "FailureClass",
        "Crash",
        "Timeout",
        "Cancel",
        "DiskFull",
        "McpFailure",
        "ProviderUnknown",
        "RecoveryPlan",
        "FailureIncident",
        "recovery(self, unknown",
        "failure_incidents",
        "failure.incidents",
        "failure.reconcile",
        "failure.release",
        "failure.reconciled",
        "resource.quarantined",
        "resource.released",
        "requires_reconciliation",
        "automatic_retry_allowed",
        "resource_release_requires_reconciliation",
        "resource_release_stop_unconfirmed",
        "failure_reconciliation_evidence_required",
        "runtime_outcome_unchanged",
        "new_request_required",
        "result_unknown",
        "reconciliation",
        "HumanInboxKind::Reconciliation",
    ] {
        assert!(
            domain.contains(marker)
                || platform.contains(marker)
                || commands.contains(marker)
                || daemon.contains(marker)
                || baseline.contains(marker),
            "reliability marker missing: {marker}"
        );
    }

    for marker in [
        "FailureClass::Crash",
        "FailureClass::Timeout",
        "FailureClass::Cancel",
        "FailureClass::DiskFull",
        "FailureClass::McpFailure",
        "FailureClass::ProviderUnknown",
    ] {
        assert!(
            platform.contains(marker),
            "failure class is not projected: {marker}"
        );
    }
    assert!(platform.contains("failure.reconciled"));
    assert!(platform.contains("automatic_retry_allowed:false"));
    assert!(platform.contains("new_request_required:true"));
    assert!(platform.contains("!self.await_capability_stop(run_id).await"));
    assert!(!platform.contains("CapabilityBroker"));
    assert!(!platform.contains("ModelClient"));
}
