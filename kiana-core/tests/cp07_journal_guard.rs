#[test]
fn cp07_keeps_complete_frame_recovery_and_does_not_add_a_second_store() {
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let core = include_str!("../src/dispatch.rs");
    for marker in [
        "JournalFrame",
        "JournalFramePayload",
        "body_sha256",
        "logical_events",
        "journal_frame_integrity_failed",
        "eventlog_legacy_writer_after_upgrade",
        "eventlog_repair_failed",
        "eventlog_recovery_sync_failed",
    ] {
        assert!(
            journal.contains(marker) || jsonl.contains(marker),
            "CP-07 marker missing: {marker}"
        );
    }
    assert!(jsonl.contains("commit_transition"));
    assert!(jsonl.contains("sync_all"));
    assert!(core.contains("commit_confirmed"));
    for forbidden in ["second EventLog", "new model loop", "CapabilityBroker::new"] {
        assert!(
            !jsonl.contains(forbidden),
            "CP-07 storage must not contain {forbidden}"
        );
    }
}
