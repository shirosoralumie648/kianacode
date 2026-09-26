#[test]
fn er33_capacity_migration_keeps_storage_and_migration_safety_boundaries() {
    let domain = include_str!("../../kiana-domain/src/er33_capacity_migration.rs");
    let core = include_str!("../src/er33_capacity_migration.rs");
    let capacity = include_str!("../../kiana-domain/src/persistence_capacity.rs");
    let migration = include_str!("../../kiana-domain/src/migration_preflight.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    for marker in [
        "Er33CapacityMigrationDrill",
        "Er33DrillMetric",
        "Er33MigrationCheck",
        "EventFrame",
        "ProjectionRebuild",
        "ReceiptQuery",
        "QueueDepth",
        "over_quota_rejected",
        "partial_frame_appended",
        "unknown_version_rejected",
        "facts_deleted_for_capacity",
        "StorageCapacity",
        "MigrationPreflight",
        "validate_er33_capacity_migration_drill",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || capacity.contains(marker)
                || migration.contains(marker)
                || jsonl.contains(marker),
            "ER-33 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "CapabilityBroker::new",
        "ModelClient::new",
        "remove_file",
        "delete_facts",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "ER-33 destructive/effect marker present: {forbidden}"
        );
    }
}
