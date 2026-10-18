//! AUT-15 先拒绝优先夹具：Core 协调器把可重取命令拆成 dispatch 与 observation 两段，
//! 拒绝、重放、并发竞争与“提交不确定”都必须保持 0 次或恰好 1 次 effect，绝不伪造成功。
//!
//! 夹具驱动真实的 `MemoryAutomationDispatchStore` 与注入的计数 runner；`calls` 计数器代表
//! “调用方真正触达外部 effect 的 runner 调用”。本文件不连接 EventLog、Broker、模型或网络。

use async_trait::async_trait;
use kiana_core::{dispatch_automation_worker_command, AutomationWorkerDispatchOutcome};
use kiana_domain::{
    prepare_automation_dispatch_command, AutomationDispatchClaimCommit,
    AutomationDispatchClaimRequest, AutomationDispatchCommand, AutomationDispatchCommit,
    AutomationDispatchIntent, AutomationDispatchObservationCommit,
    AutomationDispatchObservationRequest, AutomationDispatchState, AutomationWorkerEffectOutcome,
    AUTOMATION_DISPATCH_SCHEMA,
};
use kiana_eventlog::MemoryAutomationDispatchStore;
use kiana_ports::{AutomationDispatchStore, AutomationWorkerEffectRunner, PortError};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 注入 runner 的行为计划；`NotADigest` 用来模拟 runner 返回非法形状。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Plan {
    Succeeded,
    Failed,
    Unknown,
    NotADigest,
}

/// 只会计数的 runner：每次调用都记录一次，并返回计划中的 outcome。
struct CountingRunner {
    calls: Arc<AtomicUsize>,
    plan: Plan,
}

impl CountingRunner {
    fn new(calls: Arc<AtomicUsize>, plan: Plan) -> Self {
        Self { calls, plan }
    }
}

#[async_trait]
impl AutomationWorkerEffectRunner for CountingRunner {
    async fn run_dispatch_attempt(
        &self,
        _command: &AutomationDispatchCommand,
    ) -> AutomationWorkerEffectOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.plan {
            Plan::Succeeded => AutomationWorkerEffectOutcome::Succeeded {
                result_digest: A.to_owned(),
            },
            Plan::Failed => AutomationWorkerEffectOutcome::Failed {
                failure_code: "connector_timeout".to_owned(),
                detail_digest: B.to_owned(),
                retryable: true,
                result_digest: B.to_owned(),
            },
            Plan::Unknown => AutomationWorkerEffectOutcome::Unknown {
                result_digest: A.to_owned(),
            },
            Plan::NotADigest => AutomationWorkerEffectOutcome::Succeeded {
                result_digest: "not-a-digest".to_owned(),
            },
        }
    }
}

/// 观察提交永远失败的 store 包装：用来证明“提交不确定”返回 Unknown，而不是成功或失败。
struct ObserveFailingStore {
    inner: Arc<MemoryAutomationDispatchStore>,
}

#[async_trait]
impl AutomationDispatchStore for ObserveFailingStore {
    async fn commit_command(
        &self,
        command: AutomationDispatchCommand,
        expected_revision: u64,
    ) -> Result<AutomationDispatchCommit, PortError> {
        self.inner.commit_command(command, expected_revision).await
    }

    async fn claim_command(
        &self,
        request: AutomationDispatchClaimRequest,
    ) -> Result<AutomationDispatchClaimCommit, PortError> {
        self.inner.claim_command(request).await
    }

    async fn observe_command(
        &self,
        _request: AutomationDispatchObservationRequest,
    ) -> Result<AutomationDispatchObservationCommit, PortError> {
        Err(PortError::Failed(
            "automation_dispatch_observe_unavailable".to_owned(),
        ))
    }

    async fn fetch_command(
        &self,
        command_id: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        self.inner.fetch_command(command_id).await
    }

