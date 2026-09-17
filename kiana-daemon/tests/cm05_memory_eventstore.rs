#[test]
fn memory_commit_has_no_unjournaled_visibility() {
    let source = include_str!("../src/harness_memory.rs");
    let write = source
        .split("fn write_record_with_mutation")
        .nth(1)
        .expect("write path");
    let write_journal = write
        .find("journal_memory_fact(")
        .expect("write journals before projection");
    let write_append = write
        .find("append_record_file(&mut file, &record)")
        .expect("write projection append");
    assert!(write_journal < write_append);

    let review = source
        .split("fn review_records_with_mutation")
        .nth(1)
        .expect("review path");
    let review_journal = review
        .find("journal_memory_fact(")
        .expect("review journals before projection");
    let review_append = review
        .find("append_record_file(&mut file, &record)")
        .expect("review projection append");
    assert!(review_journal < review_append);

    for marker in [
        "MEMORY_FACT_EVENT_KIND",
        "MEMORY_STREAM",
        "MemoryBodyRef",
        "MemoryJournalFact",
        "append_idempotent_expected",
        "memory_journal_idempotency_payload_mismatch",
        "ensure_memory_projection",
        "memory_projection_unjournaled",
        "memory_projection_lag",
        "memory_proposal_event_journal_required",
        "MemoryMutationLedger",
        "read_records_file",
        "project_memory_facts",
        "projection_cursor",
    ] {
        assert!(
            source.contains(marker),
            "memory journal marker missing: {marker}"
        );
    }
    assert!(!source
        .contains("append_record_file(&mut file, &record)?;\n        let _ = journal_memory_fact"));
}

#[test]
fn projection_rebuild_matches_committed_memory() {
    let domain = include_str!("../../kiana-domain/src/memory_journal.rs");
    let source = include_str!("../src/harness_memory.rs");
    let fixture = include_str!("../../kiana-domain/tests/cm05_memory_eventstore.rs");
    for marker in [
        "project_memory_facts",
        "MemoryProjection",
        "matches_records",
        "source_cursor",
        "MEMORY_FACT_EVENT_KIND",
        "stream_version",
        "body_ref",
        "memory_body_ref_hash_mismatch",
        "memory_stream_version_invalid",
        "projection_rebuild_matches_committed_memory",
    ] {
        assert!(
            domain.contains(marker) || source.contains(marker) || fixture.contains(marker),
            "projection marker missing: {marker}"
        );
    }
    assert!(domain.contains("BTreeMap<String, Value>"));
    assert!(domain.contains("json_digest"));
    assert!(source.contains("memory_projection_lag"));
}
