#[test]
fn daemon_migration_guard_requires_registry_preflight_backup_and_quarantine() {
    let domain = include_str!("../../kiana-domain/src/migration_record.rs");
    let registry = include_str!("../../kiana-domain/src/migration_registry.rs");
    let preflight = include_str!("../../kiana-domain/src/migration_preflight.rs");
    let runner = include_str!("../../kiana-domain/src/migration_runner.rs");
    for marker in [
        "MigrationRecord",
        "backup_snapshot_id",
        "preflight_digest",
        "migration_record_transition_invalid",
        "Quarantined",
        "registry_digest",
        "requires_verified_backup",
    ] {
        assert!(
            domain.contains(marker)
                || registry.contains(marker)
                || preflight.contains(marker)
                || runner.contains(marker),
            "missing migration marker: {marker}"
        );
    }
}
