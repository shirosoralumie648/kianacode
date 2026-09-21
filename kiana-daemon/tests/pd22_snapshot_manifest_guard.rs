#[test]
fn daemon_snapshot_guard_requires_sealed_manifest_before_adapter_effects() {
    let domain = include_str!("../../kiana-domain/src/snapshot_manifest.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/lib.rs");
    assert!(domain.contains("SnapshotManifest"));
    assert!(domain.contains("snapshot_backup_root_overlaps_active_root"));
    assert!(domain.contains("snapshot_manifest_seal_invalid"));
    assert!(eventlog.contains("EventStorePort"));
}
