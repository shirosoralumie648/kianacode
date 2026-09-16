use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, EvidenceStatus, MemoryCollection, MemoryMutation,
    MemoryMutationLedger, MemoryMutationOperation, MemoryMutationOutcome, MemoryMutationTarget,
    MemoryScope, ProjectIdentity, Purpose, SourceKind, SourceRef,
};
use serde_json::json;

fn scope() -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new("/tmp/cm04", "/tmp/cm04", Some(1), Some(2), "trust").unwrap(),
        "cm04-session",
        vec![MemoryCollection::parse("project:code").unwrap()],
        Purpose {
            id: "memory.mutation".to_owned(),
            description: "CM-04 fixture".to_owned(),
        },
        true,
    )
    .unwrap()
}

fn evidence() -> Vec<SourceRef> {
    vec![SourceRef::new(
        "event:cm04",
        SourceKind::Event,
        "event://cm04",
        "cursor:1",
        json_digest(&json!({"fixture": "cm04"})),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap()]
}

fn mutation(
    operation: MemoryMutationOperation,
    record_id: &str,
    expected_revision: u64,
    key: &str,
    payload: &str,
) -> MemoryMutation {
    MemoryMutation::new(
        format!("mutation:{key}"),
        operation,
        "local-user",
        scope(),
        vec![MemoryMutationTarget::new(
            record_id,
            "project:code",
            expected_revision,
        )],
        evidence(),
        1,
        1,
        key,
        &json!({"text": payload}),
    )
    .unwrap()
}

#[test]
fn duplicate_memory_mutation_returns_original_receipt() {
    let mut ledger = MemoryMutationLedger::new();
    let first = mutation(
        MemoryMutationOperation::Add,
        "memory-1",
        0,
        "cm04:add",
        "one",
    );
    let committed = ledger.apply(first.clone()).unwrap();
    let replayed = ledger.apply(first).unwrap();
    let MemoryMutationOutcome::Committed { receipt } = committed else {
        panic!("first mutation must commit")
    };
    let MemoryMutationOutcome::Replayed { original } = replayed else {
        panic!("same mutation must replay")
    };
    assert_eq!(original, receipt);
    assert_eq!(ledger.current_revision("memory-1", "project:code"), 1);
}

#[test]
fn stale_revision_never_last_write_wins() {
    let mut ledger = MemoryMutationLedger::new();
    ledger
        .apply(mutation(
            MemoryMutationOperation::Add,
            "memory-1",
            0,
            "cm04:add",
            "one",
        ))
        .unwrap();
    ledger
        .apply(mutation(
            MemoryMutationOperation::Update,
            "memory-1",
            1,
            "cm04:update:current",
            "two",
        ))
        .unwrap();
    let stale = mutation(
        MemoryMutationOperation::Update,
        "memory-1",
        1,
        "cm04:update:stale",
        "three",
    );
    let error = ledger.apply(stale).unwrap_err();
    assert!(
        error.starts_with("memory_mutation_revision_conflict:"),
        "{error}"
    );
    assert_eq!(ledger.current_revision("memory-1", "project:code"), 2);
}

#[test]
fn batch_preflight_rejects_one_stale_target_without_advancing_the_other() {
    let mut ledger = MemoryMutationLedger::new();
    ledger.seed_record("memory-a", "project:code", 1).unwrap();
    ledger.seed_record("memory-b", "project:code", 2).unwrap();
    let mutation = MemoryMutation::new(
        "mutation:batch",
        MemoryMutationOperation::Update,
        "local-user",
        scope(),
        vec![
            MemoryMutationTarget::new("memory-a", "project:code", 1),
            MemoryMutationTarget::new("memory-b", "project:code", 1),
        ],
        evidence(),
        1,
        1,
        "cm04:batch",
        &json!({"batch": true}),
    )
    .unwrap();
    assert!(ledger.apply(mutation).is_err());
    assert_eq!(ledger.current_revision("memory-a", "project:code"), 1);
    assert_eq!(ledger.current_revision("memory-b", "project:code"), 2);
}
