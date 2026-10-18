//! AUT-17 先拒绝优先夹具：取消事实 store 的代次单调、摘要 CAS、同代次转移白名单与安全重放。
//!
//! 夹具只驱动进程内 `MemoryAutomationCancellationStore`；它不发出 stop、不执行 effect，也不代表
//! durable 语义。

use kiana_domain::{AutomationCancelState, AutomationCancellationFact, AUTOMATION_CANCEL_SCHEMA};
use kiana_eventlog::MemoryAutomationCancellationStore;
use kiana_ports::{AutomationCancellationStore, PortError};
use serde_json::json;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fact(state: AutomationCancelState, generation: u64, epoch: u64) -> AutomationCancellationFact {
    let mut value = AutomationCancellationFact {
        schema: AUTOMATION_CANCEL_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        attempt: 1,
        cancel_generation: generation,
        authority_epoch: epoch,
        state,
        effect_started: true,
        stop_requested: true,
        stop_confirmed: true,
        stop_evidence_digest: Some(A.to_owned()),
        late_result: false,
        reason: "cancelled".to_owned(),
        fact_digest: String::new(),
    };
    match state {
        AutomationCancelState::Requested => {
            value.stop_requested = false;
            value.stop_confirmed = false;
            value.stop_evidence_digest = None;
        }
        AutomationCancelState::Stopping => value.stop_confirmed = false,
        AutomationCancelState::NotExecuted => {
            value.effect_started = false;
            value.stop_confirmed = false;
        }
        AutomationCancelState::Stopped => {}
        AutomationCancelState::ResultUnknown => value.stop_confirmed = false,
        AutomationCancelState::Fenced => value.late_result = true,
    }
    value.recompute_digest();
    value
}

fn conflict(reason: &str) -> String {
    PortError::Conflict(reason.to_owned()).to_string()
}

fn failed(reason: &str) -> String {
    PortError::Failed(reason.to_owned()).to_string()
}

#[tokio::test]
async fn first_commit_requires_an_absent_fact_and_stale_digests_are_refused() {
    let store = MemoryAutomationCancellationStore::new();
    let stopped = fact(AutomationCancelState::Stopped, 3, 4);
    let commit = store
        .commit_cancellation(stopped.clone(), None)
        .await
        .expect("first commit");
    assert!(!commit.replayed);
    assert_eq!(commit.fact.fact_digest, stopped.fact_digest);

    // 已存在事实时，摘要 CAS 不匹配（包括“假装还没有事实”）都必须拒绝。
    for expected in [None, Some(B)] {
        assert_eq!(
            store
                .commit_cancellation(fact(AutomationCancelState::Stopping, 3, 4), expected)
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_cancellation_digest_conflict")
        );
    }
    let stored = store
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .expect("recorded");
    assert_eq!(stored.fact_digest, stopped.fact_digest);
}

#[tokio::test]
async fn identical_replays_return_the_stored_copy_without_rewriting() {
    let store = MemoryAutomationCancellationStore::new();
    let requested = fact(AutomationCancelState::Requested, 3, 4);
    store
        .commit_cancellation(requested.clone(), None)
        .await
        .expect("first commit");
    let replayed = store
        .commit_cancellation(requested.clone(), Some(&requested.fact_digest))
        .await
        .expect("replay");
    assert!(replayed.replayed);
    assert_eq!(replayed.fact.fact_digest, requested.fact_digest);

    // 同代次推进（在途 → 已请求停止）是允许的，并更新事实。
    let stopping = fact(AutomationCancelState::Stopping, 3, 4);
    let advanced = store
        .commit_cancellation(stopping, Some(&requested.fact_digest))
        .await
        .expect("advance");
    assert!(!advanced.replayed);
    assert_eq!(advanced.fact.state, AutomationCancelState::Stopping);
}

