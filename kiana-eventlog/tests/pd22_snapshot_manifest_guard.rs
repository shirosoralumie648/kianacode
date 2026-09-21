#[test]
fn eventlog_snapshot_guard_keeps_manifest_as_metadata_not_new_fact_source() {
    let domain = include_str!("../../kiana-domain/src/snapshot_manifest.rs");
    let eventlog = include_str!("../src/lib.rs");
    assert!(domain.contains("source_cursor"));
    assert!(domain.contains("data_epoch"));
    assert!(domain.contains("content_hash"));
    assert!(eventlog.contains("EventStorePort"));
    assert!(!domain.contains("execute_capability"));
}
