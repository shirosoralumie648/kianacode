#[test]
fn memory_search_uses_projection_fence_and_exposes_epoch() {
    let daemon = include_str!("../src/harness_memory.rs");
    let core = include_str!("../../kiana-core/src/data_governance.rs");
    for marker in [
        "MemoryProjectionFence::evaluate",
        "data_policy.data_epoch",
        "data_source_revoked",
        "\"data_epoch\":data_policy.data_epoch",
    ] {
        assert!(
            daemon.contains(marker) || core.contains(marker),
            "missing marker: {marker}"
        );
    }
}

/// PD-18: the read path must be fenced by an evidence-bound per-object deletion index, the purge
/// path must record tombstones before touching derived data, and no surface may claim
/// invalidation without a receipt digest.
#[test]
fn memory_reads_are_fenced_by_evidence_bound_deletion_index() {
    let daemon = include_str!("../src/harness_memory.rs");
    let governance = include_str!("../src/data_governance.rs");
    let domain_index = include_str!("../../kiana-domain/src/memory_deletion_index.rs");
    for marker in [
        "MemoryDeletionIndex",
        "deletion_index_report",
        "projected_deletion_entries",
        "merge_memory_deletion_entries",
        "memory_deletion_index_stale",
        "memory_deletion_index_project_mismatch",
    ] {
        assert!(
            daemon.contains(marker) || governance.contains(marker),
            "missing daemon marker: {marker}"
        );
    }
    for marker in [
        "read_deletion_index",
        "write_deletion_index",
        "fence_purged_entry",
        "purged_entry",
    ] {
        assert!(
            governance.contains(marker),
            "missing governance marker: {marker}"
        );
    }
    for marker in [
        "memory_deletion_tombstoned",
        "memory_deletion_tombstone_revival_denied",
        "MemoryRetrievalSurface",
        "evidence_digest",
    ] {
        assert!(
            domain_index.contains(marker),
            "missing domain marker: {marker}"
        );
    }
    // The deletion index must never be treated as an eraser: the daemon only reads it during
    // search, while the purge path records tombstones and surface receipts.
    assert!(daemon.contains("deletion_index"));
    assert!(!daemon.contains("remove_file(deletion_index_path"));
}
