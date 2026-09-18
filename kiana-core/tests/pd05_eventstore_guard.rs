#[test]
fn eventstore_adapter_conformance_keeps_atomic_and_unknown_boundaries() {
    let memory = include_str!("../../kiana-eventlog/src/memory.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "supports_atomic_transitions",
        "commit_transition",
        "read_command",
        "read_from",
        "CommitOutcome::Replayed",
        "CommitOutcome::Conflict",
        "event_store_atomic_transitions_unsupported",
    ] {
        assert!(
            memory.contains(marker) || jsonl.contains(marker) || ports.contains(marker),
            "eventstore marker missing: {marker}"
        );
    }
    assert!(jsonl.contains("write_frame"));
    assert!(jsonl.contains("confirm_sync"));
    assert!(!memory.contains("reqwest"));
    assert!(!jsonl.contains("CapabilityBroker"));
}
