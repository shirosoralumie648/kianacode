#[test]
fn connector_unknowns_are_quarantined_and_reconciled_by_successor_evidence() {
    let reconciliation = include_str!("../../kiana-domain/src/connector_reconciliation.rs");
    let observation = include_str!("../../kiana-domain/src/effect_observation.rs");
    let receipt = include_str!("../../kiana-domain/src/connectors.rs");
    let daemon = include_str!("../../kiana-daemon/src/connectors.rs");

    for marker in [
        "ConnectorReconciliationCase",
        "ConnectorReconciliationEvidence",
        "ConnectorReconciliationSource",
        "ProviderQuery",
        "ManualEvidence",
        "ConnectorHumanInboxItem",
        "AutomaticRetry",
        "ReplaceOriginalReceipt",
        "original_receipt_digest",
        "connector_reconciliation_evidence_binding_mismatch",
        "connector_reconciliation_unknown_evidence_forbidden",
        "commit_reconciled",
        "reconciliation_required",
        "ConnectorReconciliationCase::from_unknown",
        "reconciliation_case",
        "connector.reconciled",
    ] {
        assert!(
            reconciliation.contains(marker)
                || observation.contains(marker)
                || receipt.contains(marker)
                || daemon.contains(marker),
            "INT-22 marker missing: {marker}"
        );
    }

    assert!(reconciliation.contains("automatic_retry_allowed: false"));
    assert!(reconciliation.contains("ConnectorReconciliationState::Pending"));
    assert!(reconciliation.contains("ConnectorReconciliationState::Reconciled"));
    assert!(daemon.contains("ConnectorReconciliationSource::ManualEvidence"));
    assert!(daemon.contains("commit_reconciled"));
    assert!(!reconciliation.contains("CapabilityBroker"));
    assert!(!reconciliation.contains("EventStorePort"));
    assert!(!reconciliation.contains("authorize_and_execute"));
}
