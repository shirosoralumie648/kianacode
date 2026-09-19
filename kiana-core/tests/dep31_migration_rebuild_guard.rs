//! DEP-31 source guard for projection/index/receipt rebuild invariants.

#[test]
fn migration_rebuild_keeps_source_projection_index_and_receipt_bound() {
    let source = include_str!("../../kiana-domain/src/migration_rebuild.rs");
    let baseline = include_str!("../../docs/roadmap/dep31-migration-rebuild-baseline.md");
    for marker in [
        "MigrationRebuildFacts",
        "MigrationRebuildReport",
        "ready_gate",
        "source_cursor",
        "projection_cursor",
        "source_generation",
        "projection_generation",
        "index_generation",
        "receipt_generation",
        "source_replay_digest",
        "receipt_source_digest",
        "migration_projection_over_facts",
        "migration_projection_cursor_lag",
        "migration_projection_generation_stale",
        "migration_index_generation_stale",
        "migration_receipt_source_invariant_failed",
        "migration_rebuild_registry_checksum_drift",
    ] {
        assert!(
            source.contains(marker),
            "DEP-31 source marker missing: {marker}"
        );
    }
    for marker in [
        "projection 越过 facts",
        "旧 generation",
        "index 不可重建",
        "receipt 关联丢失",
        "source cursor",
        "ready gate",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-31 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
