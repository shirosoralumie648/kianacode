//! DEP-29 source guard for pure bounded migration primitives.

#[test]
fn migration_primitives_are_forward_only_bounded_and_effect_free() {
    let source = include_str!("../../kiana-domain/src/migration_primitives.rs");
    let baseline = include_str!("../../docs/roadmap/dep29-migration-primitives-baseline.md");
    for marker in [
        "MigrationPrimitivePhase",
        "Expand",
        "Backfill",
        "Verify",
        "Switch",
        "Contract",
        "MigrationPrimitiveTarget",
        "MigrationCheckpoint",
        "MigrationBatchPlan",
        "MAX_MIGRATION_BATCH",
        "MAX_MIGRATION_ITEMS",
        "migration_source_cursor_rollback",
        "migration_phase_order_invalid",
        "migration_verify_required_before_switch",
        "migration_idempotency_key_mismatch",
        "checked_add",
    ] {
        assert!(
            source.contains(marker),
            "DEP-29 source marker missing: {marker}"
        );
    }
    for marker in [
        "expand",
        "backfill",
        "verify",
        "switch",
        "contract",
        "bounded",
        "idempotent",
        "checkpoint",
        "provider",
        "shell",
        "MCP",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-29 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("Provider"));
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("std::fs"));
}
