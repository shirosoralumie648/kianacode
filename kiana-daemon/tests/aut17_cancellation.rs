//! AUT-17 daemon 夹具：DaemonHost 持有的取消服务把 queued 取消判为 not_executed、把已 claim 的
//! attempt 交给注入的 stop 控制器，并且只有确认停止才记成 stopped；迟到结果只能 fence。
//!
//! 夹具驱动真实的 `DaemonHost` + `MemoryAutomationDispatchStore` + `MemoryAutomationCancellationStore`
//! + 计数 stop 控制器；stop 控制器代表“调用方真正触达停止边界的请求”。本文件不连接模型、网络或
//! 真实 provider。

use async_trait::async_trait;
use kiana_daemon::{
    AutomationCancellationService, AutomationCancellationShutdownReport, DaemonHost,
    ProjectTrustAuthority,
};
use kiana_domain::{
    automation_dispatch_result, claim_automation_dispatch_command,
    observe_automation_dispatch_command, prepare_automation_dispatch_command,
    AutomationCancelRequest, AutomationCancelState, AutomationDispatchClaim,
    AutomationDispatchClaimRequest, AutomationDispatchCommand, AutomationDispatchIntent,
    AutomationDispatchObservationRequest, AutomationDispatchState, AutomationLateResult,
    AutomationStopReport, AutomationStopState, AutomationWorkerEffectOutcome,
    AUTOMATION_DISPATCH_SCHEMA,
};
use kiana_eventlog::{MemoryAutomationCancellationStore, MemoryAutomationDispatchStore};
use kiana_ports::{AutomationWorkerStopController, PortError};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

/// 注入 stop 控制器的行为计划。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannedStop {
    Stopped,
    Stopping,
}

/// 只会计数的 stop 控制器：每次调用记录一次，并按计划返回报告。
struct CountingStopController {
    calls: Arc<AtomicUsize>,
    plan: PlannedStop,
}

impl CountingStopController {
    fn new(calls: Arc<AtomicUsize>, plan: PlannedStop) -> Self {
        Self { calls, plan }
    }
}

#[async_trait]
impl AutomationWorkerStopController for CountingStopController {
    async fn request_stop(
        &self,
        command: &AutomationDispatchCommand,
        request: &AutomationCancelRequest,
    ) -> Result<AutomationStopReport, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (state, effect_started) = match self.plan {
            PlannedStop::Stopped => (AutomationStopState::Stopped, true),
            PlannedStop::Stopping => (AutomationStopState::Stopping, true),
        };
        AutomationStopReport::new(
            command.intent.execution_id.clone(),
            request.attempt,
            request.cancel_generation,
            request.authority_epoch,
            state,
            effect_started,
            A,
        )
        .map_err(|reason| PortError::Failed(reason.to_owned()))
    }
}

fn prepared_intent(suffix: &str) -> AutomationDispatchIntent {
    let mut intent = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: format!("execution-{suffix}"),
        reservation_id: format!("reservation-{suffix}"),
        action_digest: A.to_owned(),
        worker_id: None,
        attempt: 1,
        authority_epoch: 4,
        state: AutomationDispatchState::Prepared,
        observation_digest: None,
        intent_digest: String::new(),
    };
    intent.intent_digest = kiana_domain::json_digest(&json!({
        "schema": intent.schema,
        "execution_id": intent.execution_id,
        "reservation_id": intent.reservation_id,
        "action_digest": intent.action_digest,
        "worker_id": intent.worker_id,
        "attempt": intent.attempt,
        "authority_epoch": intent.authority_epoch,
        "state": intent.state,
        "observation_digest": intent.observation_digest,
    }));
    intent
}

fn prepared_command(suffix: &str) -> AutomationDispatchCommand {
    prepare_automation_dispatch_command(
        format!("command-{suffix}"),
        format!("idem-{suffix}"),
        &prepared_intent(suffix),
    )
    .expect("prepared command")
}

fn claimed(command: &AutomationDispatchCommand) -> AutomationDispatchCommand {
    match claim_automation_dispatch_command(command, "worker-a", 1, command.revision)
        .expect("claim")
    {
        AutomationDispatchClaim::Claimed(claimed) => claimed,
        other => panic!("expected a claimed command, got {other:?}"),
    }
}

fn terminal(command: &AutomationDispatchCommand, unknown: bool) -> AutomationDispatchCommand {
    let claimed = claimed(command);
    let outcome = if unknown {
        AutomationWorkerEffectOutcome::Unknown {
            result_digest: B.to_owned(),
        }
    } else {
        AutomationWorkerEffectOutcome::Succeeded {
            result_digest: B.to_owned(),
        }
    };
    let result = automation_dispatch_result(&claimed, outcome).expect("result");
    let (observed, replayed) =
        observe_automation_dispatch_command(&claimed, &result, claimed.revision).expect("observe");
    assert!(!replayed);
    observed
}

