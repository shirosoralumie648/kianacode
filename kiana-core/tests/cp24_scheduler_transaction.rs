use kiana_core::{
    project_workflow_queue_ready, validate_workflow_queue_transaction, WorkflowQueueReadyView,
};
use kiana_domain::{
    BudgetLeaseId, WorkPacket, WorkPacketStatus, WorkflowQueueLease, WorkflowQueueLeaseStatus,
};
use std::collections::BTreeMap;

fn approved_packet(id: &str, budget: BudgetLeaseId, path: &str) -> WorkPacket {
    let mut packet = WorkPacket::builder_task(id, format!("{id} work")).with_path_allow([path]);
    packet.budget_lease_id = Some(budget);
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("draft packet can be approved");
    packet
}

fn packets(child_path: &str) -> BTreeMap<String, WorkPacket> {
    let budget = BudgetLeaseId::new();
    let parent = approved_packet("parent", budget, "src");
    let mut child = approved_packet("child", budget, child_path);
    child.parent_packet_id = Some(parent.id.clone());
    BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)])
}

fn child_claim(view: &WorkflowQueueReadyView) -> &kiana_domain::WorkflowQueueClaimContract {
    view.claims
        .iter()
        .find(|claim| claim.item_id == "child")
        .expect("child is ready")
}

#[test]
fn cp24_scheduler_transaction_accepts_one_ready_claim_and_current_lease() {
    let packet_map = packets("src/lib.rs");
    let view = project_workflow_queue_ready(&packet_map, 1_000, 2).expect("ready snapshot");
    let packet = &packet_map["child"];
    let parent = &packet_map["parent"];
    let claim = child_claim(&view);
    let lease =
        WorkflowQueueLease::issue(claim, "scheduler-a", 1, 1, 1_000, 1_100).expect("queue lease");

    validate_workflow_queue_transaction(
        &view,
        claim,
        packet,
        parent,
        &lease,
        "scheduler-a",
        1,
        1,
        1_010,
    )
    .expect("current ready snapshot and lease are dispatchable");
}

#[test]
fn cp24_scheduler_transaction_denies_stale_snapshot_owner_fence_and_expiry() {
    let packet_map = packets("src/lib.rs");
    let view = project_workflow_queue_ready(&packet_map, 1_000, 2).expect("ready snapshot");
    let packet = &packet_map["child"];
    let parent = &packet_map["parent"];
    let claim = child_claim(&view);
    let lease =
        WorkflowQueueLease::issue(claim, "scheduler-a", 1, 1, 1_000, 1_100).expect("queue lease");

    assert_eq!(
        validate_workflow_queue_transaction(
            &view,
            claim,
            packet,
            parent,
            &lease,
            "scheduler-b",
            1,
            1,
            1_010,
        )
        .expect_err("worker owner is part of the fence"),
        "workflow_queue_owner_mismatch"
    );
    assert_eq!(
        validate_workflow_queue_transaction(
            &view,
            claim,
            packet,
            parent,
            &lease,
            "scheduler-a",
            2,
            1,
            1_010,
        )
        .expect_err("stale fence cannot dispatch"),
        "workflow_queue_fence_mismatch"
    );
    assert_eq!(
        validate_workflow_queue_transaction(
            &view,
            claim,
            packet,
            parent,
            &lease,
            "scheduler-a",
            1,
            1,
            1_100,
        )
        .expect_err("expired lease cannot dispatch"),
        "workflow_queue_lease_expired"
    );

    let changed = packets("src/main.rs");
    let changed_view = project_workflow_queue_ready(&changed, 1_000, 2).expect("new snapshot");
    assert_eq!(
        validate_workflow_queue_transaction(
            &changed_view,
            claim,
            packet,
            parent,
            &lease,
            "scheduler-a",
            1,
            1,
            1_010,
        )
        .expect_err("old claim cannot cross a ready snapshot"),
        "workflow_queue_ready_snapshot_mismatch"
    );
    assert_eq!(lease.status, WorkflowQueueLeaseStatus::Active);
}
