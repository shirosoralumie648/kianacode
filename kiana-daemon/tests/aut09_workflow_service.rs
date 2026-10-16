//! AUT-09 Daemon queue service 的异步合成集成 fixture。
//!
//! 测试把 service 接到 `MemoryWorkflowQueueStore`，用显式 tick/时间驱动操作；它覆盖 worker/channel 与 Store
//! 的协调，但没有定时器、真实进程重启或 durable EventLog queue history。

use kiana_daemon::WorkflowQueueService;
use kiana_domain::{
    WorkflowQueueClaimContract, WorkflowQueueClaimRequest, WorkflowQueueClaimStatus,
    WorkflowQueueEffectRequest, WorkflowQueueEffectState, WorkflowQueueHeartbeatRequest,
};
use kiana_eventlog::MemoryWorkflowQueueStore;
use kiana_ports::WorkflowQueueStore;
use std::sync::Arc;

// A-F 是只满足摘要外形的测试值，并不验证 claim 是否来自真实 WorkPacket。
const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const E: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const F: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

/// 为 worker 流程提供一条 Ready 状态的内存 claim。
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
    // service 将 claim 与 heartbeat 顺序送给同一 MemoryStore；关闭时 NotStarted lease 可安全 fence。
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
    // 尚未启动时 tick 返回错误；启动后重复 start 被拒绝，证明单个 handle 不接受第二个已登记 worker。
    let service = WorkflowQueueService::new();
    assert!(service.tick(1).await.is_err());
    let store: Arc<dyn WorkflowQueueStore> = Arc::new(MemoryWorkflowQueueStore::new());
    service.start(store.clone()).await.expect("start service");
    let duplicate = service.start(store).await.expect_err("duplicate start");
    assert!(duplicate.to_string().contains("already_started"));
    service.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn explicit_tick_claim_effect_and_restart_do_not_repeat_unknown_work() {
    // tick 是测试明确调用的，不是 service 内的 timer；Running effect 在 shutdown 时进入恢复计数。
    let service = WorkflowQueueService::new();
    let store = Arc::new(MemoryWorkflowQueueStore::new());
    store.enqueue(claim()).await.expect("enqueue ready item");
    let store_port: Arc<dyn WorkflowQueueStore> = store.clone();
    service.start(store_port).await.expect("start service");

    let ready = service.tick(1).await.expect("tick ready view");
    assert_eq!(ready.len(), 1);
    let lease = service
        .claim(WorkflowQueueClaimRequest {
            claim: ready[0].clone(),
            owner_id: "worker-a".to_owned(),
            fence_token: 1,
            authority_epoch: 1,
            observed_at_unix_ms: 1_000,
            lease_ttl_ms: 100,
        })
        .await
        .expect("claim from tick");
    service
        .record_effect(WorkflowQueueEffectRequest {
            item_id: lease.item_id.clone(),
            owner_id: "worker-a".to_owned(),
            fence_token: 1,
            authority_epoch: 1,
            observed_at_unix_ms: 1_010,
            effect_state: WorkflowQueueEffectState::Running,
        })
        .await
        .expect("record running command");

    let report = service.shutdown_at(Some(1_020)).await.expect("shutdown");
    assert_eq!(report.fenced_count, 0);
    assert_eq!(report.recovery_required_count, 1);

    service
        .start(store_port_from(store.clone()))
        .await
        .expect("restart service");
    // 这里只重启 service worker，继续使用同一个内存 Store；证明未知项不会再次 ready，不证明操作系统进程重启后的持久恢复。
    assert!(service.tick(1).await.expect("restart tick").is_empty());
    service.shutdown().await.expect("final shutdown");
}

/// 仅把同一个测试内存 Store 转成 Port trait object，以便 service restart 仍访问同一状态。
fn store_port_from(store: Arc<MemoryWorkflowQueueStore>) -> Arc<dyn WorkflowQueueStore> {
    store
}