/// 播种一条 Prepared 命令；`claim`/`terminal` 决定是否继续推进到 Claimed 或终态。
async fn seed(
    store: &MemoryAutomationDispatchStore,
    suffix: &str,
    claimed_state: bool,
    terminal_state: Option<bool>,
) -> AutomationDispatchCommand {
    let command_id = format!("command-{suffix}");
    store
        .commit_command(prepared_command(suffix), 0)
        .await
        .expect("commit prepared");
    if claimed_state || terminal_state.is_some() {
        store
            .claim_command(AutomationDispatchClaimRequest {
                command_id: command_id.clone(),
                worker_id: "worker-a".to_owned(),
                attempt: 1,
                expected_revision: 1,
            })
            .await
            .expect("claim");
    }
    if let Some(unknown) = terminal_state {
        let result = terminal(&prepared_command(suffix), unknown)
            .result
            .expect("terminal result");
        store
            .observe_command(AutomationDispatchObservationRequest {
                command_id: command_id.clone(),
                expected_revision: 2,
                result,
            })
            .await
            .expect("observe");
    }
    store.fetch_command(&command_id).await.expect("fetch")
}

fn request(suffix: &str, generation: u64) -> AutomationCancelRequest {
    AutomationCancelRequest::new(format!("execution-{suffix}"), 1, generation, 4, "cancelled")
        .expect("request")
}

fn conflict(reason: &str) -> String {
    PortError::Conflict(reason.to_owned()).to_string()
}

/// 装配一个已启动取消服务的 Host；返回 Host 与三个被测边界。
async fn host_with_service(
    stop: Arc<dyn AutomationWorkerStopController>,
) -> (
    DaemonHost,
    Arc<MemoryAutomationDispatchStore>,
    Arc<MemoryAutomationCancellationStore>,
) {
    let host = DaemonHost::with_harness_and_project_authority(
        KianaHarness::new(Arc::new(ScriptedModel::new(Vec::new()))),
        Arc::new(TrustedProject),
    )
    .expect("daemon host");
    let store = Arc::new(MemoryAutomationDispatchStore::new());
    let cancellations = Arc::new(MemoryAutomationCancellationStore::new());
    host.start_automation_cancellation_service(store.clone(), cancellations.clone(), stop)
        .await
        .expect("start cancellation service on host");
    (host, store, cancellations)
}

