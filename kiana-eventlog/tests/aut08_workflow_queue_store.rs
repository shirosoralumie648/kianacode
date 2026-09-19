use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimRequest, WorkflowQueueClaimStatus,
    WorkflowQueueEffectRequest, WorkflowQueueEffectState, WorkflowQueueHeartbeatRequest,
    WorkflowQueueReclaimRequest,
};
use kiana_eventlog::MemoryWorkflowQueueStore;
use kiana_ports::WorkflowQueueStore;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn named_claim(item_id: &str) -> WorkflowQueueClaimContract {
    WorkflowQueueClaimContract::new(
        item_id,
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
        10_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    )
    .expect("valid claim")
}

fn claim() -> WorkflowQueueClaimContract {
    named_claim("packet-1")
}

fn request(owner_id: &str, fence_token: u64) -> WorkflowQueueClaimRequest {
    request_for("packet-1", owner_id, fence_token)
}

fn request_for(item_id: &str, owner_id: &str, fence_token: u64) -> WorkflowQueueClaimRequest {
    WorkflowQueueClaimRequest {
        claim: named_claim(item_id),
        owner_id: owner_id.to_owned(),
        fence_token,
        authority_epoch: 1,
        observed_at_unix_ms: 1_000,
        lease_ttl_ms: 100,
    }
}

#[tokio::test]
async fn two_workers_have_one_winning_claim() {
    let store = MemoryWorkflowQueueStore::new();
    let (left, right) = tokio::join!(
        store.claim(request("worker-a", 1)),
        store.claim(request("worker-b", 2))
    );
    assert_eq!(left.is_ok() as u8 + right.is_ok() as u8, 1);
}

#[tokio::test]
async fn safe_expiry_reclaims_but_old_worker_cannot_heartbeat() {
    let store = MemoryWorkflowQueueStore::new();
    let first = store
        .claim(request("worker-a", 1))
        .await
        .expect("first claim");
    let next = store
        .reclaim(WorkflowQueueReclaimRequest {
            item_id: first.item_id.clone(),
            new_owner_id: "worker-b".to_owned(),
            new_fence_token: 2,
            authority_epoch: 2,
            observed_at_unix_ms: 1_100,
            lease_ttl_ms: 100,
        })
        .await
        .expect("safe reclaim");
    assert_eq!(next.owner_id, "worker-b");
    let stale = store
        .heartbeat(WorkflowQueueHeartbeatRequest {
            item_id: first.item_id,
            owner_id: "worker-a".to_owned(),
            fence_token: 1,
            authority_epoch: 1,
            observed_at_unix_ms: 1_110,
            lease_ttl_ms: 100,
        })
        .await
        .expect_err("stale worker");
    assert!(
        stale.to_string().contains("owner_mismatch")
            || stale.to_string().contains("fence_mismatch")
    );
}

#[tokio::test]
async fn unknown_effect_enters_recovery_and_cannot_be_reclaimed() {
    let store = MemoryWorkflowQueueStore::new();
    let first = store
        .claim(request("worker-a", 1))
        .await
        .expect("first claim");
    store
        .record_effect(WorkflowQueueEffectRequest {
            item_id: first.item_id.clone(),
            owner_id: "worker-a".to_owned(),
            fence_token: 1,
            authority_epoch: 1,
            observed_at_unix_ms: 1_010,
            effect_state: WorkflowQueueEffectState::ResultUnknown,
        })
        .await
        .expect("unknown effect");
    let blocked = store
        .reclaim(WorkflowQueueReclaimRequest {
            item_id: first.item_id,
            new_owner_id: "worker-b".to_owned(),
            new_fence_token: 2,
            authority_epoch: 2,
            observed_at_unix_ms: 1_100,
            lease_ttl_ms: 100,
        })
        .await
        .expect_err("recovery required");
    assert!(
        blocked.to_string().contains("reclaimable") || blocked.to_string().contains("recovery")
    );
}

#[tokio::test]
async fn ready_view_is_bounded_lexical_and_excludes_claimed_items() {
    let store = MemoryWorkflowQueueStore::new();
    store
        .enqueue(named_claim("packet-b"))
        .await
        .expect("enqueue b");
    store
        .enqueue(named_claim("packet-a"))
        .await
        .expect("enqueue a");
    let ready = store.ready(2).await.expect("ready view");
    assert_eq!(
        ready
            .iter()
            .map(|claim| claim.item_id.as_str())
            .collect::<Vec<_>>(),
        vec!["packet-a", "packet-b"]
    );
    store
        .claim(request_for("packet-a", "worker-a", 1))
        .await
        .expect("claim packet-a");
    let ready = store.ready(2).await.expect("ready view after claim");
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].item_id, "packet-b");
}