#[tokio::test]
async fn stale_generations_cannot_advance_a_recorded_cancel() {
    let store = MemoryAutomationCancellationStore::new();
    let stopped = fact(AutomationCancelState::Stopped, 3, 4);
    store
        .commit_cancellation(stopped.clone(), None)
        .await
        .expect("first commit");

    // 更低代次：陈旧取消，不推进事实。
    assert_eq!(
        store
            .commit_cancellation(
                fact(AutomationCancelState::Requested, 2, 4),
                Some(&stopped.fact_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_generation_stale")
    );
    // 更高代次：新的取消代次允许写入。
    let fresh = store
        .commit_cancellation(
            fact(AutomationCancelState::Requested, 4, 4),
            Some(&stopped.fact_digest),
        )
        .await
        .expect("new generation");
    assert_eq!(fresh.fact.cancel_generation, 4);
    assert_eq!(fresh.fact.state, AutomationCancelState::Requested);
    assert_eq!(
        store
            .fetch_cancellation("execution-1", 1)
            .await
            .expect("fetch")
            .expect("recorded")
            .cancel_generation,
        4
    );
}

#[tokio::test]
async fn closed_states_cannot_be_reopened_within_a_generation() {
    let store = MemoryAutomationCancellationStore::new();
    let stopped = fact(AutomationCancelState::Stopped, 3, 4);
    store
        .commit_cancellation(stopped.clone(), None)
        .await
        .expect("first commit");

    // 已确认停止不能回到在途/未执行/未知。
    for reopened in [
        AutomationCancelState::Requested,
        AutomationCancelState::Stopping,
        AutomationCancelState::NotExecuted,
        AutomationCancelState::ResultUnknown,
    ] {
        assert_eq!(
            store
                .commit_cancellation(fact(reopened, 3, 4), Some(&stopped.fact_digest))
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_cancel_transition_refused"),
            "closed state reopened as {reopened:?}"
        );
    }
    // 迟到结果只允许落到 Fenced。
    let fenced = store
        .commit_cancellation(
            fact(AutomationCancelState::Fenced, 3, 4),
            Some(&stopped.fact_digest),
        )
        .await
        .expect("fence");
    assert_eq!(fenced.fact.state, AutomationCancelState::Fenced);
    // Fenced 是终态：同代次不能回到任何活着的状态。
    for target in [
        AutomationCancelState::Requested,
        AutomationCancelState::Stopping,
        AutomationCancelState::NotExecuted,
        AutomationCancelState::Stopped,
        AutomationCancelState::ResultUnknown,
    ] {
        assert_eq!(
            store
                .commit_cancellation(fact(target, 3, 4), Some(&fenced.fact.fact_digest))
                .await
                .unwrap_err()
                .to_string(),
            conflict("automation_cancel_transition_refused"),
            "fenced fact reopened as {target:?}"
        );
    }
    // 新的取消代次仍然可以写入（旧代次的隔离不影响新代次）。
    let next_generation = store
        .commit_cancellation(
            fact(AutomationCancelState::Requested, 4, 4),
            Some(&fenced.fact.fact_digest),
        )
        .await
        .expect("next generation");
    assert_eq!(next_generation.fact.cancel_generation, 4);
}

#[tokio::test]
async fn epoch_drift_within_a_generation_is_refused() {
    let store = MemoryAutomationCancellationStore::new();
    let stopping = fact(AutomationCancelState::Stopping, 3, 4);
    store
        .commit_cancellation(stopping.clone(), None)
        .await
        .expect("first commit");
    assert_eq!(
        store
            .commit_cancellation(
                fact(AutomationCancelState::Stopped, 3, 5),
                Some(&stopping.fact_digest)
            )
            .await
            .unwrap_err()
            .to_string(),
        conflict("automation_cancel_authority_epoch_conflict")
    );
    let stored = store
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .expect("recorded");
    assert_eq!(stored.state, AutomationCancelState::Stopping);
    assert_eq!(stored.authority_epoch, 4);
}

#[tokio::test]
async fn invalid_facts_never_reach_the_store() {
    let store = MemoryAutomationCancellationStore::new();
    let mut invalid = fact(AutomationCancelState::Stopped, 3, 4);
    invalid.stop_confirmed = false;
    invalid.recompute_digest();
    assert_eq!(
        store
            .commit_cancellation(invalid, None)
            .await
            .unwrap_err()
            .to_string(),
        failed("automation_cancellation_fact_invalid")
    );
    assert!(store
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .is_none());

    // 未知字段在反序列化层就被拒绝。
    let mut value = serde_json::to_value(fact(AutomationCancelState::Requested, 3, 4)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationCancellationFact>(value).is_err());
}

#[tokio::test]
async fn fetch_is_a_pure_read_and_missing_keys_return_none() {
    let store = MemoryAutomationCancellationStore::new();
    assert!(store
        .fetch_cancellation("execution-1", 1)
        .await
        .expect("fetch")
        .is_none());
    assert!(store
        .fetch_cancellation("execution-1", 2)
        .await
        .expect("fetch")
        .is_none());
    store
        .commit_cancellation(fact(AutomationCancelState::NotExecuted, 1, 4), None)
        .await
        .expect("commit");
    // 同一 execution 的另一个 attempt 是独立的取消代次流。
    assert!(store
        .fetch_cancellation("execution-1", 2)
        .await
        .expect("fetch")
        .is_none());
    assert_eq!(
        store
            .fetch_cancellation("execution-1", 1)
            .await
            .expect("fetch")
            .expect("recorded")
            .state,
        AutomationCancelState::NotExecuted
    );
}
