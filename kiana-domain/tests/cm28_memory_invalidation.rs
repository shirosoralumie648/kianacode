use kiana_domain::{
    DeleteRequest, DeletionTombstone, HistoricalReceiptInvalidation, MemoryInvalidationKind,
    MemoryInvalidationPlan, MemoryPropagationState, MemoryPropagationTarget, RequestId,
};
use std::collections::BTreeSet;

fn tombstone() -> DeletionTombstone {
    let request = DeleteRequest::new(
        RequestId::new(),
        "/cm28-project",
        BTreeSet::from(["src/secret.txt".to_owned()]),
        "delete",
        "data_subject_request",
        "principal:operator",
        7,
        3,
        5,
    )
    .unwrap();
    DeletionTombstone::new(
        &request,
        "src/secret.txt",
        format!("sha256:{}", "a".repeat(64)),
        4,
    )
    .unwrap()
}

#[test]
fn deletion_propagates_to_memory_and_index() {
    let tombstone = tombstone();
    let receipt =
        HistoricalReceiptInvalidation::new("receipt:cm28", "memory:secret", "revision:4", 100)
            .unwrap();
    let plan = MemoryInvalidationPlan::from_tombstone(
        &tombstone,
        MemoryInvalidationKind::Deleted,
        100,
        vec![receipt],
    )
    .unwrap();
    plan.validate().unwrap();
    for target in [
        MemoryPropagationTarget::MemoryJsonl,
        MemoryPropagationTarget::MemoryBody,
        MemoryPropagationTarget::Bm25Index,
        MemoryPropagationTarget::DenseIndex,
        MemoryPropagationTarget::RepoIndex,
        MemoryPropagationTarget::ContextPlan,
        MemoryPropagationTarget::Summary,
        MemoryPropagationTarget::Checkpoint,
        MemoryPropagationTarget::PromptCache,
        MemoryPropagationTarget::Ui,
    ] {
        let record = plan.targets.iter().find(|record| record.target == target);
        assert_eq!(
            record.map(|record| record.state),
            Some(MemoryPropagationState::Invalidated)
        );
        assert_eq!(record.map(|record| record.reinjection_allowed), Some(false));
    }
}

#[test]
fn historical_receipt_is_preserved_but_not_reinjected() {
    let tombstone = tombstone();
    let plan = MemoryInvalidationPlan::from_tombstone(
        &tombstone,
        MemoryInvalidationKind::Revoked,
        101,
        vec![HistoricalReceiptInvalidation::new(
            "receipt:cm28",
            "memory:secret",
            "revision:4",
            101,
        )
        .unwrap()],
    )
    .unwrap();
    let historical = plan
        .targets
        .iter()
        .find(|record| record.target == MemoryPropagationTarget::HistoricalReceipt)
        .unwrap();
    assert_eq!(historical.state, MemoryPropagationState::PreservedInvalid);
    assert!(!plan.can_reinject("receipt:cm28", "memory:secret"));
    assert!(plan
        .historical_receipts
        .iter()
        .any(|receipt| receipt.receipt_id == "receipt:cm28" && receipt.deleted_at_ms == 101));
}
