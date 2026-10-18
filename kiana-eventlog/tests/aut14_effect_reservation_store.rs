//! AUT-14 先拒绝优先夹具：CAS 语义 reservation 存储的幂等重取、revision 围栏、
//! attempt 唯一性与绑定漂移。
//!
//! 夹具驱动真实的 `MemoryAutomationEffectReservationStore` 与 Domain ledger；它只证明
//! 进程内 CAS 语义（CI 语义），不声称 fsync、崩溃恢复或跨进程 fencing。

use kiana_domain::{
    AutomationEffectAttempt, AutomationEffectReservation, AutomationExecutionBinding,
    AutomationReservationLedger, AutomationReservationState, AUTOMATION_RESERVATION_SCHEMA,
};
use kiana_eventlog::MemoryAutomationEffectReservationStore;
use kiana_ports::PortError;
use serde_json::json;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn recompute(value: &mut AutomationEffectReservation) {
    value.reservation_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "reservation_id": value.reservation_id,
        "idempotency_key": value.idempotency_key,
        "execution_id": value.execution_id,
        "action_digest": value.action_digest,
        "capability": value.capability,
        "authority_epoch": value.authority_epoch,
        "config_revision": value.config_revision,
        "policy_revision": value.policy_revision,
        "budget_digest": value.budget_digest,
        "path_scope_digest": value.path_scope_digest,
        "approval_digest": value.approval_digest,
        "state": value.state,
    }));
}

fn recompute_attempt(value: &mut AutomationEffectAttempt) {
    value.attempt_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "attempt_id": value.attempt_id,
        "reservation_id": value.reservation_id,
        "reservation_digest": value.reservation_digest,
        "execution_id": value.execution_id,
        "action_digest": value.action_digest,
        "capability": value.capability,
        "permit_id": value.permit_id,
        "permit_digest": value.permit_digest,
        "authority_epoch": value.authority_epoch,
        "attempt": value.attempt,
    }));
}

fn reservation() -> AutomationEffectReservation {
    let mut value = AutomationEffectReservation {
        schema: AUTOMATION_RESERVATION_SCHEMA.to_owned(),
        reservation_id: "reservation-1".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        execution_id: "execution-1".to_owned(),
        action_digest: A.to_owned(),
        capability: "shell".to_owned(),
        authority_epoch: 4,
        config_revision: "config-1".to_owned(),
        policy_revision: "policy-1".to_owned(),
        budget_digest: A.to_owned(),
        path_scope_digest: A.to_owned(),
        approval_digest: Some(A.to_owned()),
        state: AutomationReservationState::Reserved,
        reservation_digest: String::new(),
    };
    recompute(&mut value);
    value
}

fn named_reservation(reservation_id: &str, idempotency_key: &str) -> AutomationEffectReservation {
    let mut value = reservation();
    value.reservation_id = reservation_id.to_owned();
    value.idempotency_key = idempotency_key.to_owned();
    recompute(&mut value);
    value
}

fn ledger() -> AutomationReservationLedger {
    let mut ledger =
        AutomationReservationLedger::new(4, "config-1".to_owned(), "policy-1".to_owned());
    ledger
        .register_execution(
            AutomationExecutionBinding::new("execution-1", A, "shell", A, A, Some(A.to_owned()))
                .expect("valid execution binding"),
        )
        .expect("registered");
    ledger
}

fn started_attempt(
    ledger: &mut AutomationReservationLedger,
    reservation: &AutomationEffectReservation,
    permit_id: &str,
    attempt_id: &str,
) -> AutomationEffectAttempt {
    ledger.reserve(reservation.clone()).expect("reserved");
    ledger
        .issue_permit(&reservation.reservation_id, permit_id, 7)
        .expect("permit issued");
    let begin = ledger
        .begin_attempt(permit_id, attempt_id, 4)
        .expect("attempt started");
    assert!(begin.started());
    begin.attempt().clone()
}

fn conflict_reason(error: PortError) -> String {
    match error {
        PortError::Conflict(reason) => reason,
        other => panic!("expected PortError::Conflict, got {other:?}"),
    }
}

