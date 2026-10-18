//! AUT-15 先拒绝优先夹具：可重取 dispatch command 的 CAS 提交、幂等重取、唯一 claim、
//! 观察 revision 竞态与终态不可覆盖。
//!
//! 夹具只驱动 `MemoryAutomationDispatchStore`（进程内 CI 语义）与 Domain 转移函数；不连接
//! EventLog、Broker、模型或网络，也不执行任何外部动作。它不声称 fsync、崩溃恢复或跨进程 fencing。

use kiana_domain::{
    automation_dispatch_result, prepare_automation_dispatch_command,
    AutomationDispatchClaimRequest, AutomationDispatchCommand, AutomationDispatchIntent,
    AutomationDispatchObservationRequest, AutomationDispatchResult, AutomationDispatchState,
    AutomationObservation, AutomationObservationOutcome, AutomationWorkerEffectOutcome,
    AUTOMATION_DISPATCH_SCHEMA,
};
use kiana_eventlog::MemoryAutomationDispatchStore;
use kiana_ports::PortError;
use serde_json::json;
use std::sync::Arc;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 按 Domain 私有 digest 的同一字段集合重算 intent 摘要，用于篡改夹具。
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

/// 按 Domain 私有 digest 的同一字段集合重算命令摘要。
fn recompute_command(value: &mut AutomationDispatchCommand) {
    let (outcome, result_digest, failure_code, failure_detail_digest, failure_retryable) =
        match &value.result {
            Some(result) => (
                Some(result.observation.outcome),
                Some(result.observation.result_digest.as_str()),
                result.failure.as_ref().map(|failure| failure.code.as_str()),
                result
                    .failure
                    .as_ref()
                    .map(|failure| failure.detail_digest.as_str()),
                result.failure.as_ref().map(|failure| failure.retryable),
            ),
            None => (None, None, None, None, None),
        };
    value.command_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "command_id": value.command_id,
        "idempotency_key": value.idempotency_key,
        "revision": value.revision,
        "intent_digest": value.intent.intent_digest,
        "intent_state": value.intent.state,
        "worker_id": value.intent.worker_id,
        "attempt": value.intent.attempt,
        "observation_digest": value.intent.observation_digest,
        "outcome": outcome,
        "result_digest": result_digest,
        "failure_code": failure_code,
        "failure_detail_digest": failure_detail_digest,
        "failure_retryable": failure_retryable,
    }));
}

