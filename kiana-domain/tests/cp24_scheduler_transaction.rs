use kiana_domain::{
    BudgetLeaseId, WorkPacket, WorkPacketStatus, WorkflowQueueClaimContract,
    WorkflowQueueClaimStatus,
};

fn approved_packet(id: &str, budget: BudgetLeaseId, path: &str) -> WorkPacket {
    let mut packet = WorkPacket::builder_task(id, format!("{id} work")).with_path_allow([path]);
    packet.budget_lease_id = Some(budget);
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("draft packet can be approved");
    packet
}

fn child_claim(child: &WorkPacket, parent: &WorkPacket) -> WorkflowQueueClaimContract {
    WorkflowQueueClaimContract::from_work_packet(
        child,
        parent,
        true,
        false,
        0,
        2,
        false,
        None,
        2_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    )
    .expect("server-derived claim is valid")
}

#[test]
fn cp24_claim_rebinds_to_packet_parent_scope_budget_and_path_lock() {
    let budget = BudgetLeaseId::new();
    let parent = approved_packet("parent", budget, "src");
    let mut child = approved_packet("child", budget, "src/lib.rs");
    child.parent_packet_id = Some(parent.id.clone());
    let claim = child_claim(&child, &parent);

    claim
        .validate_packet_binding(&child, &parent)
        .expect("claim remains bound to the immutable packet snapshot");

    let mut widened_scope = child.clone();
    widened_scope.path_allow = vec!["outside".to_owned()];
    assert_eq!(
        claim
            .validate_packet_binding(&widened_scope, &parent)
            .expect_err("child path cannot widen beyond parent"),
        "workflow_queue_scope_intersection_invalid"
    );

    let mut widened_budget = child;
    widened_budget.budget_lease_id = Some(BudgetLeaseId::new());
    assert_eq!(
        claim
            .validate_packet_binding(&widened_budget, &parent)
            .expect_err("child budget cannot escape the parent lease"),
        "workflow_queue_budget_subset_invalid"
    );
}

#[test]
fn cp24_claim_rejects_parent_mismatch_and_forged_immutable_digests() {
    let budget = BudgetLeaseId::new();
    let parent = approved_packet("parent", budget, "src");
    let other_parent = approved_packet("other-parent", budget, "src");
    let mut child = approved_packet("child", budget, "src/lib.rs");
    child.parent_packet_id = Some(parent.id.clone());
    let claim = child_claim(&child, &parent);

    assert_eq!(
        claim
            .validate_packet_binding(&child, &other_parent)
            .expect_err("claim cannot change parent snapshot"),
        "workflow_queue_parent_packet_mismatch"
    );

    let mut forged = claim.clone();
    forged.work_packet_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        forged
            .validate_packet_binding(&child, &parent)
            .expect_err("caller cannot replace the packet digest"),
        "workflow_queue_packet_digest_mismatch"
    );
}
