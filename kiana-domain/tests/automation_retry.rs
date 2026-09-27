use kiana_domain::{
    classify_automation_retry, AutomationRetryDecision, AutomationRetryInput,
    AUTOMATION_RETRY_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input() -> AutomationRetryInput {
    let mut value = AutomationRetryInput {
        schema: AUTOMATION_RETRY_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        attempt: 1,
        max_attempts: 3,
        effect_started: false,
        effect_known: true,
        capability_idempotent: true,
        approval_valid: true,
        budget_remaining: 2,
        transient_pre_send: true,
        retry_after_ms: Some(10),
        deadline_remaining_ms: 100,
        input_digest: D.to_owned(),
        decision_digest: String::new(),
    };
    value.decision_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "execution_id": value.execution_id,
        "attempt": value.attempt,
        "max_attempts": value.max_attempts,
        "effect_started": value.effect_started,
        "effect_known": value.effect_known,
        "capability_idempotent": value.capability_idempotent,
        "approval_valid": value.approval_valid,
        "budget_remaining": value.budget_remaining,
        "transient_pre_send": value.transient_pre_send,
        "retry_after_ms": value.retry_after_ms,
        "deadline_remaining_ms": value.deadline_remaining_ms,
        "input_digest": value.input_digest,
    }));
    value
}

#[test]
fn bounded_pre_send_retry_is_allowed() {
    assert_eq!(
        classify_automation_retry(&input()).unwrap(),
        AutomationRetryDecision::RetryNewAttempt
    );
}

#[test]
fn unknown_effect_and_non_idempotent_or_expired_inputs_do_not_retry() {
    let mut unknown = input();
    unknown.effect_started = true;
    unknown.effect_known = false;
    unknown.decision_digest = kiana_domain::json_digest(&json!({
        "schema": unknown.schema,
        "execution_id": unknown.execution_id,
        "attempt": unknown.attempt,
        "max_attempts": unknown.max_attempts,
        "effect_started": unknown.effect_started,
        "effect_known": unknown.effect_known,
        "capability_idempotent": unknown.capability_idempotent,
        "approval_valid": unknown.approval_valid,
        "budget_remaining": unknown.budget_remaining,
        "transient_pre_send": unknown.transient_pre_send,
        "retry_after_ms": unknown.retry_after_ms,
        "deadline_remaining_ms": unknown.deadline_remaining_ms,
        "input_digest": unknown.input_digest,
    }));
    assert_eq!(
        classify_automation_retry(&unknown).unwrap(),
        AutomationRetryDecision::ReconcileUnknown
    );

    let mut no = input();
    no.capability_idempotent = false;
    no.decision_digest = kiana_domain::json_digest(&json!({
        "schema": no.schema,
        "execution_id": no.execution_id,
        "attempt": no.attempt,
        "max_attempts": no.max_attempts,
        "effect_started": no.effect_started,
        "effect_known": no.effect_known,
        "capability_idempotent": no.capability_idempotent,
        "approval_valid": no.approval_valid,
        "budget_remaining": no.budget_remaining,
        "transient_pre_send": no.transient_pre_send,
        "retry_after_ms": no.retry_after_ms,
        "deadline_remaining_ms": no.deadline_remaining_ms,
        "input_digest": no.input_digest,
    }));
    assert_eq!(
        classify_automation_retry(&no).unwrap(),
        AutomationRetryDecision::NoRetry
    );
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(input()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationRetryInput>(value).is_err());
}
