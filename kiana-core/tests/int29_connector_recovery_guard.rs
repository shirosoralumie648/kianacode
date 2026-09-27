#[test]
fn connector_recovery_keeps_paused_unknown_and_fence_boundaries() {
    let domain = include_str!("../../kiana-domain/src/connector_recovery.rs");
    let core = include_str!("../src/connector_recovery.rs");
    for marker in [
        "ConnectorRecoveryFact",
        "projection_cursor",
        "source_generation",
        "pending_unknown_count",
        "pending_reconciliation_count",
        "current_worker_epoch",
        "current_lease_epoch",
        "default_paused",
        "ReadyForAdmission",
        "validate_connector_recovery_fact",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-29 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "auto_retry",
        "resume_worker",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-29 recovery widened effect boundary: {forbidden}"
        );
    }
}
