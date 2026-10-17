use kiana_domain::{
    BudgetLeaseId, WorkPacket, WorkPacketStatus, WorkflowQueueClaimContract,
    WorkflowQueueClaimStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn contract(
    status: WorkflowQueueClaimStatus,
    active: u32,
    max: u32,
    cycle: bool,
    duplicate: bool,
) -> Result<WorkflowQueueClaimContract, String> {
    WorkflowQueueClaimContract::new(
        "packet-1",
        A,
        Some(B.to_owned()),
        C,
        Some(D.to_owned()),
        E,
        F,
        true,
        cycle,
        true,
        true,
        active,
        max,
        duplicate,
        (status == WorkflowQueueClaimStatus::Claimed).then(|| "worker-1".to_owned()),
        2_000,
        1_000,
        status,
    )
}

#[test]
fn shared_claim_contract_accepts_one_scoped_packet_claim() {
    let value =
        contract(WorkflowQueueClaimStatus::Claimed, 1, 2, false, false).expect("valid claim");
    value.validate().expect("claim validates");
}

#[test]
fn queue_claim_rejects_cycle_duplicate_and_parallel_overflow() {
    assert_eq!(
        contract(WorkflowQueueClaimStatus::Ready, 2, 1, false, false)
            .expect_err("parallel overflow"),
        "workflow_queue_parallel_limit_exceeded"
    );
    assert_eq!(
        contract(WorkflowQueueClaimStatus::Ready, 0, 1, true, false).expect_err("cycle"),
        "workflow_queue_dependency_cycle"
    );
    assert_eq!(
        contract(WorkflowQueueClaimStatus::Ready, 0, 1, false, true).expect_err("duplicate claim"),
        "workflow_queue_duplicate_claim"
    );
}

#[test]
fn ready_claim_cannot_smuggle_owner_or_missing_parent_scope() {
    let ready = contract(WorkflowQueueClaimStatus::Ready, 0, 1, false, false).expect("ready claim");
    ready.validate().expect("ready validates");

    let missing = WorkflowQueueClaimContract::new(
        "packet-1",
        A,
        None,
        C,
        Some(D.to_owned()),
        E,
        F,
        true,
        false,
        true,
        true,
        0,
        1,
        false,
        None,
        2_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    );
    assert_eq!(
        missing.expect_err("parent scope is required"),
        "workflow_queue_parent_scope_required"
    );
}

/// 创建带预算租约、单一路径许可且已 Approved 的 packet，与 CP-24 fixture 同一形状。
fn approved_packet(id: &str, budget: BudgetLeaseId, path: &str) -> WorkPacket {
    let mut packet = WorkPacket::builder_task(id, format!("{id} work")).with_path_allow([path]);
    packet.budget_lease_id = Some(budget);
    packet
        .transition_status(WorkPacketStatus::Approved)
        .expect("draft packet can be approved");
    packet
}

#[test]
fn shared_claim_derives_and_rebinds_the_path_lock_from_work_packet() {
    // Claim 的路径锁摘要必须来自同一个 WorkPacket 合同；伪造摘要不能通过交接前重绑定。
    let budget = BudgetLeaseId::new();
    let parent = approved_packet("parent", budget, "src");
    let mut child = approved_packet("child", budget, "src/lib.rs");
    child.parent_packet_id = Some(parent.id.clone());
    let claim = WorkflowQueueClaimContract::from_work_packet(
        &child,
        &parent,
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
    .expect("server-derived claim is valid");
    assert_eq!(claim.item_id, child.id);
    assert_eq!(
        claim.path_lock_digest,
        child.workflow_queue_path_lock_digest()
    );
    claim
        .validate_packet_binding(&child, &parent)
        .expect("claim re-derives the same path-lock binding");

    let mut forged = claim;
    forged.path_lock_digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        forged
            .validate_packet_binding(&child, &parent)
            .expect_err("forged path-lock digest cannot pass the shared binding"),
        "workflow_queue_path_lock_digest_mismatch"
    );
}

#[test]
fn expired_claim_and_missing_parent_budget_fail_closed() {
    let expired = WorkflowQueueClaimContract::new(
        "packet-1",
        A,
        Some(B.to_owned()),
        C,
        Some(D.to_owned()),
        E,
        F,
        true,
        false,
        true,
        true,
        0,
        1,
        false,
        None,
        1_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    );
    assert_eq!(
        expired.expect_err("claim expiry must be after the observation instant"),
        "workflow_queue_claim_expired"
    );

    let missing_parent_budget = WorkflowQueueClaimContract::new(
        "packet-1",
        A,
        Some(B.to_owned()),
        C,
        None,
        E,
        F,
        true,
        false,
        true,
        true,
        0,
        1,
        false,
        None,
        2_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    );
    assert_eq!(
        missing_parent_budget.expect_err("parent budget digest is required"),
        "workflow_queue_parent_budget_required"
    );
}

#[test]
fn later_child_deadline_is_a_budget_breach_in_the_shared_contract() {
    // 子 packet 与父级共用 lease 但 deadline 更晚时，共享预算合同在产出 Claim 前拒绝。
    let budget = BudgetLeaseId::new();
    let mut parent = approved_packet("parent", budget, "src");
    parent.deadline_unix_ms = Some(5_000);
    let mut child = approved_packet("child", budget, "src/lib.rs");
    child.parent_packet_id = Some(parent.id.clone());
    child.deadline_unix_ms = Some(9_000);
    assert_eq!(
        WorkflowQueueClaimContract::from_work_packet(
            &child,
            &parent,
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
        .expect_err("child deadline cannot exceed the parent budget window"),
        "workflow_queue_budget_subset_invalid"
    );
}
