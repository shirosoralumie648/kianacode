//! AUT-15 daemon 夹具：DaemonHost 持有的 dispatch/observation 服务把可重取命令的
//! 派发与观察分开，崩溃窗口、重复 worker 与观察竞态都不得重复 effect 或伪造 success。
//!
//! 夹具驱动真实的 `DaemonHost` + `MemoryAutomationDispatchStore` + 计数 runner；runner 代表
//! “调用方真正触达外部 effect 的边界”。本文件不连接模型、网络或真实 provider。

use async_trait::async_trait;
use kiana_daemon::{
    AutomationDispatchService, AutomationDispatchShutdownReport, DaemonHost, ProjectTrustAuthority,
};
use kiana_domain::{
    prepare_automation_dispatch_command, AutomationDispatchClaimRequest, AutomationDispatchCommand,
    AutomationDispatchIntent, AutomationDispatchObservationRequest, AutomationDispatchState,
    AutomationWorkerEffectOutcome, AUTOMATION_DISPATCH_SCHEMA,
};
use kiana_eventlog::MemoryAutomationDispatchStore;
use kiana_ports::AutomationWorkerEffectRunner;
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

/// 只会计数的 runner：每次调用记录一次并返回成功；非法形状与未知结果由计划决定。
struct CountingRunner {
    calls: Arc<AtomicUsize>,
    outcome: PlannedOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlannedOutcome {
    Succeeded,
    Unknown,
}

impl CountingRunner {
    fn new(calls: Arc<AtomicUsize>, outcome: PlannedOutcome) -> Self {
        Self { calls, outcome }
    }
}

#[async_trait]
impl AutomationWorkerEffectRunner for CountingRunner {
    async fn run_dispatch_attempt(
        &self,
        _command: &AutomationDispatchCommand,
    ) -> AutomationWorkerEffectOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.outcome {
            PlannedOutcome::Succeeded => AutomationWorkerEffectOutcome::Succeeded {
                result_digest: A.to_owned(),
            },
            PlannedOutcome::Unknown => AutomationWorkerEffectOutcome::Unknown {
                result_digest: A.to_owned(),
            },
        }
    }
}

fn recompute_intent(value: &mut AutomationDispatchIntent) {
    value.intent_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "execution_id": value.execution_id,
        "reservation_id": value.reservation_id,
        "action_digest": value.action_digest,
        "worker_id": value.worker_id,
        "attempt": value.attempt,
        "authority_epoch": value.authority_epoch,
        "state": value.state,
        "observation_digest": value.observation_digest,
    }));
}

fn prepared_command(command_id: &str, idempotency_key: &str) -> AutomationDispatchCommand {
    let mut intent = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: A.to_owned(),
        worker_id: None,
        attempt: 1,
        authority_epoch: 4,
        state: AutomationDispatchState::Prepared,
        observation_digest: None,
        intent_digest: String::new(),
    };
    recompute_intent(&mut intent);
    prepare_automation_dispatch_command(command_id, idempotency_key, &intent)
        .expect("prepared command")
}

fn conflict(reason: &str) -> String {
    kiana_ports::PortError::Conflict(reason.to_owned()).to_string()
}

/// 一个已启动、注入内存 store 与计数 runner 的 daemon 服务。
async fn service(
    calls: &Arc<AtomicUsize>,
    outcome: PlannedOutcome,
) -> (
    AutomationDispatchService,
    Arc<MemoryAutomationDispatchStore>,
) {
    let store = Arc::new(MemoryAutomationDispatchStore::new());
    let service = AutomationDispatchService::new();
    service
        .start(
            store.clone(),
            Arc::new(CountingRunner::new(calls.clone(), outcome)),
        )
        .await
        .expect("start service");
    (service, store)
}