#[tokio::test]
async fn daemon_host_owns_the_cancellation_service_and_fails_closed_before_start() {
    // 未启动：任何请求都返回 Unavailable，且不会触碰任何边界。
    let host = DaemonHost::with_harness_and_project_authority(
        KianaHarness::new(Arc::new(ScriptedModel::new(Vec::new()))),
        Arc::new(TrustedProject),
    )
    .expect("daemon host");
    assert_eq!(
        host.automation_cancellation_service()
            .cancel("command-1", request("1", 1))
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:automation_cancellation_service_not_started"
    );
    assert_eq!(
        host.automation_cancellation_service()
            .shutdown()
            .await
            .expect("shutdown without start"),
        AutomationCancellationShutdownReport {
            stopped_count: 0,
            not_executed_count: 0,
            recovery_required_count: 0,
        }
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let (host, store, _cancellations) = host_with_service(Arc::new(CountingStopController::new(
        calls.clone(),
        PlannedStop::Stopped,
    )))
    .await;
    seed(&store, "1", false, None).await;

    // Host 的启动包装是单例：重复 start 被拒绝。
    assert_eq!(
        host.start_automation_cancellation_service(
            store.clone(),
            Arc::new(MemoryAutomationCancellationStore::new()),
            Arc::new(CountingStopController::new(
                calls.clone(),
                PlannedStop::Stopped
            )),
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_cancellation_service_already_started")
    );

    // Host shutdown 会先停止该服务；停止后再调用返回 Unavailable。
    host.shutdown().await.expect("host shutdown");
    assert_eq!(
        host.automation_cancellation_service()
            .cancel("command-1", request("1", 1))
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:automation_cancellation_service_not_started"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn queued_cancellation_is_not_executed_and_claimed_cancellation_stops_with_evidence() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (host, store, cancellations) = host_with_service(Arc::new(CountingStopController::new(
        calls.clone(),
        PlannedStop::Stopped,
    )))
    .await;
    let service: AutomationCancellationService = host.automation_cancellation_service();
    seed(&store, "1", false, None).await;
    seed(&store, "2", true, None).await;

    // queued：没有 effect，也没有 stop 请求。
    let queued = service
        .cancel("command-1", request("1", 1))
        .await
        .expect("queued cancel");
    match queued {
        kiana_core::AutomationCancellationOutcome::NotExecuted(fact) => {
            assert_eq!(fact.state, AutomationCancelState::NotExecuted);
        }
        other => panic!("expected not_executed, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // claimed：恰好一次 stop，并且只有已确认的报告才记成 stopped。
    let claimed = service
        .cancel("command-2", request("2", 1))
        .await
        .expect("claimed cancel");
    match claimed {
        kiana_core::AutomationCancellationOutcome::Stopped(fact) => {
            assert_eq!(fact.state, AutomationCancelState::Stopped);
            assert!(fact.stop_confirmed);
            assert_eq!(fact.stop_evidence_digest.as_deref(), Some(A));
        }
        other => panic!("expected stopped, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 授权/代次围栏在服务边界同样生效。
    let mut stale_epoch = request("2", 2);
    stale_epoch.authority_epoch = 3;
    stale_epoch.recompute_digest();
    assert_eq!(
        service
            .cancel("command-2", stale_epoch)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_authority_epoch_stale")
    );
    assert_eq!(
        service
            .cancel("command-2", request("2", 1))
            .await
            .expect("replay")
            .clone(),
        kiana_core::AutomationCancellationOutcome::Replayed(
            cancellations
                .fetch_cancellation("execution-2", 1)
                .await
                .expect("fetch")
                .expect("recorded")
        )
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let report = service.shutdown().await.expect("shutdown");
    assert_eq!(report.stopped_count, 1);
    assert_eq!(report.not_executed_count, 1);
    assert_eq!(report.recovery_required_count, 0);
}

#[tokio::test]
async fn unconfirmed_and_unknown_cancellations_are_counted_as_recovery() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (host, store, _cancellations) = host_with_service(Arc::new(CountingStopController::new(
        calls.clone(),
        PlannedStop::Stopping,
    )))
    .await;
    let service = host.automation_cancellation_service();
    seed(&store, "1", true, None).await;
    seed(&store, "2", true, Some(true)).await;

    // 未确认停止：只有在途事实，进入 recovery。
    match service
        .cancel("command-1", request("1", 1))
        .await
        .expect("unconfirmed cancel")
    {
        kiana_core::AutomationCancellationOutcome::RecoveryRequired(fact) => {
            assert_eq!(fact.state, AutomationCancelState::Stopping);
            assert!(!fact.stop_confirmed);
        }
        other => panic!("expected recovery, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 未知终态：进入 recovery，但不触碰停止边界。
    match service
        .cancel("command-2", request("2", 1))
        .await
        .expect("unknown cancel")
    {
        kiana_core::AutomationCancellationOutcome::RecoveryRequired(fact) => {
            assert_eq!(fact.state, AutomationCancelState::ResultUnknown);
            assert!(fact.effect_started);
        }
        other => panic!("expected recovery, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 已观测终态：取消被拒，停止边界次数不变。
    seed(&store, "3", true, Some(false)).await;
    assert_eq!(
        service
            .cancel("command-3", request("3", 1))
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_execution_terminal")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let report = service.shutdown().await.expect("shutdown");
    assert_eq!(report.stopped_count, 0);
    assert_eq!(report.not_executed_count, 0);
    assert_eq!(report.recovery_required_count, 2);
}

#[tokio::test]
async fn late_results_are_fenced_and_never_counted_as_stopped() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (host, store, _cancellations) = host_with_service(Arc::new(CountingStopController::new(
        calls.clone(),
        PlannedStop::Stopped,
    )))
    .await;
    let service = host.automation_cancellation_service();
    seed(&store, "1", true, None).await;
    seed(&store, "2", true, None).await;
    assert!(matches!(
        service
            .cancel("command-1", request("1", 1))
            .await
            .expect("cancel one"),
        kiana_core::AutomationCancellationOutcome::Stopped(_)
    ));
    assert!(matches!(
        service
            .cancel("command-2", request("2", 1))
            .await
            .expect("cancel two"),
        kiana_core::AutomationCancellationOutcome::Stopped(_)
    ));

    // 没有取消事实的 execution 不能隔离迟到结果。
    let orphan = AutomationLateResult::new("execution-3", 1, 1, 4, B).expect("late");
    assert_eq!(
        service
            .record_late_result(orphan)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_fact_unknown")
    );

    // 迟到结果把第二条取消隔离成 Fenced，绝不改写第一条已确认的停止。
    let late = AutomationLateResult::new("execution-2", 1, 1, 4, B).expect("late");
    let fenced = service
        .record_late_result(late.clone())
        .await
        .expect("fence");
    assert_eq!(fenced.fact.state, AutomationCancelState::Fenced);
    assert!(fenced.fact.late_result);
    assert!(fenced.fact.stop_confirmed);
    assert_eq!(fenced.fact.stop_evidence_digest.as_deref(), Some(A));
    assert_eq!(
        service
            .record_late_result(late)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_already_fenced")
    );
    let fetched = service
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .expect("recorded");
    assert_eq!(fetched.state, AutomationCancelState::Stopped);

    // 已隔离的取消计入 recovery，不能被说成 stopped。
    let report = service.shutdown().await.expect("shutdown");
    assert_eq!(report.stopped_count, 1);
    assert_eq!(report.not_executed_count, 0);
    assert_eq!(report.recovery_required_count, 1);
}
