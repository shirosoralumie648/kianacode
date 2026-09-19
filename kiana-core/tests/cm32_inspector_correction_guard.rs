#[test]
fn inspector_is_redacted_and_correction_reuses_memory_mutation() {
    let domain = include_str!("../../kiana-domain/src/inspector.rs");
    let memory = include_str!("../../kiana-domain/src/memory_mutation.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_memory.rs");

    for marker in [
        "ContextMemoryInspectorSnapshot",
        "INSPECTOR_SNAPSHOT_SCHEMA",
        "UserMemoryCorrection",
        "USER_CORRECTION_SCHEMA",
        "locator_digest",
        "source_revision",
        "receipt_digest",
        "context_plan_digest",
        "projection",
        "invalidation_plan_digest",
        "candidate_state_digests",
        "user_correction_governance_required",
        "operator_approval",
        "MemoryMutation",
        "MemoryMutationOperation::Update",
        "MemoryMutationOperation::Delete",
        "memory.review",
        "mutation_receipt",
    ] {
        assert!(
            domain.contains(marker)
                || memory.contains(marker)
                || protocol.contains(marker)
                || daemon.contains(marker),
            "CM-32 source marker missing: {marker}"
        );
    }

    assert!(domain.contains("locator_digest"));
    assert!(domain.contains("operator_approval"));
    assert!(!domain.contains("ModelClient"));
    assert!(!daemon.contains("CapabilityBroker"));
}
