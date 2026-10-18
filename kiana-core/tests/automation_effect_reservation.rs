//! AUT-14 先拒绝优先夹具：未 reservation、未知 execution、旧 epoch、scope drift 与重复
//! attempt 都必须以 **0 次 broker/effect 调用**失败；成功路径必须恰好一次，并且提交后可以
//! 安全重取而不重新执行。
//!
//! 夹具只驱动 Domain ledger 与 Core 门面；`broker_calls` 计数器代表“调用方真正触达
//! Broker/handler 的 effect 闭包”。本文件不连接 EventLog、模型或网络。

use kiana_core::{
    dispatch_reserved_automation_effect, issue_automation_effect_permit, reserve_automation_effect,
};
use kiana_domain::{
    AutomationEffectReservation, AutomationExecutionBinding, AutomationPermitState,
    AutomationReservationLedger, AutomationReservationState, AUTOMATION_RESERVATION_SCHEMA,
};
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

fn drift(
    reservation_id: &str,
    idempotency_key: &str,
    mutate: impl FnOnce(&mut AutomationEffectReservation),
) -> AutomationEffectReservation {
    let mut value = reservation();
    value.reservation_id = reservation_id.to_owned();
    value.idempotency_key = idempotency_key.to_owned();
    mutate(&mut value);
    recompute(&mut value);
    value
}

fn binding() -> AutomationExecutionBinding {
    AutomationExecutionBinding::new("execution-1", A, "shell", A, A, Some(A.to_owned()))
        .expect("valid execution binding")
}

fn ledger() -> AutomationReservationLedger {
    let mut ledger =
        AutomationReservationLedger::new(4, "config-1".to_owned(), "policy-1".to_owned());
    ledger.register_execution(binding()).expect("registered");
    ledger
}

/// 只有在 permit/attempt 检查全部通过时才会把 `broker_calls` 加一。
fn dispatch(
    ledger: &mut AutomationReservationLedger,
    permit_id: &str,
    attempt_id: &str,
    expected_epoch: u64,
    broker_calls: &mut usize,
) -> Result<(), &'static str> {
    dispatch_reserved_automation_effect(ledger, permit_id, attempt_id, expected_epoch, |_attempt| {
        *broker_calls += 1;
    })
}

#[test]
fn reservation_and_permit_are_required_before_any_broker_call() {
    let mut ledger = ledger();
    let mut calls = 0usize;

    assert_eq!(
        dispatch(&mut ledger, "permit-1", "attempt-1", 4, &mut calls),
        Err("automation_effect_permit_unknown")
    );
    assert_eq!(
        dispatch(&mut ledger, "permit-2", "attempt-2", 4, &mut calls),
        Err("automation_effect_permit_unknown")
    );
    assert_eq!(
        issue_automation_effect_permit(&mut ledger, "reservation-1", "permit-1", 7),
        Err("automation_effect_permit_reservation_unknown")
    );
    assert_eq!(calls, 0, "denied dispatch must not reach the broker");
}

#[test]
fn unknown_execution_ids_fail_closed_with_zero_broker_calls() {
    let mut ledger =
        AutomationReservationLedger::new(4, "config-1".to_owned(), "policy-1".to_owned());
    let mut calls = 0usize;

    assert_eq!(
        reserve_automation_effect(&mut ledger, reservation()),
        Err("automation_effect_reservation_execution_unknown")
    );
    assert_eq!(
        issue_automation_effect_permit(&mut ledger, "reservation-1", "permit-1", 7),
        Err("automation_effect_permit_reservation_unknown")
    );
    assert_eq!(
        dispatch(&mut ledger, "permit-1", "attempt-1", 4, &mut calls),
        Err("automation_effect_permit_unknown")
    );
    assert_eq!(calls, 0, "unknown execution must not reach the broker");
}

