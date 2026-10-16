//! CP-24 Core transaction gate 的纯内存行为 fixture。
//!
//! 它把 ready projection、WorkPacket、Claim 与直接签发的 Lease 放在同一测试快照中，覆盖成功交接及陈旧
//! snapshot/owner/fence/expiry 拒绝。这里没有调用 QueueStore，所以 Lease 是测试构造值，不证明真正已被原子认领；
//! 本测试也是当前仓库中 `validate_workflow_queue_transaction()` 的直接调用点，不代表生产路由已接线。

use kiana_core::{
    project_workflow_queue_ready, validate_workflow_queue_transaction, WorkflowQueueReadyView,
};
use kiana_domain::{
    BudgetLeaseId, WorkPacket, WorkPacketStatus, WorkflowQueueLease, WorkflowQueueLeaseStatus,
};
use std::collections::BTreeMap;

/// 创建一个带给定 budget lease、单一路径许可并已 Approved 的测试 packet。
fn approved_packet(id: &str, budget: BudgetLeaseId, path: &str) -> WorkPacket {
    let mut packet = WorkPacket::builder_task(id, format!("{id} work")).with_path_allow([path]);
    packet.budget_lease_id = Some(budget);
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("draft packet can be approved");
    packet
}

/// 生成同一 parent/child 图；参数只替换 child 的路径，用于制造 ready snapshot 漂移。
fn packets(child_path: &str) -> BTreeMap<String, WorkPacket> {
    let budget = BudgetLeaseId::new();
    let parent = approved_packet("parent", budget, "src");
    let mut child = approved_packet("child", budget, child_path);
    child.parent_packet_id = Some(parent.id.clone());
    BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)])
}

/// 从投影中取 child 项；父项存在但不被本测试当作交接目标。
fn child_claim(view: &WorkflowQueueReadyView) -> &kiana_domain::WorkflowQueueClaimContract {
    view.claims
        .iter()
        .find(|claim| claim.item_id == "child")
        .expect("child is ready")
}

#[test]
fn cp24_scheduler_transaction_accepts_one_ready_claim_and_current_lease() {
    // 用当前 packet 快照产生 Claim，再直接构造 worker lease，最后让 Core gate核对所有绑定字段。
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
    // 同一个有效快照下逐项改变 owner/fence/time，随后换 packet snapshot；每项都应在交接前拒绝。
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
