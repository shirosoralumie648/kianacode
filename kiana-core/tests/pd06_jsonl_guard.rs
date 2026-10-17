#[test]
fn pd06_keeps_jsonl_as_eventstore_fact_source_with_fail_closed_recovery() {
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "eventlog_sync_failed",
        "eventlog_directory_sync_failed",
        "eventlog_file_replaced",
        "eventlog_committed_prefix_truncated",
        "eventlog_legacy_writer_after_upgrade",
        "event_store_atomic_transitions_unsupported",
        "parse_json_value_reject_duplicates",
        "StrictValueSeed",
        "eventlog_parent_changed",
        "eventlog_symlink_rejected",
        "eventlog_hardlink_rejected",
        "eventlog_disk_limit",
        "eventlog_worker_failed",
        "eventlog_worker_queue_full",
        "eventlog_closed",
        "eventlog_file_removed",
    ] {
        assert!(
            jsonl.contains(marker) || ports.contains(marker),
            "PD-06 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "reqwest",
        "tokio::spawn",
    ] {
        assert!(
            !jsonl.contains(forbidden),
            "JSONL fact source widened: {forbidden}"
        );
    }
}