#[test]
fn stale_epoch_and_scope_digest_drift_never_reach_the_broker() {
    let mut calls = 0usize;

    let mut stale =
        AutomationReservationLedger::new(5, "config-1".to_owned(), "policy-1".to_owned());
    stale.register_execution(binding()).expect("registered");
    assert_eq!(
        reserve_automation_effect(&mut stale, reservation()),
        Err("automation_effect_reservation_fence_drift")
    );
    assert_eq!(calls, 0, "stale epoch must not reach the broker");

    let mut ledger = ledger();
    assert_eq!(
        reserve_automation_effect(&mut ledger, reservation()),
        Ok(reservation())
    );
    let cases = vec![
        (
            "budget",
            drift("reservation-budget", "idem-budget", |value| {
                value.budget_digest = B.to_owned();
            }),
        ),
        (
            "path",
            drift("reservation-path", "idem-path", |value| {
                value.path_scope_digest = B.to_owned();
            }),
        ),
        (
            "approval",
            drift("reservation-approval", "idem-approval", |value| {
                value.approval_digest = Some(B.to_owned());
            }),
        ),
        (
            "action",
            drift("reservation-action", "idem-action", |value| {
                value.action_digest = B.to_owned();
            }),
        ),
        (
            "capability",
            drift("reservation-capability", "idem-capability", |value| {
                value.capability = "apply_patch".to_owned();
            }),
        ),
    ];
    for (name, candidate) in cases {
        assert_eq!(
            reserve_automation_effect(&mut ledger, candidate),
            Err("automation_effect_reservation_scope_drift"),
            "scope drift case {name} must be refused"
        );
    }
    assert_eq!(
        issue_automation_effect_permit(&mut ledger, "reservation-budget", "permit-budget", 7),
        Err("automation_effect_permit_reservation_unknown")
    );
    assert_eq!(calls, 0, "scope drift must not reach the broker");
}

#[test]
fn one_reservation_yields_one_attempt_and_replay_never_reexecutes() {
    let mut ledger = ledger();
    let mut calls = 0usize;

    let stored = reserve_automation_effect(&mut ledger, reservation()).expect("first reserve");
    let replayed = reserve_automation_effect(&mut ledger, reservation()).expect("safe re-fetch");
    assert_eq!(stored.reservation_digest, replayed.reservation_digest);
    assert_eq!(calls, 0, "reservation replay must not reach the broker");

    let permit =
        issue_automation_effect_permit(&mut ledger, "reservation-1", "permit-1", 7).expect("issue");
    assert_eq!(permit.state, AutomationPermitState::Issued);
    assert_eq!(
        issue_automation_effect_permit(&mut ledger, "reservation-1", "permit-1", 7),
        Ok(permit.clone())
    );

    assert_eq!(
        dispatch(&mut ledger, "permit-1", "attempt-1", 4, &mut calls),
        Ok(())
    );
    assert_eq!(
        calls, 1,
        "the first dispatch reaches the broker exactly once"
    );

    assert_eq!(
        dispatch(&mut ledger, "permit-1", "attempt-1", 4, &mut calls),
        Err("automation_effect_attempt_already_started")
    );
    assert_eq!(
        dispatch(&mut ledger, "permit-1", "attempt-2", 4, &mut calls),
        Err("automation_effect_attempt_conflict")
    );
    assert_eq!(
        dispatch(&mut ledger, "permit-1", "attempt-1", 3, &mut calls),
        Err("automation_effect_permit_fence_drift")
    );
    assert_eq!(
        issue_automation_effect_permit(&mut ledger, "reservation-1", "permit-2", 8),
        Err("automation_effect_permit_conflict")
    );
    assert_eq!(calls, 1, "replays and conflicts must not reach the broker");

    let attempt = ledger
        .fetch_attempt("reservation-1")
        .expect("safe re-fetch");
    assert_eq!(attempt.reservation_digest, stored.reservation_digest);
    assert_eq!(attempt.attempt, 1);
    let consumed = ledger.fetch_permit("permit-1").expect("safe re-fetch");
    assert_eq!(consumed.state, AutomationPermitState::Consumed);
}
