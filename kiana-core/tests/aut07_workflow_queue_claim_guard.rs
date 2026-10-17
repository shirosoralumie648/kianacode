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
        "workflow_queue_parent_budget_required",
        "workflow_queue_path_lock_digest_mismatch",
        "workflow_queue_ready_duplicate_item",
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
        "path-lock",
        "source/CI",
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

#[test]
fn aut07_named_stable_codes_are_wired_into_the_claim_lane() {
    // 名称绑定：每个稳定码夹具都必须同时存在于 fixture 文件与该 Step 的手动 workflow 中，
    // 这样 workflow 才能按名复核拒绝路径，而不是只跑整个 target。
    let workflow = include_str!("../../.github/workflows/aut07-workflow-queue-claim.yml");
    let domain = include_str!("../../kiana-domain/tests/aut07_workflow_queue_claim.rs");
    let ready = include_str!("../../kiana-core/tests/aut07_workflow_queue_ready.rs");
    for target in [
        "cargo test -p kiana-domain --test aut07_workflow_queue_claim",
        "cargo test -p kiana-core --test aut07_workflow_queue_ready",
        "cargo test -p kiana-core --test aut07_workflow_queue_claim_guard",
    ] {
        assert!(
            workflow.contains(target),
            "AUT-07 lane target missing: {target}"
        );
    }
    for fixture in [
        "queue_claim_rejects_cycle_duplicate_and_parallel_overflow",
        "ready_claim_cannot_smuggle_owner_or_missing_parent_scope",
        "shared_claim_derives_and_rebinds_the_path_lock_from_work_packet",
        "expired_claim_and_missing_parent_budget_fail_closed",
        "later_child_deadline_is_a_budget_breach_in_the_shared_contract",
    ] {
        assert!(
            domain.contains(fixture),
            "AUT-07 domain fixture missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "AUT-07 domain fixture not wired into the lane: {fixture}"
        );
    }
    for fixture in [
        "ready_view_reuses_packet_readiness_and_emits_one_claim_per_item",
        "dependency_readiness_blocks_child_before_queue_claim",
        "child_scope_or_budget_widening_fails_closed_before_claim",
        "ready_view_rejects_two_claims_for_one_item",
        "ready_view_matches_ready_packets_for_the_same_snapshot",
        "expired_packet_claim_is_reclaimable_but_active_claim_is_never_readied",
        "lease_expired_beyond_the_packet_deadline_cannot_cross_the_gate",
    ] {
        assert!(
            ready.contains(fixture),
            "AUT-07 ready fixture missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "AUT-07 ready fixture not wired into the lane: {fixture}"
        );
    }
    for code in [
        "workflow_queue_path_lock_digest_mismatch",
        "workflow_queue_claim_expired",
        "workflow_queue_parent_budget_required",
        "workflow_queue_budget_subset_invalid",
        "workflow_queue_ready_duplicate_item",
        "workflow_queue_deadline_exceeded",
    ] {
        assert!(
            domain.contains(code) || ready.contains(code),
            "AUT-07 stable code missing: {code}"
        );
    }
}
