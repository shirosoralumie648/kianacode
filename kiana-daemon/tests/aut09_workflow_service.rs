use kiana_daemon::WorkflowQueueService;
use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimRequest, WorkflowQueueClaimStatus,
    WorkflowQueueHeartbeatRequest,
};
use kiana_eventlog::MemoryWorkflowQueueStore;
use kiana_ports::WorkflowQueueStore;
use std::sync::Arc;

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
        10_000,
        1_000,
        WorkflowQueueClaimStatus::Ready,
    )
    .expect("valid claim")
}

#[tokio::test]
async fn bounded_service_routes_queue_ops_and_fences_on_shutdown() {
    let service = WorkflowQueueService::new();
    let store: Arc<dyn WorkflowQueueStore> = Arc::new(MemoryWorkflowQueueStore::new());
    service.start(store).await.expect("start service");
    let lease = service
        .claim(WorkflowQueueClaimRequest {
            claim: claim(),
            owner_id: "worker-a".to_owned(),
            fence_token: 1,
            authority_epoch: 1,
            observed_at_unix_ms: 1_000,
            lease_ttl_ms: 100,
        })
        .await
        .expect("claim through service");
    let renewed = service
        .heartbeat(WorkflowQueueHeartbeatRequest {
            item_id: lease.item_id.clone(),
            owner_id: "worker-a".to_owned(),
            fence_token: 1,
            authority_epoch: 1,
            observed_at_unix_ms: 1_010,
            lease_ttl_ms: 100,
        })
        .await
        .expect("heartbeat through service");
    assert!(renewed.sequence > lease.sequence);
    let report = service.shutdown_at(Some(1_020)).await.expect("shutdown");
    assert_eq!(report.fenced_count, 1);
    assert_eq!(report.recovery_required_count, 0);
}

#[tokio::test]
async fn service_start_is_singleton_and_unstarted_calls_fail_closed() {
    let service = WorkflowQueueService::new();
    assert!(service.tick(1).await.is_err());
    let store: Arc<dyn WorkflowQueueStore> = Arc::new(MemoryWorkflowQueueStore::new());
    service.start(store.clone()).await.expect("start service");
    let duplicate = service.start(store).await.expect_err("duplicate start");
    assert!(duplicate.to_string().contains("already_started"));
    service.shutdown().await.expect("shutdown");
}
