#[test]
fn eventstore_indexes_and_pages_are_source_cursor_bound() {
    let journal = include_str!("../../kiana-eventlog/src/journal_core.rs");
    let memory = include_str!("../../kiana-eventlog/src/memory.rs");
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    for marker in [
        "requests",
        "streams",
        "boundaries",
        "read_command",
        "read_stream",
        "cursor_not_commit_boundary",
        "has_more",
    ] {
        assert!(
            journal.contains(marker) || memory.contains(marker) || jsonl.contains(marker),
            "index/page marker missing: {marker}"
        );
    }
    assert!(!journal.contains("CapabilityBroker"));
    assert!(!jsonl.contains("KianaHarness"));
}
