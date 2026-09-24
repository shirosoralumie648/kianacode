#[test]
fn billing_projector_has_one_source_and_read_only_boundary() {
    let source = include_str!("../src/billing_ledger_projector.rs");
    for marker in [
        "BillingProjectionCursor",
        "replay_fence",
        "BillingQuarantineRecord",
        "BILLING_WINDOW_MILLIS",
        "COST_LEDGER_ENTRY_EVENT",
        "COST_ALLOCATION_EVENT",
    ] {
        assert!(source.contains(marker), "BQ-20 marker missing: {marker}");
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "commit_transition",
        "append(",
        "release_reservation",
        "ApprovalStore",
    ] {
        assert!(
            !source.contains(forbidden),
            "query boundary widened: {forbidden}"
        );
    }
}
