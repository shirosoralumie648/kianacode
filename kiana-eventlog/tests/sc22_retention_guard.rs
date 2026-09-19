#[test]
fn sc22_eventlog_adapter_keeps_scan_receipt_and_delete_boundaries_explicit() {
    let source = include_str!("../src/retention_store.rs");
    for marker in [
        "append_scan",
        "append_hold_receipt",
        "retention_scan_cursor_regressed",
        "retention_scan_digest_conflict",
        "retention_tombstone_deferred_to_sc23",
        "retention_purge_deferred_to_sc23",
        "source_cursor",
        "projection_cursor",
    ] {
        assert!(
            source.contains(marker),
            "missing SC-22 eventlog marker: {marker}"
        );
    }
    assert!(!source.contains("remove_file"));
}
