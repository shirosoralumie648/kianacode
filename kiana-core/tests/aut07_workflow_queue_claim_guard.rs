//! AUT-07 source guard for shared WorkPacket/workflow queue claim boundaries.

#[test]
fn aut07_claim_contract_keeps_scope_budget_path_and_expiry_gates_explicit() {
    let source = include_str!("../../kiana-domain/src/workflow_queue_claim.rs");
    let packet = include_str!("../../kiana-domain/src/work_packets.rs");
    let queue = include_str!("../src/workflow_queue.rs");
    let baseline = include_str!("../../docs/roadmap/aut07-workflow-queue-claim-baseline.md");
    for marker in [
        "WorkflowQueueClaimContract",
        "parent_scope_digest",
        "parent_budget_digest",
        "path_lock_digest",
        "workflow_queue_dependency_cycle",
        "workflow_queue_duplicate_claim",
        "workflow_queue_parallel_limit_exceeded",
        "workflow_queue_claim_expired",
        "workflow_queue_scope_intersection_invalid",
        "workflow_queue_budget_subset_invalid",
        "from_work_packet",
        "workflow_queue_packet_digest",
        "workflow_queue_scope_is_subset_of",
        "workflow_queue_budget_is_subset_of",
    ] {
        assert!(
            source.contains(marker) || packet.contains(marker) || queue.contains(marker),
            "AUT-07 source marker missing: {marker}"
        );
    }
    for marker in [
        "AUT-07",
        "WorkPacket",
        "scope",
        "budget",
        "path lock",
        "partial",
        "AUT-08",
        "durable",
        "scheduler",
    ] {
        assert!(
            baseline.contains(marker),
            "AUT-07 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
