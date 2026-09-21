#[test]
fn daemon_memory_path_binds_mutation_to_journal_and_server_authority() {
    let source = include_str!("../src/harness_memory.rs");
    assert!(source.contains("MemoryMutationAuthority::Human"));
    assert!(source.contains("MemoryMutationJournalStage::Candidate"));
    assert!(source.contains("MemoryMutationJournalStage::Approve"));
    assert!(source.contains("MemoryMutationJournalStage::Tombstone"));
    assert!(source.contains("with_mutation(mutation.clone(), stage)"));
    assert!(source.contains("memory_mutation_journal_missing"));
}
