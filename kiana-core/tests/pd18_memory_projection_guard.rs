#[test]
fn governance_projection_rechecks_memory_epoch_contract() {
    let core = include_str!("../src/data_governance.rs");
    let domain = include_str!("../../kiana-domain/src/memory_projection.rs");
    assert!(core.contains("MemoryProjectionFence::validate_epoch"));
    assert!(domain.contains("memory_projection_data_epoch_mismatch"));
    assert!(domain.contains("project_scope_denied"));
    assert!(domain.contains("source_revoked_or_retention_expired"));
}

/// PD-18: the governance projector must rebuild the read-time deletion index from the same event
/// history, bind it to the current epoch, and never report a derived retrieval surface as usable
/// while its tombstone receipt is still missing.
#[test]
fn governance_projection_rebuilds_and_binds_memory_deletion_index() {
    let core = include_str!("../src/data_governance.rs");
    let domain = include_str!("../../kiana-domain/src/memory_projection.rs");
    let index = include_str!("../../kiana-domain/src/memory_deletion_index.rs");
    for marker in [
        "project_memory_deletion_index",
        "apply_memory_deletion_surface_states",
        "MemoryDeletionEntry::new",
        "merge_memory_deletion_entries",
    ] {
        assert!(core.contains(marker), "missing core marker: {marker}");
    }
    for marker in [
        "deletion_index_digest",
        "retention_expired",
        "deletion_index_project_mismatch",
        "memory_deletion_index_epoch_stale",
    ] {
        assert!(domain.contains(marker), "missing fence marker: {marker}");
    }
    for marker in [
        "MEMORY_DELETION_INDEX_SCHEMA",
        "memory_deletion_index_digest_mismatch",
        "memory_deletion_surface_evidence_required",
    ] {
        assert!(index.contains(marker), "missing index marker: {marker}");
    }
    // A propagation receipt is evidence, not an assumption: the pending state must stay Unknown.
    assert!(index.contains("Unknown"));
    assert!(!index.contains("Invalidated => true"));
}
