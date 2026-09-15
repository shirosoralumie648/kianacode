#[test]
fn command_receipt_and_read_set_are_single_event_store_boundaries() {
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let stream = include_str!("../../kiana-eventlog/src/stream.rs");
    let baseline = include_str!("../../docs/roadmap/event-receipt-command-receipt-baseline.md");

    for marker in [
        "COMMAND_RECEIPT_SCHEMA",
        "CommandReceipt",
        "validate_against",
        "command_receipt_identity_mismatch",
        "command_receipt_cursor_invalid",
        "command_receipt_event_ids_mismatch",
        "command_receipt_read_set_missing",
        "command_receipt_version_regressed",
    ] {
        assert!(
            journal.contains(marker),
            "missing receipt contract marker {marker}"
        );
    }
    assert!(contracts.contains("kiana.command-receipt.v1"));
    assert!(ports.contains("read_command"));
    assert!(ports.contains("commit_transition"));
    assert!(dispatch.contains("CommitOutcome::Unknown"));
    assert!(dispatch.contains("commit_confirmed"));
    assert!(stream.contains("CommitOutcome::Committed"));
    assert!(stream.contains("CommittedTransition::new"));
    assert!(baseline.contains("transition_missing_dependency_is_denied"));
    assert!(baseline.contains("read_set_conflict_appends_nothing"));
    assert!(baseline.contains("unknown_commit_never_dispatches"));
    assert!(baseline.contains("read_command"));
}
