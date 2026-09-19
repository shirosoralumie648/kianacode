//! DEP-28 source guard for read-only migration preflight.

#[test]
fn migration_preflight_is_matrix_bounded_and_side_effect_free() {
    let source = include_str!("../../kiana-domain/src/migration_preflight.rs");
    let baseline = include_str!("../../docs/roadmap/dep28-migration-preflight-baseline.md");
    for marker in [
        "MigrationPreflightAxis",
        "MIGRATION_PREFLIGHT_AXIS_COUNT",
        "Store",
        "Schema",
        "Projection",
        "Workflow",
        "Provider",
        "Config",
        "Space",
        "Clock",
        "Lease",
        "MigrationPreflightFacts",
        "MigrationPreflightReport",
        "read_only",
        "effect_calls",
        "migration_downgrade_denied",
        "migration_runner_concurrent",
        "migration_space_insufficient",
        "migration_active_unknown",
        "migration_backup_unverified",
        "remediation",
        "is_ready",
    ] {
        assert!(
            source.contains(marker),
            "DEP-28 source marker missing: {marker}"
        );
    }
    for marker in [
        "major mismatch",
        "downgrade",
        "并发 runner",
        "空间不足",
        "active Unknown",
        "old writer",
        "未 verified backup",
        "read-only",
        "effect_calls=0",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-28 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("std::fs"));
}