fn failure_reason(error: PortError) -> String {
    match error {
        PortError::Failed(reason) => reason,
        other => panic!("expected PortError::Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn reservation_commit_replays_safely_and_is_cas_fenced() {
    let store = MemoryAutomationEffectReservationStore::new();
    let first = reservation();

    let committed = store.commit_reservation(0, first.clone()).await.unwrap();
    assert_eq!(committed.stored.revision, 1);
    assert!(!committed.replayed);

    // 提交结果不确定时按幂等键重取：即使 expected_revision 已过期也返回原事实。
    let replay = store.commit_reservation(0, first.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.stored, committed.stored);
    assert_eq!(store.revision("execution-1").await, 1);

    // 同幂等键换摘要必须拒绝。
    let mut conflict = first.clone();
    conflict.action_digest = B.to_owned();
    recompute(&mut conflict);
    assert_eq!(
        conflict_reason(store.commit_reservation(1, conflict).await.unwrap_err()),
        "automation_effect_reservation_idempotency_conflict"
    );

    // 同一 execution 上的新 reservation 必须 CAS 通过。
    let second = named_reservation("reservation-2", "idem-2");
    assert_eq!(
        conflict_reason(
            store
                .commit_reservation(0, second.clone())
                .await
                .unwrap_err()
        ),
        "automation_effect_reservation_revision_conflict"
    );
    let stored_second = store.commit_reservation(1, second).await.unwrap();
    assert_eq!(stored_second.stored.revision, 2);
    assert!(!stored_second.replayed);

    // 同 reservation_id 换幂等键视为冲突，而不是静默覆盖。
    let third = named_reservation("reservation-2", "idem-3");
    assert_eq!(
        conflict_reason(store.commit_reservation(2, third).await.unwrap_err()),
        "automation_effect_reservation_conflict"
    );

    // 校验失败的载荷在加锁前就被拒绝。
    let mut invalid = first.clone();
    invalid.reservation_digest = B.to_owned();
    assert_eq!(
        failure_reason(store.commit_reservation(2, invalid).await.unwrap_err()),
        "automation_effect_reservation_invalid"
    );

    // 安全重取：按 ID 与按幂等键都返回同一份已提交事实。
    assert_eq!(
        store.fetch_reservation("reservation-1").await.unwrap(),
        committed.stored
    );
    assert_eq!(
        store
            .fetch_reservation_by_idempotency_key("idem-1")
            .await
            .unwrap(),
        committed.stored
    );
    assert_eq!(
        conflict_reason(
            store
                .fetch_reservation("reservation-unknown")
                .await
                .unwrap_err()
        ),
        "automation_effect_reservation_unknown"
    );
    assert_eq!(
        conflict_reason(
            store
                .fetch_reservation_by_idempotency_key("idem-unknown")
                .await
                .unwrap_err()
        ),
        "automation_effect_reservation_unknown"
    );
}

#[tokio::test]
async fn attempts_are_cas_committed_once_and_binding_drift_is_refused() {
    let store = MemoryAutomationEffectReservationStore::new();
    let mut ledger = ledger();
    let first = named_reservation("reservation-1", "idem-1");
    let second = named_reservation("reservation-2", "idem-2");

    let first_commit = store.commit_reservation(0, first.clone()).await.unwrap();
    assert_eq!(first_commit.stored.revision, 1);
    let second_commit = store.commit_reservation(1, second.clone()).await.unwrap();
    assert_eq!(second_commit.stored.revision, 2);

    let attempt = started_attempt(&mut ledger, &first, "permit-1", "attempt-1");
    let committed = store.commit_attempt(2, attempt.clone()).await.unwrap();
    assert_eq!(committed.stored.revision, 3);
    assert!(!committed.replayed);

    // 同一 attempt 重放返回既有记录，不新建事实。
    let replay = store.commit_attempt(2, attempt.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.stored, committed.stored);

    // 新 attempt 必须按 execution 流 revision CAS。
    let attempt2 = started_attempt(&mut ledger, &second, "permit-2", "attempt-2");
    assert_eq!(
        conflict_reason(store.commit_attempt(2, attempt2.clone()).await.unwrap_err()),
        "automation_effect_reservation_revision_conflict"
    );
    let committed2 = store.commit_attempt(3, attempt2).await.unwrap();
    assert_eq!(committed2.stored.revision, 4);
    assert_eq!(store.revision("execution-1").await, 4);

    // 未提交 reservation 的 attempt 拒绝。
    let third = named_reservation("reservation-3", "idem-3");
    let attempt3 = started_attempt(&mut ledger, &third, "permit-3", "attempt-3");
    assert_eq!(
        conflict_reason(store.commit_attempt(4, attempt3).await.unwrap_err()),
        "automation_effect_attempt_reservation_unknown"
    );

    // 绑定漂移：attempt 声称的 execution 与已提交 reservation 不一致。
    let mut drifted = attempt.clone();
    drifted.execution_id = "execution-2".to_owned();
    recompute_attempt(&mut drifted);
    assert_eq!(
        conflict_reason(store.commit_attempt(0, drifted).await.unwrap_err()),
        "automation_effect_attempt_binding_drift"
    );

    // 一 reservation 一 attempt：换 attempt_id 的重签载荷仍拒绝。
    let mut forged = attempt.clone();
    forged.attempt_id = "attempt-other".to_owned();
    recompute_attempt(&mut forged);
    assert_eq!(
        conflict_reason(store.commit_attempt(4, forged).await.unwrap_err()),
        "automation_effect_attempt_conflict"
    );

    // 无效载荷在加锁前以 Failed 拒绝。
    let mut invalid = attempt.clone();
    invalid.attempt = 0;
    assert_eq!(
        failure_reason(store.commit_attempt(4, invalid).await.unwrap_err()),
        "automation_effect_attempt_invalid"
    );

    // 安全重取 attempt。
    assert_eq!(
        store.fetch_attempt("reservation-1").await.unwrap(),
        committed.stored
    );
    assert_eq!(
        conflict_reason(
            store
                .fetch_attempt("reservation-unknown")
                .await
                .unwrap_err()
        ),
        "automation_effect_attempt_unknown"
    );
}

#[tokio::test]
async fn concurrent_commits_leave_exactly_one_winning_reservation() {
    let store = MemoryAutomationEffectReservationStore::new();
    let first = named_reservation("reservation-1", "idem-1");
    let second = named_reservation("reservation-2", "idem-2");
    let (a, b) = tokio::join!(
        store.commit_reservation(0, first),
        store.commit_reservation(0, second)
    );
    let outcomes = [a, b];
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        conflict_reason(outcomes.into_iter().find_map(Result::err).unwrap()),
        "automation_effect_reservation_revision_conflict"
    );
    assert_eq!(store.revision("execution-1").await, 1);

    // 同一幂等键并发提交：恰好一次新建、一次重放，最终只有一份事实。
    let store = MemoryAutomationEffectReservationStore::new();
    let (a, b) = tokio::join!(
        store.commit_reservation(0, reservation()),
        store.commit_reservation(0, reservation())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.replayed, b.replayed);
    assert_eq!(a.stored, b.stored);
    assert_eq!(store.revision("execution-1").await, 1);
}
