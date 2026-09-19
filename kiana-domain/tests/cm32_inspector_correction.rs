use kiana_domain::{
    json_digest, memory_source_snapshot, AuthenticatedPrincipalRef, ContextMemoryInspectorSnapshot,
    EvidenceStatus, MemoryCollection, MemoryMutation, MemoryMutationOperation,
    MemoryMutationTarget, MemoryScope, ProjectIdentity, ProjectionLagView, Purpose,
    RetrievalReceipt, RetrievalReceiptEntry, RetrievalReceiptStage, SourceKind, SourceRef,
    UserMemoryCorrection,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn receipt() -> RetrievalReceipt {
    let source = memory_source_snapshot(
        "memory:1",
        "project:code",
        "revision:1",
        &digest("body"),
        EvidenceStatus::Attributed,
    )
    .unwrap();
    let entry = RetrievalReceiptEntry::new(
        "memory:1",
        RetrievalReceiptStage::Retrieved,
        source,
        digest("evidence"),
        false,
        None,
    )
    .unwrap();
    RetrievalReceipt::new(
        "receipt:cm32",
        "secret locator query",
        json_digest(&json!({"query": "secret locator query"})),
        digest("scope"),
        "fixture.v1",
        1,
        false,
        Vec::new(),
        vec![entry],
        Vec::new(),
    )
    .unwrap()
}

fn inspector() -> ContextMemoryInspectorSnapshot {
    ContextMemoryInspectorSnapshot::from_receipt(
        "inspector:cm32",
        &receipt(),
        digest("context-plan"),
        ProjectionLagView::new(1, Some(1), 1, 1, None).unwrap(),
        1,
        None,
        "active",
        "ready",
    )
    .unwrap()
}

#[test]
fn inspector_matches_receipt_without_private_leak() {
    let inspector = inspector();
    let encoded = serde_json::to_string(&inspector).unwrap();
    assert!(!encoded.contains("secret locator query"));
    assert!(!encoded.contains("project:code:memory:1"));
    assert!(encoded.contains("receipt:cm32"));
    assert!(encoded.contains("revision:1"));
    inspector.validate().unwrap();
}

#[test]
fn user_correction_requires_governed_mutation() {
    let scope = MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new(
            "/tmp/cm32-project",
            "/tmp/cm32-project",
            Some(1),
            Some(2),
            "trust",
        )
        .unwrap(),
        "session-cm32",
        vec![MemoryCollection::parse("project:code").unwrap()],
        Purpose {
            id: "memory.correction".to_owned(),
            description: "operator correction".to_owned(),
        },
        true,
    )
    .unwrap();
    let evidence = SourceRef::new(
        "memory:1",
        SourceKind::Memory,
        "project:code:memory:1",
        "revision:1",
        digest("body"),
        None,
        EvidenceStatus::Attributed,
    )
    .unwrap();
    let mutation = MemoryMutation::new(
        "mutation:cm32",
        MemoryMutationOperation::Update,
        "local-user",
        scope,
        vec![MemoryMutationTarget::new("memory:1", "project:code", 1)],
        vec![evidence],
        1,
        1,
        "idempotency:cm32",
        &json!({"text":"corrected"}),
    )
    .unwrap();
    assert!(
        UserMemoryCorrection::new("correction:cm32", &inspector(), mutation.clone(), false)
            .is_err()
    );
    let correction =
        UserMemoryCorrection::new("correction:cm32", &inspector(), mutation, true).unwrap();
    correction.validate_against(&inspector()).unwrap();
}
