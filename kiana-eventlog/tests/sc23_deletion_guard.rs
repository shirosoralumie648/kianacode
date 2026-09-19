#[test]
fn sc23_eventlog_keeps_tombstone_manifest_and_unknown_boundaries() {
    let source = include_str!("../src/retention_store.rs");
    for marker in [
        "append_deletion_tombstone",
        "append_deletion_manifest",
        "deletion_tombstone_digest_conflict",
        "deletion_data_epoch_regressed",
        "deletion_tombstone_missing",
        "deletion_manifest_tombstone_boundary_mismatch",
    ] {
        assert!(
            source.contains(marker),
            "missing SC-23 eventlog marker: {marker}"
        );
    }
    assert!(!source.contains("fs::remove_file"));
}
