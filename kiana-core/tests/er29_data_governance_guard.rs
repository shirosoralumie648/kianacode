//! ER-29 source guard: data governance is a projection/propagation boundary, not a second
//! authority or an EventLog rewrite path.

#[test]
fn er29_separates_receipt_metadata_from_payload_and_fences_every_store() {
    let domain = include_str!("../../kiana-domain/src/governance.rs");
    let core = include_str!("../src/data_governance.rs");
    let deletion = include_str!("../src/deletion.rs");
    let receipts = include_str!("../src/receipts.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/governance_contract.rs");
    let retention = include_str!("../../kiana-eventlog/src/retention_store.rs");
    let memory = include_str!("../../kiana-eventlog/src/memory_store.rs");
    let query = include_str!("../../kiana-query/src/index_invalidation.rs");
    let baseline = include_str!("../../docs/roadmap/er29-data-governance-baseline.md");

    for marker in [
        "ReceiptAuditMetadata",
        "ReceiptPayloadRef",
        "ReceiptDataBinding",
        "redact_payload_refs",
        "receipt_payload_authorization_denied",
        "DataPropagationPlan",
        "DataPropagationReceipt",
        "DataPropagationTarget",
        "ImmutableEventSeal",
        "immutable_event_seal",
        "data_epoch",
        "immutable_event_seal: true",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || receipts.contains(marker)
                || baseline.contains(marker),
            "ER-29 contract marker missing: {marker}"
        );
    }

    for marker in [
        "plan_deletion_propagation",
        "receipt_redaction_is_not_authorization",
        "deletion_governance_snapshot_stale",
        "receipt_data_binding_from_events",
        "seal_governance_events",
        "if pending_invalidation",
    ] {
        assert!(
            core.contains(marker) || deletion.contains(marker),
            "ER-29 core marker missing: {marker}"
        );
    }

    for marker in [
        "invalidate_artifact",
        "artifact_data_revoked_or_expired",
        "append_propagation_receipt",
        "deletion_manifest_missing",
        "MemoryDataGovernanceStore",
        "read_allowed",
        "index_data_epoch_stale",
        "governed_index_invalidation",
    ] {
        assert!(
            artifacts.contains(marker)
                || ports.contains(marker)
                || retention.contains(marker)
                || memory.contains(marker)
                || query.contains(marker),
            "ER-29 propagation marker missing: {marker}"
        );
    }

    assert!(eventlog.contains("data.propagation_receipt"));
    assert!(eventlog.contains("data.immutable_event_seal"));
    assert!(eventlog.contains("eventlog_immutable_event_seal_payload_boundary"));
    assert!(!core.contains("CapabilityBroker::new"));
    assert!(!core.contains("KianaHarness::new"));
}
