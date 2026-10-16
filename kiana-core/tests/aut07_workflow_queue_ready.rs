//! AUT-07 Core ready-view 的确定性 fixture。
//!
//! 测试用内存 WorkPacket 检查 readiness 复用、依赖阻塞与父子 scope/budget 收窄；不连接 QueueStore，
//! 因此通过只能说明可生成 Claim 视图，不能说明已原子占额、创建 Lease 或启动 scheduler。

use kiana_core::project_workflow_queue_ready;
use kiana_domain::{BudgetLeaseId, WorkPacket, WorkPacketStatus};
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
