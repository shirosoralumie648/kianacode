#[test]
fn deletion_propagation_is_epoch_fenced_and_history_is_not_reinjected() {
    let domain = include_str!("../../kiana-domain/src/memory_invalidation.rs");
    let governance = include_str!("../../kiana-domain/src/governance.rs");
    let core_governance = include_str!("../src/data_governance.rs");
    let core_receipts = include_str!("../src/receipts.rs");
    let daemon = include_str!("../../kiana-daemon/src/data_governance.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let context = include_str!("../../kiana-daemon/src/context_query.rs");
    let checkpoint = include_str!("../src/workspace_checkpoints.rs");

    for marker in [
        "MemoryInvalidationPlan",
        "MemoryPropagationTarget",
        "MemoryJsonl",
        "MemoryBody",
        "Bm25Index",
        "DenseIndex",
        "RepoIndex",
        "ContextPlan",
        "Summary",
        "Checkpoint",
        "PromptCache",
        "HistoricalReceipt",
        "PreservedInvalid",
        "reinjection_allowed",
        "data_epoch",
        "tombstone_digest",
        "historical_receipts",
        "memory-jsonl",
        "bm25-index",
        "dense-index",
        "repo-index",
        "context-plan",
        "prompt-cache",
        "preserved_invalid",
        "data_revoked",
        "checkpoint_data_epoch_changed",
        "revoked_sources",
        "Derived caches are disposable",
    ] {
        assert!(
            domain.contains(marker)
                || governance.contains(marker)
                || core_governance.contains(marker)
                || core_receipts.contains(marker)
                || daemon.contains(marker)
                || memory.contains(marker)
                || context.contains(marker)
                || checkpoint.contains(marker),
            "CM-28 source marker missing: {marker}"
        );
    }

    assert!(domain.contains("reinjection_allowed"));
    assert!(core_receipts.contains("historical_receipt"));
    assert!(daemon.contains("historical_receipts"));
    assert!(!daemon.contains("ModelClient"));
    assert!(!core_governance.contains("CapabilityBroker"));
}
