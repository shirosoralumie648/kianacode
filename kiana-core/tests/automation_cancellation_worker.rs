//! AUT-17 先拒绝优先夹具：Core 取消协调器按已提交命令状态判定 not_executed / stop / recovery，
//! 迟到结果只能被隔离，任何不确定路径都不返回成功。
//!
//! 夹具驱动真实的 `MemoryAutomationDispatchStore` 与 `MemoryAutomationCancellationStore`，以及一个
//! 只会计数的 stop 控制器；`calls` 计数器代表“调用方真正触达停止边界的 stop 请求次数”。本文件不
//! 连接 EventLog、Broker、模型或网络。

use async_trait::async_trait;
use kiana_core::{
    cancel_automation_worker_command, record_automation_late_result, AutomationCancellationOutcome,
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
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 注入 stop 控制器的行为计划。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StopPlan {
    /// 返回指定分类的报告（effect_started 由分类决定）。
    Report(AutomationStopState),
    /// 停止边界不可用。
    Unavailable,
    /// 返回一份代次漂移的报告：调用方不得采纳。
    Drifted,
}

/// 只会计数的 stop 控制器：每次调用记录一次，并按计划返回报告。
struct CountingStopController {
    calls: Arc<AtomicUsize>,
    plan: StopPlan,
}

impl CountingStopController {
    fn new(calls: Arc<AtomicUsize>, plan: StopPlan) -> Self {
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
        match self.plan {
            StopPlan::Report(state) => {
                let effect_started = matches!(
                    state,
                    AutomationStopState::Stopping | AutomationStopState::Stopped
                );
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
            StopPlan::Unavailable => Err(PortError::Unavailable(
                "automation_stop_controller_unavailable".to_owned(),
            )),
            StopPlan::Drifted => AutomationStopReport::new(
                command.intent.execution_id.clone(),
                request.attempt,
                request.cancel_generation + 1,
                request.authority_epoch,
                AutomationStopState::Stopped,
                true,
                A,
            )
            .map_err(|reason| PortError::Failed(reason.to_owned())),
        }
    }
}

fn prepared_intent() -> AutomationDispatchIntent {
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

fn claimed(command: &AutomationDispatchCommand) -> AutomationDispatchCommand {
    match claim_automation_dispatch_command(command, "worker-a", 1, command.revision)
        .expect("claim")
    {
        AutomationDispatchClaim::Claimed(claimed) => claimed,
        other => panic!("expected a claimed command, got {other:?}"),
    }
}

/// 构造目标状态的命令值对象（只用于测试断言，不直接进入 store）。
fn command(state: AutomationDispatchState) -> AutomationDispatchCommand {
    let prepared = prepare_automation_dispatch_command("command-1", "idem-1", &prepared_intent())
        .expect("prepared");
    match state {
        AutomationDispatchState::Prepared => prepared,
        AutomationDispatchState::Claimed => claimed(&prepared),
        AutomationDispatchState::Observed | AutomationDispatchState::Unknown => {
            let claimed = claimed(&prepared);
            let outcome = if state == AutomationDispatchState::Unknown {
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
                observe_automation_dispatch_command(&claimed, &result, claimed.revision)
                    .expect("observe");
            assert!(!replayed);
            observed
        }
    }
}

/// 把一条命令按目标状态推进到 store，并返回权威副本。
///
/// 走的是真实 CAS 路径：commit(Prepared, revision 1) → claim(revision 2) → observe(revision 3)。
async fn store_with(
    store: &MemoryAutomationDispatchStore,
    state: AutomationDispatchState,
) -> AutomationDispatchCommand {
    store
        .commit_command(command(AutomationDispatchState::Prepared), 0)
        .await
        .expect("commit prepared");
    if matches!(
        state,
        AutomationDispatchState::Claimed
            | AutomationDispatchState::Observed
            | AutomationDispatchState::Unknown
    ) {
        store
            .claim_command(AutomationDispatchClaimRequest {
                command_id: "command-1".to_owned(),
                worker_id: "worker-a".to_owned(),
                attempt: 1,
                expected_revision: 1,
            })
            .await
            .expect("claim");
    }
    if matches!(
        state,
        AutomationDispatchState::Observed | AutomationDispatchState::Unknown
    ) {
        let result = command(state).result.expect("terminal result");
        store
            .observe_command(AutomationDispatchObservationRequest {
                command_id: "command-1".to_owned(),
                expected_revision: 2,
                result,
            })
            .await
            .expect("observe");
    }
    store.fetch_command("command-1").await.expect("fetch")
}

fn request(generation: u64) -> AutomationCancelRequest {
    AutomationCancelRequest::new("execution-1", 1, generation, 4, "cancelled").expect("request")
}

fn conflict(reason: &str) -> String {
    PortError::Conflict(reason.to_owned()).to_string()
}

#[tokio::test]
async fn queued_cancellation_is_not_executed_and_never_touches_the_stop_controller() {
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(
        calls.clone(),
        StopPlan::Report(AutomationStopState::Stopped),
    );
    store_with(&store, AutomationDispatchState::Prepared).await;

    let outcome =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("cancel");
    let fact = match outcome {
        AutomationCancellationOutcome::NotExecuted(fact) => fact,
        other => panic!("expected not_executed, got {other:?}"),
    };
    assert_eq!(fact.state, AutomationCancelState::NotExecuted);
    assert!(!fact.effect_started);
    assert!(!fact.stop_requested);
    assert!(fact.stop_evidence_digest.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let stored = cancellations
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .expect("recorded");
    assert_eq!(stored.fact_digest, fact.fact_digest);
    // 取消事实不改变命令流：命令仍是 Prepared，effect 从未发生。
    assert_eq!(
        store
            .fetch_command("command-1")
            .await
            .expect("command")
            .state(),
        AutomationDispatchState::Prepared
    );
}

#[tokio::test]
async fn claimed_cancellation_requests_exactly_one_stop_and_records_the_evidence() {
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(
        calls.clone(),
        StopPlan::Report(AutomationStopState::Stopped),
    );
    store_with(&store, AutomationDispatchState::Claimed).await;

    let outcome =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("cancel");
    let fact = match outcome {
        AutomationCancellationOutcome::Stopped(fact) => fact,
        other => panic!("expected stopped, got {other:?}"),
    };
    assert_eq!(fact.state, AutomationCancelState::Stopped);
    assert!(fact.stop_requested);
    assert!(fact.stop_confirmed);
    assert!(fact.effect_started);
    assert_eq!(fact.stop_evidence_digest.as_deref(), Some(A));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 同一代次同请求重放：不再写事实、不再请求 stop。
    let replayed =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("replay");
    match replayed {
        AutomationCancellationOutcome::Replayed(fact) => {
            assert_eq!(fact.state, AutomationCancelState::Stopped)
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unconfirmed_or_unavailable_stops_are_recovery_only() {
    // A) 未确认停止：记录在途事实，进入 recovery；重放不再请求第二次 stop。
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(
        calls.clone(),
        StopPlan::Report(AutomationStopState::Stopping),
    );
    store_with(&store, AutomationDispatchState::Claimed).await;

    let outcome =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("cancel");
    let fact = match outcome {
        AutomationCancellationOutcome::RecoveryRequired(fact) => fact,
        other => panic!("expected recovery, got {other:?}"),
    };
    assert_eq!(fact.state, AutomationCancelState::Stopping);
    assert!(!fact.stop_confirmed);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let replayed =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("replay");
    match replayed {
        AutomationCancellationOutcome::Replayed(fact) => {
            assert_eq!(fact.state, AutomationCancelState::Stopping)
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // B) 停止边界不可用：在途事实保留，重试只重发 stop，绝不产生第二次 effect。
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(calls.clone(), StopPlan::Unavailable);
    store_with(&store, AutomationDispatchState::Claimed).await;

    let outcome =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("cancel");
    let fact = match outcome {
        AutomationCancellationOutcome::RecoveryRequired(fact) => fact,
        other => panic!("expected recovery, got {other:?}"),
    };
    assert_eq!(fact.state, AutomationCancelState::Requested);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let stored = cancellations
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .expect("recorded");
    assert_eq!(stored.state, AutomationCancelState::Requested);

    // 在途重放：同一代次只重新请求 stop（幂等），仍然不返回成功。
    let retried =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("retry");
    match retried {
        AutomationCancellationOutcome::RecoveryRequired(fact) => {
            assert_eq!(fact.state, AutomationCancelState::Requested)
        }
        other => panic!("expected recovery, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn drifted_stop_reports_are_never_adopted() {
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(calls.clone(), StopPlan::Drifted);
    store_with(&store, AutomationDispatchState::Claimed).await;

    let outcome =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("cancel");
    let fact = match outcome {
        AutomationCancellationOutcome::RecoveryRequired(fact) => fact,
        other => panic!("expected recovery, got {other:?}"),
    };
    // 漂移报告不被采纳：仍然是未确认的在途事实，没有 stop_confirmed。
    assert_eq!(fact.state, AutomationCancelState::Requested);
    assert!(!fact.stop_confirmed);
    assert!(fact.stop_evidence_digest.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn terminal_and_unknown_commands_fail_closed() {
    // 已观测终态：取消被拒，且不触碰 stop 控制器。
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(
        calls.clone(),
        StopPlan::Report(AutomationStopState::Stopped),
    );
    store_with(&store, AutomationDispatchState::Observed).await;
    assert_eq!(
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_execution_terminal")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(cancellations
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .is_none());

    // 未知终态：直接进入 recovery，不假装能停止。
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    store_with(&store, AutomationDispatchState::Unknown).await;
    let outcome =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
            .await
            .expect("cancel");
    match outcome {
        AutomationCancellationOutcome::RecoveryRequired(fact) => {
            assert_eq!(fact.state, AutomationCancelState::ResultUnknown);
            assert!(fact.effect_started);
            assert!(fact.recovery_required());
        }
        other => panic!("expected recovery, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stale_or_conflicting_requests_never_re_request_a_stop() {
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(
        calls.clone(),
        StopPlan::Report(AutomationStopState::Stopped),
    );
    store_with(&store, AutomationDispatchState::Claimed).await;

    let stopped =
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(5))
            .await
            .expect("cancel");
    assert!(matches!(stopped, AutomationCancellationOutcome::Stopped(_)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 陈旧代次。
    assert_eq!(
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(4))
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_generation_stale")
    );
    // 同代次不同内容。
    let mut conflicting = request(5);
    conflicting.reason = "operator-request".to_owned();
    conflicting.recompute_digest();
    assert_eq!(
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &conflicting)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_generation_conflict")
    );
    // 更高代次但 epoch 与命令不符。
    let mut stale_epoch = request(6);
    stale_epoch.authority_epoch = 3;
    stale_epoch.recompute_digest();
    assert_eq!(
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &stale_epoch)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_authority_epoch_stale")
    );
    // attempt 漂移。
    let mut wrong_attempt = request(7);
    wrong_attempt.attempt = 2;
    wrong_attempt.recompute_digest();
    assert_eq!(
        cancel_automation_worker_command(
            &store,
            &cancellations,
            &stop,
            "command-1",
            &wrong_attempt
        )
        .await
        .unwrap_err()
        .to_string(),
        conflict("automation_cancel_attempt_drift")
    );
    // 不存在的命令。
    assert_eq!(
        cancel_automation_worker_command(&store, &cancellations, &stop, "command-2", &request(8))
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_command_unknown")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn late_results_are_fenced_and_cannot_become_a_success() {
    let store = MemoryAutomationDispatchStore::new();
    let cancellations = MemoryAutomationCancellationStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let stop = CountingStopController::new(
        calls.clone(),
        StopPlan::Report(AutomationStopState::Stopped),
    );
    store_with(&store, AutomationDispatchState::Claimed).await;
    cancel_automation_worker_command(&store, &cancellations, &stop, "command-1", &request(1))
        .await
        .expect("cancel");

    // 没有取消事实的 execution 不能隔离迟到结果。
    let orphan = AutomationLateResult::new("execution-2", 1, 1, 4, B).expect("late");
    assert_eq!(
        record_automation_late_result(&cancellations, &orphan)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_fact_unknown")
    );

    let late = AutomationLateResult::new("execution-1", 1, 1, 4, B).expect("late");
    let commit = record_automation_late_result(&cancellations, &late)
        .await
        .expect("fence");
    assert!(!commit.replayed);
    assert_eq!(commit.fact.state, AutomationCancelState::Fenced);
    assert!(commit.fact.late_result);
    // 停止历史被保留，没有被迟到结果改写。
    assert!(commit.fact.stop_confirmed);
    assert_eq!(commit.fact.stop_evidence_digest.as_deref(), Some(A));
    assert!(commit.fact.recovery_required());

    // 二次隔离被拒；迟到结果不能把状态改回任何活着的值。
    assert_eq!(
        record_automation_late_result(&cancellations, &late)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_already_fenced")
    );
    let stored = cancellations
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .expect("recorded");
    assert_eq!(stored.state, AutomationCancelState::Fenced);
    // 命令流仍未改变：取消与隔离都不伪造执行结果。
    assert_eq!(
        store
            .fetch_command("command-1")
            .await
            .expect("command")
            .state(),
        AutomationDispatchState::Claimed
    );
}