#[tokio::test]
async fn commit_and_dispatch_are_separate_phases_and_replays_never_rerun_the_effect() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (service, store) = service(&calls, PlannedOutcome::Succeeded).await;

    // 派发前：命令只是可重取事实，尚未绑定 worker，也没有 effect。
    let committed = service
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");
    assert!(!committed.replayed);
    assert_eq!(committed.command.state(), AutomationDispatchState::Prepared);
    let committed_again = service
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("idempotent re-fetch");
    assert!(committed_again.replayed);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 观察阶段：dispatch 重取命令、claim、执行一次 effect 并记录结果。
    let observed = service
        .dispatch("command-1", "worker-a", 1)
        .await
        .expect("dispatch");
    let command = match observed {
        kiana_core::AutomationWorkerDispatchOutcome::Observed(command) => command,
        other => panic!("expected an observed command, got {other:?}"),
    };
    assert_eq!(command.state(), AutomationDispatchState::Observed);
    assert_eq!(command.revision, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        service
            .fetch_command("command-1")
            .await
            .expect("fetch through service"),
        command
    );

    // 同一 worker/attempt 重放；其他 worker/attempt 被拒；effect 计数不变。
    match service
        .dispatch("command-1", "worker-a", 1)
        .await
        .expect("replay")
    {
        kiana_core::AutomationWorkerDispatchOutcome::Replayed(replayed) => {
            assert_eq!(replayed, command)
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(
        service
            .dispatch("command-1", "worker-b", 1)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_worker_conflict")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 关闭报告：确定结果的命令计入 observed，而不是 recovery。
    let report = service.shutdown().await.expect("shutdown");
    assert_eq!(
        report,
        AutomationDispatchShutdownReport {
            observed_count: 1,
            recovery_required_count: 0,
        }
    );
    assert_eq!(store.revision("execution-1").await, 3);
}

#[tokio::test]
async fn a_claimed_command_replays_after_restart_without_a_second_effect() {
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(MemoryAutomationDispatchStore::new());
    let service = AutomationDispatchService::new();
    service
        .start(
            store.clone(),
            Arc::new(CountingRunner::new(
                calls.clone(),
                PlannedOutcome::Succeeded,
            )),
        )
        .await
        .expect("start service");
    service
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    // 崩溃窗口：claim 已经提交，但 worker 在观察前消失（测试用公开 store 路径播种同一状态）。
    let claimed = store
        .claim_command(AutomationDispatchClaimRequest {
            command_id: "command-1".to_owned(),
            worker_id: "worker-a".to_owned(),
            attempt: 1,
            expected_revision: 1,
        })
        .await
        .expect("seeded claim")
        .command;
    assert_eq!(claimed.state(), AutomationDispatchState::Claimed);

    // 重启服务（新 service handle，同一个 store）：重取命令、重放，不重复 effect。
    service.shutdown().await.expect("shutdown first service");
    let restarted = AutomationDispatchService::new();
    restarted
        .start(
            store.clone(),
            Arc::new(CountingRunner::new(
                calls.clone(),
                PlannedOutcome::Succeeded,
            )),
        )
        .await
        .expect("restart service");
    match restarted
        .dispatch("command-1", "worker-a", 1)
        .await
        .expect("replay after restart")
    {
        kiana_core::AutomationWorkerDispatchOutcome::Replayed(command) => {
            assert_eq!(command.state(), AutomationDispatchState::Claimed)
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(
        restarted
            .dispatch("command-1", "worker-b", 1)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_worker_conflict")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // 关闭报告把未对账的 claim 计入 recovery-required。
    let report = restarted.shutdown().await.expect("shutdown restarted");
    assert_eq!(
        report,
        AutomationDispatchShutdownReport {
            observed_count: 0,
            recovery_required_count: 1,
        }
    );
}

#[tokio::test]
async fn unknown_results_and_observation_races_are_counted_as_recovery_only() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (service, store) = service(&calls, PlannedOutcome::Unknown).await;
    service
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    let unknown = service
        .dispatch("command-1", "worker-a", 1)
        .await
        .expect("unknown dispatch");
    match unknown {
        kiana_core::AutomationWorkerDispatchOutcome::Unknown(command) => {
            assert_eq!(command.state(), AutomationDispatchState::Unknown)
        }
        other => panic!("expected an unknown outcome, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 终态后的观察竞态：异构结果（成功）不能把 Unknown 改写成成功；相同结果只重放。
    let stored = service.fetch_command("command-1").await.expect("fetch");
    assert_eq!(stored.state(), AutomationDispatchState::Unknown);
    // 手工构造“迟到成功”结果：它与已记录观察身份一致但内容不同，因此只能被拒。
    let forged = kiana_domain::AutomationDispatchResult {
        observation: kiana_domain::AutomationObservation {
            execution_id: "execution-1".to_owned(),
            reservation_id: "reservation-1".to_owned(),
            action_digest: A.to_owned(),
            worker_id: "worker-a".to_owned(),
            authority_epoch: 4,
            outcome: kiana_domain::AutomationObservationOutcome::Succeeded,
            result_digest: A.to_owned(),
        },
        failure: None,
    };
    assert_eq!(forged.validate(), Ok(()));
    assert_eq!(
        store
            .observe_command(AutomationDispatchObservationRequest {
                command_id: "command-1".to_owned(),
                expected_revision: stored.revision,
                result: forged,
            })
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_observation_conflict")
    );
    let recorded = stored
        .result
        .as_ref()
        .expect("recorded unknown result")
        .clone();
    let replayed = store
        .observe_command(AutomationDispatchObservationRequest {
            command_id: "command-1".to_owned(),
            expected_revision: stored.revision,
            result: recorded,
        })
        .await
        .expect("identical replay");
    assert!(replayed.replayed);
    assert_eq!(
        service
            .fetch_command("command-1")
            .await
            .expect("fetch")
            .state(),
        AutomationDispatchState::Unknown
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Unknown 终态计入 recovery-required，绝不计入 observed。
    let report = service.shutdown().await.expect("shutdown");
    assert_eq!(
        report,
        AutomationDispatchShutdownReport {
            observed_count: 0,
            recovery_required_count: 1,
        }
    );
}

#[tokio::test]
async fn daemon_host_owns_the_service_and_shuts_it_down_with_the_spine() {
    // 服务由 DaemonHost 装配：注入 store/runner 后只能通过 Host 的 handle 使用。
    let host = DaemonHost::with_harness_and_project_authority(
        KianaHarness::new(Arc::new(ScriptedModel::new(Vec::new()))),
        Arc::new(TrustedProject),
    )
    .expect("daemon host");
    let store = Arc::new(MemoryAutomationDispatchStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    host.start_automation_dispatch_service(
        store.clone(),
        Arc::new(CountingRunner::new(
            calls.clone(),
            PlannedOutcome::Succeeded,
        )),
    )
    .await
    .expect("start service on host");
    let service = host.automation_dispatch_service();

    service
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit through host service");
    match service
        .dispatch("command-1", "worker-a", 1)
        .await
        .expect("dispatch through host service")
    {
        kiana_core::AutomationWorkerDispatchOutcome::Observed(command) => {
            assert_eq!(command.state(), AutomationDispatchState::Observed)
        }
        other => panic!("expected an observed command, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Host 的既有启动包装是单例：重复 start 被拒绝，服务不能出现第二个 worker。
    assert_eq!(
        host.start_automation_dispatch_service(
            store,
            Arc::new(CountingRunner::new(
                calls.clone(),
                PlannedOutcome::Succeeded
            )),
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_dispatch_service_already_started")
    );

    // Host shutdown 会先停止该服务；停止后再调用返回 Unavailable，effect 计数不再变化。
    host.shutdown().await.expect("host shutdown");
    assert_eq!(
        service
            .dispatch("command-1", "worker-b", 1)
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:automation_dispatch_service_not_started"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