fn prepared_command_for(
    execution_id: &str,
    command_id: &str,
    idempotency_key: &str,
) -> AutomationDispatchCommand {
    let mut intent = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: execution_id.to_owned(),
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

fn prepared_command(command_id: &str, idempotency_key: &str) -> AutomationDispatchCommand {
    prepared_command_for("execution-1", command_id, idempotency_key)
}

fn success_result(command: &AutomationDispatchCommand) -> AutomationDispatchResult {
    automation_dispatch_result(
        command,
        AutomationWorkerEffectOutcome::Succeeded {
            result_digest: A.to_owned(),
        },
    )
    .expect("structured success result")
}

fn failure_result(command: &AutomationDispatchCommand) -> AutomationDispatchResult {
    automation_dispatch_result(
        command,
        AutomationWorkerEffectOutcome::Failed {
            failure_code: "connector_timeout".to_owned(),
            detail_digest: B.to_owned(),
            retryable: true,
            result_digest: B.to_owned(),
        },
    )
    .expect("structured failure result")
}

fn claim_request(
    command_id: &str,
    worker_id: &str,
    attempt: u32,
    expected_revision: u64,
) -> AutomationDispatchClaimRequest {
    AutomationDispatchClaimRequest {
        command_id: command_id.to_owned(),
        worker_id: worker_id.to_owned(),
        attempt,
        expected_revision,
    }
}

fn conflict(reason: &str) -> String {
    PortError::Conflict(reason.to_owned()).to_string()
}

#[tokio::test]
async fn committed_command_is_refetched_after_a_crash_and_never_recommits() {
    let store = MemoryAutomationDispatchStore::new();
    let command = prepared_command("command-1", "idem-1");
    let committed = store
        .commit_command(command.clone(), 0)
        .await
        .expect("first commit");
    assert!(!committed.replayed);
    assert_eq!(committed.command, command);
    assert_eq!(store.revision("execution-1").await, 1);

    // “提交后崩溃”：同键同首次摘要携带过期 expected_revision 重取，返回当前权威副本且不新建事实。
    let replayed = store
        .commit_command(command.clone(), 0)
        .await
        .expect("idempotent re-fetch");
    assert!(replayed.replayed);
    assert_eq!(replayed.command.revision, 1);
    assert_eq!(store.revision("execution-1").await, 1);
    assert_eq!(
        store
            .fetch_command_by_idempotency_key("idem-1")
            .await
            .expect("fetch by key"),
        command
    );

    // 同键换内容：即使新命令自洽（execution 漂移后摘要重算），也按键冲突拒绝且不改变存储。
    let mut tampered = prepared_command("command-1", "idem-1");
    tampered.intent.execution_id = "execution-2".to_owned();
    recompute_intent(&mut tampered.intent);
    recompute_command(&mut tampered);
    assert_eq!(tampered.validate(), Ok(()));
    assert_eq!(
        store
            .commit_command(tampered, 0)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_idempotency_conflict")
    );
    assert_eq!(
        store
            .commit_command(prepared_command("command-2", "idem-1"), 0)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_idempotency_conflict")
    );

    // 同 command_id 换幂等键、一个 execution 第二个命令、revision 错位全部拒绝。
    assert_eq!(
        store
            .commit_command(prepared_command("command-1", "idem-2"), 1)
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_command_conflict")
    );
    for expected_revision in [0u64, 2] {
        assert_eq!(
            store
                .commit_command(prepared_command("command-2", "idem-2"), expected_revision)
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_dispatch_revision_conflict")
        );
    }
    assert_eq!(store.revision("execution-1").await, 1);
    assert_eq!(
        store
            .fetch_command("command-2")
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_command_unknown")
    );
}

#[tokio::test]
async fn one_command_yields_exactly_one_worker_attempt() {
    let store = MemoryAutomationDispatchStore::new();
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    let claimed = store
        .claim_command(claim_request("command-1", "worker-a", 1, 1))
        .await
        .expect("first claim");
    assert!(claimed.claimed);
    assert_eq!(claimed.command.revision, 2);
    assert_eq!(claimed.command.state(), AutomationDispatchState::Claimed);

    // 第二个 worker / 第二个 attempt 不可能取得 claim。
    for (worker, attempt) in [("worker-b", 1u32), ("worker-a", 2)] {
        assert_eq!(
            store
                .claim_command(claim_request("command-1", worker, attempt, 2))
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_dispatch_worker_conflict")
        );
    }

    // 同一 worker + attempt 携带过期 revision 是安全重取：claimed=false，不推进 revision。
    let refetched = store
        .claim_command(claim_request("command-1", "worker-a", 1, 1))
        .await
        .expect("safe re-fetch");
    assert!(!refetched.claimed);
    assert_eq!(refetched.command, claimed.command);
    assert_eq!(store.revision("execution-1").await, 2);

    // 未提交命令的 claim 必须 fail-closed；attempt=0 属于契约错误而不是状态冲突。
    assert_eq!(
        store
            .claim_command(claim_request("command-missing", "worker-a", 1, 0))
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_command_unknown")
    );
    assert_eq!(
        store
            .claim_command(claim_request("command-1", "worker-b", 0, 2))
            .await
            .unwrap_err()
            .to_string(),
        "port_failed:automation_dispatch_command_invalid"
    );
}

#[tokio::test]
async fn observation_revision_races_never_overwrite_a_terminal_result() {
    let store = MemoryAutomationDispatchStore::new();
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");
    let claimed = store
        .claim_command(claim_request("command-1", "worker-a", 1, 1))
        .await
        .expect("claim")
        .command;
    assert_eq!(claimed.state(), AutomationDispatchState::Claimed);

    // 未 claim 的命令不能直接观察（第二个 execution 上的一条独立命令）。
    let unclaimed = prepared_command_for("execution-2", "command-2", "idem-2");
    store
        .commit_command(unclaimed.clone(), 0)
        .await
        .expect("commit 2");
    // 手工构造结果：Domain 的结果构造器只接受 Claimed 命令，这里要测的正是“未 claim 就观察”。
    let premature = AutomationDispatchResult {
        observation: AutomationObservation {
            execution_id: "execution-2".to_owned(),
            reservation_id: "reservation-1".to_owned(),
            action_digest: A.to_owned(),
            worker_id: "worker-a".to_owned(),
            authority_epoch: 4,
            outcome: AutomationObservationOutcome::Succeeded,
            result_digest: A.to_owned(),
        },
        failure: None,
    };
    assert_eq!(premature.validate(), Ok(()));
    assert_eq!(
        store
            .observe_command(AutomationDispatchObservationRequest {
                command_id: "command-2".to_owned(),
                expected_revision: unclaimed.revision,
                result: premature,
            })
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_observation_not_claimed")
    );

    let success = success_result(&claimed);
    let observed = store
        .observe_command(AutomationDispatchObservationRequest {
            command_id: "command-1".to_owned(),
            expected_revision: 2,
            result: success.clone(),
        })
        .await
        .expect("first observation");
    assert!(!observed.replayed);
    assert_eq!(observed.command.state(), AutomationDispatchState::Observed);
    assert_eq!(observed.command.revision, 3);

    // 相同结果携带过期 revision = 安全重放，不新建事实。
    let replay = store
        .observe_command(AutomationDispatchObservationRequest {
            command_id: "command-1".to_owned(),
            expected_revision: 2,
            result: success.clone(),
        })
        .await
        .expect("identical replay");
    assert!(replay.replayed);
    assert_eq!(replay.command, observed.command);

    // 异构结果：当前 revision 是观察冲突，陈旧 revision 是版本冲突；终态保持原结果。
    let replacement = failure_result(&claimed);
    assert_eq!(
        store
            .observe_command(AutomationDispatchObservationRequest {
                command_id: "command-1".to_owned(),
                expected_revision: 3,
                result: replacement.clone(),
            })
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_observation_conflict")
    );
    assert_eq!(
        store
            .observe_command(AutomationDispatchObservationRequest {
                command_id: "command-1".to_owned(),
                expected_revision: 2,
                result: replacement,
            })
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_dispatch_revision_conflict")
    );
    let current = store.fetch_command("command-1").await.expect("fetch");
    assert_eq!(current.result.as_ref(), Some(&success));
    assert_eq!(current.revision, 3);

    // 幂等键重取返回**当前**权威副本（已被 claim/观察推进），而不是首次提交时的快照。
    assert_eq!(
        store
            .fetch_command_by_idempotency_key("idem-1")
            .await
            .expect("fetch by key"),
        current
    );
}

#[tokio::test]
async fn concurrent_workers_leave_exactly_one_winning_claim() {
    let store = Arc::new(MemoryAutomationDispatchStore::new());
    store
        .commit_command(prepared_command("command-1", "idem-1"), 0)
        .await
        .expect("commit");

    let mut handles = Vec::new();
    for worker in ["worker-a", "worker-b"] {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            store
                .claim_command(claim_request("command-1", worker, 1, 1))
                .await
        }));
    }
    let mut winners = 0;
    let mut losers = 0;
    for handle in handles {
        match handle.await.expect("join") {
            Ok(commit) if commit.claimed => winners += 1,
            Ok(commit) => {
                assert!(!commit.claimed);
                losers += 1;
            }
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    conflict("automation_dispatch_worker_conflict")
                );
                losers += 1;
            }
        }
    }
    assert_eq!(winners, 1, "exactly one worker owns the attempt");
    assert_eq!(losers, 1);

    // 存储里只有一个 owner、一个 attempt，revision 只推进一次。
    let stored = store.fetch_command("command-1").await.expect("fetch");
    assert_eq!(stored.state(), AutomationDispatchState::Claimed);
    assert_eq!(stored.revision, 2);
    assert_eq!(stored.intent.attempt, 1);
    assert!(matches!(
        stored.intent.worker_id.as_deref(),
        Some("worker-a") | Some("worker-b")
    ));
}
