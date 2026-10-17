//! AUT-07 Core ready-view 的确定性 fixture。
//!
//! 测试用内存 WorkPacket 检查 readiness 复用、依赖阻塞与父子 scope/budget 收窄；不连接 QueueStore，
//! 因此通过只能说明可生成 Claim 视图，不能说明已原子占额、创建 Lease 或启动 scheduler。
//! 交接前 gate 只验证同一内存快照上的 lease 绑定（owner/fence/epoch/deadline），不证明任何持久队列状态。

use kiana_core::{project_workflow_queue_ready, validate_workflow_queue_transaction};
use kiana_domain::{
    ready_packets, BudgetLeaseId, CellId, PacketClaim, SessionId, WorkPacket, WorkPacketStatus,
    WorkflowQueueLease,
};
use std::collections::BTreeMap;

/// 给 fixture packet 安装预算租约 ID，并按 WorkPacket 状态机从 Draft 推进为 Approved。
fn approved(mut packet: WorkPacket, budget: BudgetLeaseId) -> WorkPacket {
    packet.budget_lease_id = Some(budget);
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("draft -> approved");
    packet
}

#[test]
fn ready_view_reuses_packet_readiness_and_emits_one_claim_per_item() {
    // 父/子 packet 都 approved、路径范围相互包含；ready view 按 item ID 稳定排序并各产生一条 Claim。
    let budget = BudgetLeaseId::new();
    let parent = approved(
        WorkPacket::builder_task("parent", "parent work").with_path_allow(["src"]),
        budget,
    );
    let mut child = approved(
        WorkPacket::builder_task("child", "child work").with_path_allow(["src/lib.rs"]),
        budget,
    );
    child.parent_packet_id = Some(parent.id.clone());
    let packets = BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)]);

    let view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready view");
    view.validate().expect("view validates");
    assert_eq!(view.claims.len(), 2);
    assert_eq!(
        view.claims
            .iter()
            .map(|claim| claim.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["child", "parent"]
    );
}

#[test]
fn dependency_readiness_blocks_child_before_queue_claim() {
    // child 声明 parent 为依赖，但 parent 尚未结束，所以只让 parent 出现在 ready claims 中。
    let budget = BudgetLeaseId::new();
    let parent = approved(
        WorkPacket::builder_task("parent", "parent work").with_path_allow(["src"]),
        budget,
    );
    let mut child = approved(
        WorkPacket::builder_task("child", "child work")
            .with_path_allow(["src/lib.rs"])
            .with_dependencies(["parent"]),
        budget,
    );
    child.parent_packet_id = Some(parent.id.clone());
    let packets = BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)]);

    let view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready view");
    assert_eq!(view.claims.len(), 1);
    assert_eq!(view.claims[0].item_id, "parent");
    assert_eq!(
        view.blocked.get("child").map(String::as_str),
        Some("dependency_incomplete:parent")
    );
}

#[test]
fn child_scope_or_budget_widening_fails_closed_before_claim() {
    // child 路径越出 parent，且预算 lease 也不同；readiness projection 在产出 Claim 前拒绝扩权。
    let parent_budget = BudgetLeaseId::new();
    let child_budget = BudgetLeaseId::new();
    let parent = approved(
        WorkPacket::builder_task("parent", "parent work").with_path_allow(["src"]),
        parent_budget,
    );
    let mut child = approved(
        WorkPacket::builder_task("child", "child work").with_path_allow(["outside"]),
        child_budget,
    );
    child.parent_packet_id = Some(parent.id.clone());
    let packets = BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)]);

    let error = project_workflow_queue_ready(&packets, 1_000, 2).unwrap_err();
    assert!(
        error == "workflow_queue_scope_intersection_invalid"
            || error == "workflow_queue_budget_subset_invalid"
    );
}

#[test]
fn ready_view_rejects_two_claims_for_one_item() {
    // 同一 item 出现两条 Ready claim 时，交接前必须失败关闭：一个 item 只有一个有效 claim。
    let budget = BudgetLeaseId::new();
    let parent = approved(
        WorkPacket::builder_task("parent", "parent work").with_path_allow(["src"]),
        budget,
    );
    let mut child = approved(
        WorkPacket::builder_task("child", "child work").with_path_allow(["src/lib.rs"]),
        budget,
    );
    child.parent_packet_id = Some(parent.id.clone());
    let packets = BTreeMap::from([(parent.id.clone(), parent), (child.id.clone(), child)]);

    let mut view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready view");
    let claim = view.claims[0].clone();
    view.claims = vec![claim.clone(), claim];
    view.ready_digest = view.digest();
    assert_eq!(
        view.validate().unwrap_err(),
        "workflow_queue_ready_duplicate_item"
    );
}

