//! DEP-27 source guard for the ordered migration registry contract.

#[test]
fn migration_registry_is_checksum_and_release_bound() {
    let source = include_str!("../../kiana-domain/src/migration_registry.rs");
    let baseline = include_str!("../../docs/roadmap/dep27-migration-registry-baseline.md");
    for marker in [
        "MIGRATION_REGISTRY_SCHEMA",
        "MigrationReleaseBinding",
        "signature_digest",
        "MigrationCompatibilityWindow",
        "MigrationPrecondition",
        "MigrationStep",
        "MigrationRegistry",
        "deny_unknown_fields",
        "ordered_steps",
        "validate_for_release",
        "migration_registry_digest_mismatch",
        "migration_registry_version_gap",
        "from_format_version >= self.to_format_version",
        "backup_required",
    ] {
        assert!(
            source.contains(marker),
            "DEP-27 source marker missing: {marker}"
        );
    }
    for marker in [
        "unknown migration",
        "checksum drift",
        "重复版本",
        "no owner",
        "backup",
        "down migration",
        "partial",
        "signature verification",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-27 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("CapabilityBroker"));
}
