#[test]
fn billing_recovery_keeps_fence_and_reconciliation_visible() {
    let domain = include_str!("../../kiana-domain/src/billing_recovery.rs");
    let core = include_str!("../src/billing_recovery.rs");
    for marker in [
        "BillingRecoveryFact",
        "reservation_digest",
        "source_cursor",
        "authority_epoch",
        "lease_epoch",
        "NeedsReconciliation",
        "continue_resets_usage",
        "settlement_count",
        "validate_billing_recovery",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "BQ-21 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "auto_success",
        "release_reservation",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "BQ-21 boundary executes effects: {forbidden}"
        );
    }
}
