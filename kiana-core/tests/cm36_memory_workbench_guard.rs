#[test]
fn memory_workbench_stays_redacted_acl_gated_and_outside_execution_authority() {
    let workbench = include_str!("../../kiana-domain/src/memory_workbench.rs");
    let mutation = include_str!("../../kiana-domain/src/memory_mutation.rs");
    let acl = include_str!("../../kiana-domain/src/memory_acl.rs");

    for marker in [
        "MemoryWorkbenchListItem",
        "MemoryWorkbenchRelations",
        "MemoryAccessPath::ReviewList",
        "MemoryAclDecision::evaluate",
        "memory_workbench_private_preview_leak",
        "memory_workbench_relation_outside_list",
        "MemoryBulkMutationPlan",
        "expected_revisions",
        "MemoryBulkAtomicity::Atomic",
        "MemoryBulkAtomicity::ExplicitSplit",
        "memory_bulk_atomic_partial_result",
        "memory_bulk_split_group_partial_result",
        "MemoryBulkItemStatus::Unknown",
        "MemoryExportManifest",
        "recipient_digest",
        "redaction_profile_digest",
        "MemoryMutationOperation::Approve",
        "MemoryMutationOperation::Publish",
        "MemoryMutationOperation::Expire",
        "MemoryMutationOperation::Delete",
    ] {
        assert!(
            workbench.contains(marker) || mutation.contains(marker) || acl.contains(marker),
            "CM-36 source marker missing: {marker}"
        );
    }

    assert!(workbench.contains("if !decision.allowed"));
    assert!(workbench.contains("visible_ids.contains(record_id)"));
    assert!(workbench.contains("validate_explicit_split_groups"));
    assert!(workbench.contains("target.expected_revision"));
    assert!(!workbench.contains("CapabilityBroker"));
    assert!(!workbench.contains("EventStore"));
    assert!(!workbench.contains("remove_file"));
    assert!(!workbench.contains("std::fs"));
}
