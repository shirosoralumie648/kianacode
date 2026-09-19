#[test]
fn projection_lag_and_unknown_mutation_share_one_recovery_boundary() {
    let domain = include_str!("../../kiana-domain/src/projection_recovery.rs");
    let receipts = include_str!("../src/receipts.rs");
    let projection = include_str!("../src/projection_checkpoint.rs");
    let memory = include_str!("../../kiana-domain/src/memory_journal.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/stream.rs");

    for marker in [
        "ProjectionLagView",
        "PROJECTION_LAG_VIEW_SCHEMA",
        "projection_pending",
        "projection_cursor_unobserved",
        "projection_cursor_lagging",
        "read_consistent",
        "UnknownMutationReconciliation",
        "unknown_mutation_new_id_forbidden",
        "unknown_mutation_request_digest_mismatch",
        "retry_with_new_id_allowed",
        "result_unknown",
        "projection_lag_from_events",
        "ProjectionCheckpoint",
        "project_memory_facts",
        "idempotency",
        "committed_cursor",
    ] {
        assert!(
            domain.contains(marker)
                || receipts.contains(marker)
                || projection.contains(marker)
                || memory.contains(marker)
                || eventlog.contains(marker),
            "CM-29 source marker missing: {marker}"
        );
    }

    assert!(receipts.contains("projection_lag_from_events(events)"));
    assert!(domain.contains("mutation_id"));
    assert!(!receipts.contains("CapabilityBroker"));
}
