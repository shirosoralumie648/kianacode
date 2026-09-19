use kiana_domain::{WorkflowQueueClaimContract, WorkflowQueueClaimStatus};

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
        contract(WorkflowQueueClaimStatus::Ready, 1, 1, false, false)
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
