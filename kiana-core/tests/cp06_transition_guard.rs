#[test]
fn transition_contract_owns_read_set_cas_and_command_idempotency() {
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let event_store = include_str!("../../kiana-eventlog/src/event_store_core.rs");
    let memory = include_str!("../../kiana-eventlog/src/memory.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let core = include_str!("../src/dispatch.rs");
    let baseline = include_str!("../../docs/roadmap/control-plane-transition-baseline.md");

    for marker in [
        "TransitionBatch",
        "expected_versions",
        "CommitOutcome",
        "CommandReceipt",
        "journal_read_set_duplicate",
        "journal_write_not_in_read_set",
        "journal_frame_size_limit",
        "JOURNAL_FRAME_SCHEMA",
    ] {
        assert!(journal.contains(marker), "missing journal marker {marker}");
    }
    assert!(ports.contains("commit_transition"));
    assert!(ports.contains("event_store_atomic_transitions_unsupported"));
    assert!(ports.contains("append_idempotent_expected"));
    assert!(event_store.contains("plan_transition"));
    assert!(event_store.contains("event_store_command_digest_mismatch"));
    assert!(event_store.contains("TransitionPlan::Conflict"));
    assert!(memory.contains("CommitOutcome::Replayed"));
    assert!(jsonl.contains("CommitOutcome::Unknown"));
    assert!(core.contains("commit_confirmed"));
    assert!(core.contains("CommitOutcome::Conflict"));
    assert!(core.contains("CommitOutcome::Unknown"));
    assert!(baseline.contains("cp_transition_conflict_changes_no_aggregate"));
    assert!(baseline.contains("cp_same_command_different_payload_is_conflict"));
    assert!(baseline.contains("cp_stale_allow_is_recomputed_after_cas_conflict"));
    assert!(baseline.contains("无部分状态"));
}