#[test]
fn ready_view_matches_ready_packets_for_the_same_snapshot() {
    // 队列 ready view 与 packet 看板必须给出同一 readiness 投影：同一快照下 ready/blocked/expired
    // 三个集合逐项相等，且每条 ready Claim 都能从 packet/parent 重算出相同绑定。
    let budget = BudgetLeaseId::new();
    let parent = approved(
        WorkPacket::builder_task("parent", "parent work").with_path_allow(["src"]),
        budget,
    );
    let mut child = approved(
        WorkPacket::builder_task("child", "child work").with_path_allow(["src/lib.rs"]),
        budget,
    );
    child.parent_packet_id = Some(parent.id.clone());
    let mut blocked_child = approved(
        WorkPacket::builder_task("blocked-child", "blocked child work")
            .with_path_allow(["src/lib.rs"])
            .with_dependencies(["child"]),
        budget,
    );
    blocked_child.parent_packet_id = Some(parent.id.clone());
    let packets = BTreeMap::from([
        (parent.id.clone(), parent),
        (child.id.clone(), child),
        (blocked_child.id.clone(), blocked_child),
    ]);

    let readiness = ready_packets(&packets, 1_000).expect("packet readiness");
    let view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready view");

    let claim_items = view
        .claims
        .iter()
        .map(|claim| claim.item_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(claim_items, readiness.ready);
    assert_eq!(view.blocked, readiness.blocked);
    assert_eq!(view.expired_claims, readiness.expired_claims);
    assert_eq!(
        view.blocked.get("blocked-child").map(String::as_str),
        Some("dependency_incomplete:child")
    );

    // parent-child intersection：child 的 scope/budget/path-lock 摘要等于同一快照的重算值。
    let child_claim = view
        .claims
        .iter()
        .find(|claim| claim.item_id == "child")
        .expect("child is ready");
    let child_packet = &packets["child"];
    let parent_packet = &packets["parent"];
    assert_eq!(
        child_claim.parent_scope_digest.as_deref(),
        Some(parent_packet.workflow_queue_scope_digest().as_str())
    );
    assert_eq!(
        child_claim.item_scope_digest,
        child_packet.workflow_queue_scope_digest()
    );
    assert_eq!(
        child_claim.parent_budget_digest.as_deref(),
        Some(parent_packet.workflow_queue_budget_digest().as_str())
    );
    assert_eq!(
        child_claim.item_budget_digest,
        child_packet.workflow_queue_budget_digest()
    );
    assert_eq!(
        child_claim.path_lock_digest,
        child_packet.workflow_queue_path_lock_digest()
    );
    for claim in &view.claims {
        let packet = &packets[&claim.item_id];
        let parent = packet
            .parent_packet_id
            .as_ref()
            .and_then(|parent_id| packets.get(parent_id))
            .unwrap_or(packet);
        claim
            .validate_packet_binding(packet, parent)
            .expect("claim re-derives the shared snapshot binding");
    }
}

#[test]
fn expired_packet_claim_is_reclaimable_but_active_claim_is_never_readied() {
    // 共享合同下「一个 item 只有一个有效 claim」：active claim 的 packet 只以 packet_claimed 阻塞，
    // 不得出现在 ready/expired；过期 claim 只进 expired 投影并重新可认领。
    let budget = BudgetLeaseId::new();
    let mut expired = approved(
        WorkPacket::builder_task("expired-claim", "expired claim work")
            .with_path_allow(["src/expired"]),
        budget,
    );
    expired.claim = Some(PacketClaim {
        owner: CellId::new(),
        owner_session_id: SessionId::new("stale-worker"),
        lease_expires_at: 900,
        heartbeat_at: 800,
    });
    let mut active = approved(
        WorkPacket::builder_task("active-claim", "active claim work")
            .with_path_allow(["src/active"]),
        budget,
    );
    active.claim = Some(PacketClaim {
        owner: CellId::new(),
        owner_session_id: SessionId::new("worker"),
        lease_expires_at: 2_000,
        heartbeat_at: 1_500,
    });
    let packets = BTreeMap::from([(expired.id.clone(), expired), (active.id.clone(), active)]);

    let view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready view");
    assert_eq!(view.expired_claims, vec!["expired-claim".to_owned()]);
    assert_eq!(
        view.claims
            .iter()
            .map(|claim| claim.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["expired-claim"]
    );
    assert_eq!(
        view.blocked.get("active-claim").map(String::as_str),
        Some("packet_claimed")
    );
    assert!(!view.blocked.contains_key("expired-claim"));
}

#[test]
fn lease_expired_beyond_the_packet_deadline_cannot_cross_the_gate() {
    // 更晚 deadline 越界：owner/fence/epoch 都正确时，lease 仍不得把有效期延长到 packet
    // deadline 之后；deadline 内的 lease 同一时刻仍可交接，证明拒绝来自 deadline 而不是其它绑定。
    let budget = BudgetLeaseId::new();
    let mut packet = approved(
        WorkPacket::builder_task("deadline", "deadline work").with_path_allow(["src"]),
        budget,
    );
    packet.deadline_unix_ms = Some(1_050);
    let packets = BTreeMap::from([(packet.id.clone(), packet.clone())]);
    let view = project_workflow_queue_ready(&packets, 1_000, 2).expect("ready view");
    let claim = view
        .claims
        .iter()
        .find(|claim| claim.item_id == "deadline")
        .expect("deadline packet is ready")
        .clone();

    let too_late =
        WorkflowQueueLease::issue(&claim, "scheduler-a", 1, 1, 1_000, 1_100).expect("lease");
    assert_eq!(
        validate_workflow_queue_transaction(
            &view,
            &claim,
            &packet,
            &packet,
            &too_late,
            "scheduler-a",
            1,
            1,
            1_010,
        )
        .expect_err("lease cannot extend beyond the packet deadline"),
        "workflow_queue_deadline_exceeded"
    );

    let bounded =
        WorkflowQueueLease::issue(&claim, "scheduler-a", 1, 1, 1_000, 1_050).expect("lease");
    validate_workflow_queue_transaction(
        &view,
        &claim,
        &packet,
        &packet,
        &bounded,
        "scheduler-a",
        1,
        1,
        1_010,
    )
    .expect("lease inside the packet deadline is dispatchable");
}
