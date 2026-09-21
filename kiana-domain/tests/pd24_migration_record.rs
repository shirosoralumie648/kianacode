use kiana_domain::{
    MigrationCompatibilityWindow, MigrationPrecondition, MigrationRecord, MigrationRecordStatus,
    MigrationRegistry, MigrationReleaseBinding, MigrationStep,
};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn registry() -> MigrationRegistry {
    MigrationRegistry::new(
        MigrationReleaseBinding {
            release_manifest_digest: digest('a'),
            artifact_digest: digest('b'),
            signature_digest: digest('c'),
        },
        vec![MigrationStep {
            step_id: "migrate-1".to_owned(),
            ordinal: 1,
            from_format_version: 1,
            to_format_version: 2,
            checksum: digest('d'),
            owner: "storage-owner".to_owned(),
            backup_required: true,
            precondition: MigrationPrecondition {
                source_schema_digest: digest('e'),
                expected_source_revision: 1,
                required_owner: "storage-owner".to_owned(),
                requires_verified_backup: true,
            },
            compatibility_window: MigrationCompatibilityWindow {
                min_reader_version: 1,
                max_reader_version: 2,
                expires_at_unix_ms: 10,
            },
            upcaster: "upcast-v1-v2".to_owned(),
        }],
    )
    .unwrap()
}

#[test]
fn migration_record_binds_registry_preflight_and_backup() {
    let registry = registry();
    let record = MigrationRecord::planned(
        &registry,
        "migration-run-1",
        digest('f'),
        "snapshot-1",
        "storage-owner",
    )
    .unwrap();
    let running = record
        .transition(&registry, MigrationRecordStatus::Running, None)
        .unwrap();
    assert_eq!(running.attempt, 1);
    let applied = running
        .transition(&registry, MigrationRecordStatus::Applied, None)
        .unwrap();
    assert_eq!(applied.status, MigrationRecordStatus::Applied);
}

#[test]
fn migration_failure_and_invalid_transition_fail_closed() {
    let registry = registry();
    let record = MigrationRecord::planned(
        &registry,
        "migration-run-1",
        digest('f'),
        "snapshot-1",
        "storage-owner",
    )
    .unwrap();
    assert_eq!(
        record
            .transition(&registry, MigrationRecordStatus::Applied, None)
            .unwrap_err(),
        "migration_record_transition_invalid"
    );
    let failed = record
        .transition(&registry, MigrationRecordStatus::Running, None)
        .unwrap()
        .transition(
            &registry,
            MigrationRecordStatus::Failed,
            Some("checksum mismatch".to_owned()),
        )
        .unwrap();
    assert_eq!(failed.status, MigrationRecordStatus::Failed);
}
