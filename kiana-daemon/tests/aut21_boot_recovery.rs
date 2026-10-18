//! AUT-21 daemon 夹具：重启后默认 fenced、显式 operator resume 才解除 dispatch。
//!
//! 夹具驱动真实的 `DaemonHost` + `MemoryEventLog`；runner 是零调用 runner（本切片不执行模型
//! 或 provider）。所有断言都先证明「未 resume 时不能 dispatch / 不启动服务」，最后才证明干净
//! 重启可以 resume。

use async_trait::async_trait;
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    AutomationBootRecoveryState, ExecutionStatus, RequestContext, RequestId, RuntimeEvent,
};
use kiana_eventlog::{MemoryAutomationDispatchStore, MemoryEventLog, MemoryWorkflowQueueStore};
use kiana_ports::{AutomationWorkerEffectRunner, EventStorePort, PortError, RunnerPort};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

/// 恢复切片不允许调用模型；任何 send 都计数并返回错误。
struct ZeroCallRunner {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl RunnerPort for ZeroCallRunner {
    async fn send(
        &self,
        _command: kiana_runner_protocol::RunnerCommand,
    ) -> Result<Vec<kiana_runner_protocol::RunnerEvent>, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Failed("runner_must_not_be_called".to_owned()))
    }

    fn install_model_budget(
        &self,
        _budget: std::sync::Arc<dyn kiana_ports::ModelBudgetPort>,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

fn host(store: Arc<MemoryEventLog>, calls: Arc<AtomicUsize>) -> DaemonHost {
    DaemonHost::with_runner_and_events(Arc::new(ZeroCallRunner { calls }), store)
        .expect("daemon host")
}

fn operator(project_root: &str) -> RequestContext {
    let mut context = RequestContext::local("aut21-session", project_root);
    context.project_trusted = true;
    context.permission_profile = kiana_domain::PermissionProfile::Autonomous;
    context
}

async fn seed_clean(store: &Arc<MemoryEventLog>) {
    // 一条普通已提交事实：让 source cursor 非零，且不属于 required 前缀外的未知族。
    let event =
        RuntimeEvent::new(RequestId::new(), 1, "boot.probe", json!({"probe": "clean"})).unwrap();
    let events: Arc<dyn EventStorePort> = store.clone() as Arc<dyn EventStorePort>;
    events.append(event).await.expect("seed clean fact");
}

async fn seed_unknown(store: &Arc<MemoryEventLog>) {
    let run_id = kiana_domain::RunId::new();
    let events: Arc<dyn EventStorePort> = store.clone() as Arc<dyn EventStorePort>;
    for (sequence, kind) in [(1_u64, "run.authorized"), (2, "run.result_unknown")] {
        events
            .append(
                RuntimeEvent::new(RequestId::new(), sequence, kind, json!({"run_id": run_id}))
                    .unwrap()
                    .with_stream_metadata("run", run_id.to_string(), sequence),
            )
            .await
            .expect("seed run fact");
    }
}

#[tokio::test]
async fn restart_defaults_to_fenced_and_requires_explicit_operator_resume() {
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(MemoryEventLog::new());
    seed_clean(&store).await;
    let host = host(store, calls.clone());
    let root = "/repo/aut21-clean";

    // 观测前：兼容路径（未观测）不 fence，但也还没有恢复事实。
    assert!(!host.automation_dispatch_fenced());
    assert!(host.automation_boot_recovery_gate().fact.is_none());

    // 观测后：干净重启默认 Rebuilding，dispatch 被 fence；投影已经通过 rebuild 追平。
    let fact = host
        .observe_automation_boot_recovery(root)
        .await
        .expect("observe boot recovery");
    assert_eq!(fact.state, AutomationBootRecoveryState::Rebuilding);
    assert_eq!(fact.projection_cursor, fact.source_cursor);
    assert!(fact.projection_generation > 0);
    assert!(host.automation_dispatch_fenced());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 非 operator（带 cell）resume 被 Core 拒绝，fence 保持。
    let mut cell_context = operator(root);
    cell_context.cell_id = Some(kiana_domain::CellId::new());
    let denied = host
        .resume_automation_after_recovery(cell_context, root)
        .await
        .expect("blocked response is not a transport error");
    assert_eq!(denied.status, ExecutionStatus::Blocked);
    assert_eq!(
        denied.error.as_deref(),
        Some("automation_boot_recovery_operator_required")
    );
    assert!(host.automation_dispatch_fenced());

    // 匿名的 actor 同样拒绝。
    let mut anonymous = operator(root);
    anonymous.actor_id = None;
    let denied = host
        .resume_automation_after_recovery(anonymous, root)
        .await
        .expect("blocked response is not a transport error");
    assert_eq!(
        denied.error.as_deref(),
        Some("automation_boot_recovery_operator_required")
    );

    // 项目不匹配也拒绝（不能让另一个项目的观测解除本项目的 fence）。
    let denied = host
        .resume_automation_after_recovery(operator("/repo/other"), root)
        .await
        .expect("blocked response is not a transport error");
    assert_eq!(
        denied.error.as_deref(),
        Some("automation_boot_recovery_project_mismatch")
    );
    assert!(host.automation_dispatch_fenced());

    // 显式 operator resume：成功且只解除 fence，不启动任何 effect。
    let response = host
        .resume_automation_after_recovery(operator(root), root)
        .await
        .expect("resume");
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(
        response.output["fact"]["state"],
        json!(AutomationBootRecoveryState::Ready)
    );
    assert!(!host.automation_dispatch_fenced());
    assert_eq!(
        host.automation_boot_recovery_gate().fact.unwrap().state,
        AutomationBootRecoveryState::Ready
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 再次 resume：Core 明确拒绝「已经 Ready」的自证，fence 状态不变（仍为已解除）。
    let again = host
        .resume_automation_after_recovery(operator(root), root)
        .await
        .expect("blocked response is not a transport error");
    assert_eq!(
        again.error.as_deref(),
        Some("automation_boot_recovery_resume_already_ready")
    );
    assert!(!host.automation_dispatch_fenced());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pending_unknown_and_stale_increments_force_reconcile_required() {
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(MemoryEventLog::new());
    seed_unknown(&store).await;
    let host = host(store, calls.clone());
    let root = "/repo/aut21-unknown";

    let fact = host
        .observe_automation_boot_recovery(root)
        .await
        .expect("observe boot recovery");
    assert_eq!(fact.state, AutomationBootRecoveryState::ReconcileRequired);
    assert_eq!(fact.pending_unknown_count, 1);
    assert_eq!(fact.reconcile_cases.len(), 1);
    assert!(fact.reconcile_cases[0].pending_unknown_count >= 1);
    assert!(host.automation_dispatch_fenced());

    // resume 不是 reconcile：未结 Unknown 时显式 resume 被拒绝，fence 保持。
    let denied = host
        .resume_automation_after_recovery(operator(root), root)
        .await
        .expect("blocked response is not a transport error");
    assert_eq!(denied.status, ExecutionStatus::Blocked);
    assert_eq!(
        denied.error.as_deref(),
        Some("automation_boot_recovery_reconcile_required")
    );
    assert!(host.automation_dispatch_fenced());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 释放未结 work（重新观测前清掉 Unknown）后再 resume 才可能成功。
    let clean = Arc::new(MemoryEventLog::new());
    seed_clean(&clean).await;
    let recovered = host_for(&clean, calls.clone());
    let fact = recovered
        .observe_automation_boot_recovery(root)
        .await
        .expect("observe clean");
    assert_eq!(fact.state, AutomationBootRecoveryState::Rebuilding);
    recovered
        .resume_automation_after_recovery(operator(root), root)
        .await
        .expect("resume after clean re-observation");
    assert!(!recovered.automation_dispatch_fenced());
}

fn host_for(store: &Arc<MemoryEventLog>, calls: Arc<AtomicUsize>) -> DaemonHost {
    host(store.clone(), calls)
}

#[tokio::test]
async fn resume_and_observation_never_start_the_dispatch_or_workflow_services() {
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(MemoryEventLog::new());
    seed_clean(&store).await;
    let host = host(store, calls.clone());
    let root = "/repo/aut21-services";

    host.observe_automation_boot_recovery(root)
        .await
        .expect("observe");
    host.resume_automation_after_recovery(operator(root), root)
        .await
        .expect("resume");

    // 两个服务仍然「未启动」：恢复路径没有替调用者启动 effect worker。
    assert_eq!(
        host.automation_dispatch_service()
            .fetch_command("command-1")
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:automation_dispatch_service_not_started"
    );
    assert_eq!(
        host.workflow_queue_service()
            .tick(1)
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:workflow_service_not_started"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct NoCallEffectRunner;

#[async_trait]
impl AutomationWorkerEffectRunner for NoCallEffectRunner {
    async fn run_dispatch_attempt(
        &self,
        _command: &kiana_domain::AutomationDispatchCommand,
    ) -> kiana_domain::AutomationWorkerEffectOutcome {
        panic!("boot recovery must not dispatch an effect");
    }
}

#[tokio::test]
async fn observed_but_unresumed_host_cannot_start_dispatch_or_workflow_services() {
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(MemoryEventLog::new());
    seed_clean(&store).await;
    let host = host_for(&store, calls.clone());
    let root = "/repo/aut21-fenced-services";

    host.observe_automation_boot_recovery(root)
        .await
        .expect("observe");
    assert!(host.automation_dispatch_fenced());

    // 未显式 resume：两个服务都不允许启动（唯一的 dispatch 入口被 fence 挡住）。
    assert_eq!(
        host.start_automation_dispatch_service(
            Arc::new(MemoryAutomationDispatchStore::new()),
            Arc::new(NoCallEffectRunner),
        )
        .await
        .unwrap_err()
        .to_string(),
        "port_conflict:automation_boot_recovery_resume_required:automation_dispatch_service"
    );
    assert_eq!(
        host.start_workflow_queue_service(Arc::new(MemoryWorkflowQueueStore::new()))
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:automation_boot_recovery_resume_required:workflow_service"
    );

    // 显式 operator resume 之后同一个入口才可用。
    host.resume_automation_after_recovery(operator(root), root)
        .await
        .expect("resume");
    host.start_automation_dispatch_service(
        Arc::new(MemoryAutomationDispatchStore::new()),
        Arc::new(NoCallEffectRunner),
    )
    .await
    .expect("dispatch service starts after explicit resume");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resume_without_observation_is_refused() {
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(MemoryEventLog::new());
    let host = host(store, calls);
    assert_eq!(
        host.resume_automation_after_recovery(
            operator("/repo/aut21-unobserved"),
            "/repo/aut21-unobserved"
        )
        .await
        .unwrap_err()
        .to_string(),
        "port_conflict:automation_boot_recovery_observation_required"
    );
}
