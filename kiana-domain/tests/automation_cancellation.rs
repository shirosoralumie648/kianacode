use kiana_domain::{AutomationCancelState, AutomationCancellationFact, AUTOMATION_CANCEL_SCHEMA};
use serde_json::json;

fn fact(state: AutomationCancelState) -> AutomationCancellationFact {
    let mut value = AutomationCancellationFact {
        schema: AUTOMATION_CANCEL_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        attempt: 1,
        cancel_generation: 2,
        authority_epoch: 4,
        state,
        effect_started: true,
        stop_requested: true,
        stop_confirmed: true,
        late_result: false,
        reason: "cancelled".to_owned(),
        fact_digest: String::new(),
    };
    value.fact_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "execution_id": value.execution_id,
        "attempt": value.attempt,
        "cancel_generation": value.cancel_generation,
        "authority_epoch": value.authority_epoch,
        "state": value.state,
        "effect_started": value.effect_started,
        "stop_requested": value.stop_requested,
        "stop_confirmed": value.stop_confirmed,
        "late_result": value.late_result,
        "reason": value.reason,
    }));
    value
}

#[test]
fn stopped_and_unknown_are_visible_and_late_requires_fence() {
    assert!(fact(AutomationCancelState::Stopped).validate().is_ok());
    let mut unknown = fact(AutomationCancelState::ResultUnknown);
    unknown.stop_confirmed = false;
    unknown.fact_digest = kiana_domain::json_digest(&json!({
        "schema": unknown.schema,
        "execution_id": unknown.execution_id,
        "attempt": unknown.attempt,
        "cancel_generation": unknown.cancel_generation,
        "authority_epoch": unknown.authority_epoch,
        "state": unknown.state,
        "effect_started": unknown.effect_started,
        "stop_requested": unknown.stop_requested,
        "stop_confirmed": unknown.stop_confirmed,
        "late_result": unknown.late_result,
        "reason": unknown.reason,
    }));
    assert!(unknown.validate().is_ok());
    let mut late = fact(AutomationCancelState::Stopped);
    late.late_result = true;
    late.fact_digest = kiana_domain::json_digest(&json!({
        "schema": late.schema,
        "execution_id": late.execution_id,
        "attempt": late.attempt,
        "cancel_generation": late.cancel_generation,
        "authority_epoch": late.authority_epoch,
        "state": late.state,
        "effect_started": late.effect_started,
        "stop_requested": late.stop_requested,
        "stop_confirmed": late.stop_confirmed,
        "late_result": late.late_result,
        "reason": late.reason,
    }));
    assert_eq!(late.validate(), Err("automation_cancellation_fact_invalid"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(fact(AutomationCancelState::Requested)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationCancellationFact>(value).is_err());
}
