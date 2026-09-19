use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimStatus, WorkflowQueueEffectState,
    WorkflowQueueLease, WorkflowQueueLeaseStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn claim() -> WorkflowQueueClaimContract {
    WorkflowQueueClaimContract::new(
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
        2_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    )
    .expect("valid claim")
}

fn lease() -> WorkflowQueueLease {
    WorkflowQueueLease::issue(&claim(), "worker-a", 1, 1, 1_000, 1_100).expect("valid lease")
}

#[test]
fn heartbeat_requires_current_owner_and_fence() {
    let value = lease();
    assert_eq!(
        value
            .renew("worker-b", 1, 1, 1_010, 1_100)
            .expect_err("wrong owner"),
        "workflow_queue_owner_mismatch"
    );
    assert_eq!(
        value
            .renew("worker-a", 2, 1, 1_010, 1_100)
            .expect_err("old fence"),
        "workflow_queue_fence_mismatch"
    );
}

#[test]
fn running_effect_cannot_be_reclaimed_after_expiry() {
    let running = lease()
        .record_effect("worker-a", 1, 1, 1_010, WorkflowQueueEffectState::Running)
        .expect("running effect");
    assert_eq!(
        running
            .reclaim("worker-b", 2, 1, 1_100, 1_200)
            .expect_err("in-flight effect"),
        "workflow_queue_reclaim_effect_in_flight"
    );
}

#[test]
fn unknown_effect_is_fenced_into_recovery() {
    let unknown = lease()
        .record_effect(
            "worker-a",
            1,
            1,
            1_010,
            WorkflowQueueEffectState::ResultUnknown,
        )
        .expect("unknown effect");
    assert_eq!(unknown.status, WorkflowQueueLeaseStatus::ResultUnknown);
    assert_eq!(
        unknown.effect_state,
        WorkflowQueueEffectState::ResultUnknown
    );
    assert!(unknown.validate().is_ok());
}

#[test]
fn safe_expiry_requires_a_new_monotonic_fence() {
    let next = lease()
        .reclaim("worker-b", 2, 2, 1_100, 1_200)
        .expect("safe reclaim");
    assert_eq!(next.owner_id, "worker-b");
    assert_eq!(next.fence_token, 2);
    assert_eq!(
        next.previous_lease_digest.as_deref(),
        Some(lease().lease_digest.as_str())
    );
}
