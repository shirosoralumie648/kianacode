#[test]
fn memory_handlers_use_server_mutation_contract() {
    let source = include_str!("../src/harness_memory.rs");
    let domain = include_str!("../../kiana-domain/src/memory_mutation.rs");
    for marker in [
        "server_memory_scope",
        "MemoryMutation::new",
        "MemoryMutationLedger",
        "memory_idempotency_conflict",
        "expected_revision",
        "mutation_receipt",
    ] {
        assert!(
            source.contains(marker),
            "missing daemon mutation marker {marker}"
        );
    }
    for marker in [
        "MemoryMutationOperation::Add",
        "MemoryMutationOperation::Update",
        "MemoryMutationOperation::Delete",
        "MemoryMutationOperation::Approve",
        "MemoryMutationOperation::Publish",
        "MemoryMutationOperation::Expire",
        "MemoryMutationOperation::Revoke",
        "pub fn preflight",
        "Replayed { original",
    ] {
        assert!(
            domain.contains(marker),
            "missing domain mutation marker {marker}"
        );
    }
    let apply = source
        .find("ledger.apply(mutation)")
        .expect("mutation CAS application");
    let append = source
        .find("append_record_file(&mut file, &record)")
        .expect("record append");
    assert!(
        apply < append,
        "record append must follow mutation preflight"
    );
}