    async fn fetch_command_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<AutomationDispatchCommand, PortError> {
        self.inner
            .fetch_command_by_idempotency_key(idempotency_key)
            .await
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
    PortError::Conflict(reason.to_owned()).to_string()
}

#[tokio::test]
async fn dispatch_command_is_refetched_and_replays_without_a_second_effect() {
    let store = MemoryAutomationDispatchStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = CountingRunner::new(calls.clone(), Plan::Succeeded);
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    let observed = dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 1)
        .await
        .expect("first dispatch");
    let command = match observed {
        AutomationWorkerDispatchOutcome::Observed(command) => command,
        other => panic!("expected an observed command, got {other:?}"),
    };
    assert_eq!(command.state(), AutomationDispatchState::Observed);
    assert_eq!(command.revision, 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 同一 worker/attempt 重放：不执行第二次 effect。
    let replayed = dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 1)
        .await
        .expect("replay");
    match replayed {
        AutomationWorkerDispatchOutcome::Replayed(command) => {
            assert_eq!(command.state(), AutomationDispatchState::Observed)
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 其他 worker / 第二个 attempt 都以稳定冲突码被拒，且同样不触碰 runner。
    for (worker, attempt) in [("worker-b", 1u32), ("worker-a", 2)] {
        assert_eq!(
            dispatch_automation_worker_command(&store, &runner, "command-1", worker, attempt)
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_dispatch_worker_conflict")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 结果可重放：store 中的终态命令带结构化结果，幂等键也能取到同一副本。
    let stored = store.fetch_command("command-1").await.expect("fetch");
    assert_eq!(stored, command);
    assert!(stored
        .result
        .as_ref()
        .expect("terminal result")
        .failure
        .is_none());
    assert_eq!(
        store
            .fetch_command_by_idempotency_key("idem-1")
            .await
            .expect("fetch by key"),
        stored
    );
}

#[tokio::test]
async fn unknown_commands_and_illegal_identity_never_reach_the_runner() {
    let store = MemoryAutomationDispatchStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = CountingRunner::new(calls.clone(), Plan::Succeeded);

    // 命令不存在：0 次 effect。
    assert_eq!(
        dispatch_automation_worker_command(&store, &runner, "command-missing", "worker-a", 1)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_command_unknown")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    // 非法 worker 文本与 attempt=0 属于契约错误：同样 0 次 effect。
    assert_eq!(
        dispatch_automation_worker_command(&store, &runner, "command-1", "worker a", 1)
            .await
            .unwrap_err()
            .to_string(),
        "port_failed:automation_dispatch_command_invalid"
    );
    assert_eq!(
        dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 0)
            .await
            .unwrap_err()
            .to_string(),
        "port_failed:automation_dispatch_command_invalid"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        store
            .fetch_command("command-1")
            .await
            .expect("fetch")
            .state(),
        AutomationDispatchState::Prepared
    );
}

#[tokio::test]
async fn structured_failure_and_unknown_are_recorded_without_a_second_effect() {
    let store = MemoryAutomationDispatchStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = CountingRunner::new(calls.clone(), Plan::Failed);
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    let observed = dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 1)
        .await
        .expect("failed dispatch");
    let command = match observed {
        AutomationWorkerDispatchOutcome::Observed(command) => command,
        other => panic!("expected an observed failure, got {other:?}"),
    };
    let result = command.result.as_ref().expect("terminal result");
    assert_eq!(
        result.observation.outcome,
        kiana_domain::AutomationObservationOutcome::Failed
    );
    assert_eq!(result.observation.result_digest, B);
    assert_eq!(result.observation.worker_id, "worker-a");
    let failure = result.failure.as_ref().expect("structured failure");
    assert_eq!(failure.code, "connector_timeout");
    assert!(failure.retryable);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // 终态命令上任何 worker 重新 dispatch 都只重放，不再执行 effect。
    match dispatch_automation_worker_command(&store, &runner, "command-1", "worker-b", 1)
        .await
        .expect("terminal replay")
    {
        AutomationWorkerDispatchOutcome::Replayed(replayed) => assert_eq!(replayed, command),
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn uncertain_result_commits_stay_unknown_and_never_fabricate_success() {
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = CountingRunner::new(calls.clone(), Plan::Succeeded);

    // 观察提交不可用：effect 已经尝试，命令必须停在 Claimed/Unknown，不得说成成功。
    let inner = Arc::new(MemoryAutomationDispatchStore::new());
    let store = ObserveFailingStore {
        inner: inner.clone(),
    };
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");
    let uncertain = dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 1)
        .await
        .expect("uncertain dispatch");
    let command = match uncertain {
        AutomationWorkerDispatchOutcome::Unknown(command) => command,
        other => panic!("expected an unknown outcome, got {other:?}"),
    };
    assert_eq!(command.state(), AutomationDispatchState::Claimed);
    assert_eq!(command.revision, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        inner
            .fetch_command("command-1")
            .await
            .expect("fetch")
            .state(),
        AutomationDispatchState::Claimed
    );

    // 崩溃重启后重取：同一 worker/attempt 只重放，其他 worker 被拒；effect 始终只有一次。
    match dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 1)
        .await
        .expect("re-fetch after crash")
    {
        AutomationWorkerDispatchOutcome::Replayed(command) => {
            assert_eq!(command.state(), AutomationDispatchState::Claimed)
        }
        other => panic!("expected a replay, got {other:?}"),
    }
    assert_eq!(
        dispatch_automation_worker_command(&store, &runner, "command-1", "worker-b", 1)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_worker_conflict")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn runner_reported_unknown_and_illegal_shapes_never_become_success() {
    // runner 自报 Unknown：记录为 Unknown 终态，并保留对账所需的信息。
    let store = MemoryAutomationDispatchStore::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = CountingRunner::new(calls.clone(), Plan::Unknown);
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");
    let unknown = dispatch_automation_worker_command(&store, &runner, "command-1", "worker-a", 1)
        .await
        .expect("unknown dispatch");
    let command = match unknown {
        AutomationWorkerDispatchOutcome::Unknown(command) => command,
        other => panic!("expected an unknown outcome, got {other:?}"),
    };
    assert_eq!(command.state(), AutomationDispatchState::Unknown);
    let result = command.result.as_ref().expect("terminal result");
    assert_eq!(
        result.observation.outcome,
        kiana_domain::AutomationObservationOutcome::Unknown
    );
    assert!(result.failure.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // runner 返回非法摘要形状：effect 已尝试，因此停在 Claimed 的 Unknown，而不是伪造成功。
    let illegal_store = MemoryAutomationDispatchStore::new();
    let illegal_calls = Arc::new(AtomicUsize::new(0));
    let illegal_runner = CountingRunner::new(illegal_calls.clone(), Plan::NotADigest);
    illegal_store
        .commit_command(prepared_command("command-2", "idem-2"), 0)
        .await
        .expect("commit");
    let illegal = dispatch_automation_worker_command(
        &illegal_store,
        &illegal_runner,
        "command-2",
        "worker-a",
        1,
    )
    .await
    .expect("illegal shape dispatch");
    match illegal {
        AutomationWorkerDispatchOutcome::Unknown(command) => {
            assert_eq!(command.state(), AutomationDispatchState::Claimed);
            assert!(command.result.is_none());
        }
        other => panic!("expected an unknown outcome, got {other:?}"),
    }
    assert_eq!(illegal_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        illegal_store
            .fetch_command("command-2")
            .await
            .expect("fetch")
            .state(),
        AutomationDispatchState::Claimed
    );
}

#[tokio::test]
async fn concurrent_workers_execute_exactly_one_effect() {
    let store = Arc::new(MemoryAutomationDispatchStore::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(CountingRunner::new(calls.clone(), Plan::Succeeded));
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    let mut handles = Vec::new();
    for worker in ["worker-a", "worker-b"] {
        let store = store.clone();
        let runner = runner.clone();
        handles.push(tokio::spawn(async move {
            dispatch_automation_worker_command(&*store, &*runner, "command-1", worker, 1).await
        }));
    }
    let mut executed = 0;
    let mut denied = 0;
    for handle in handles {
        match handle.await.expect("join") {
            Ok(AutomationWorkerDispatchOutcome::Observed(_)) => executed += 1,
            Ok(AutomationWorkerDispatchOutcome::Replayed(_)) => denied += 1,
            Ok(other) => panic!("unexpected outcome: {other:?}"),
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    conflict("automation_dispatch_worker_conflict")
                );
                denied += 1;
            }
        }
    }
    assert_eq!(executed, 1, "exactly one worker runs the effect");
    assert_eq!(denied, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let stored = store.fetch_command("command-1").await.expect("fetch");
    assert_eq!(stored.state(), AutomationDispatchState::Observed);
}
