use kiana_domain::{
    AttemptId, RunId, SwarmCancelState, SwarmCancellationFact, SWARM_CANCEL_SCHEMA,
};
use serde_json::json;

fn fact(state: SwarmCancelState) -> SwarmCancellationFact {
    let mut value = SwarmCancellationFact {
        schema: SWARM_CANCEL_SCHEMA.to_owned(),
        parent_run_id: RunId::new(),
        child_run_id: RunId::new(),
        attempt_id: AttemptId::new(),
        cancel_generation: 2,
        authority_epoch: 4,
        state,
        started_effect: true,
        stop_requested: true,
        stop_confirmed: true,
        late_result: false,
        reason: "parent_cancelled".to_owned(),
        fact_digest: String::new(),
    };
    value.fact_digest = kiana_domain::json_digest(&serde_json::json!({
        "schema": value.schema,
        "parent_run_id": value.parent_run_id,
        "child_run_id": value.child_run_id,
        "attempt_id": value.attempt_id,
        "cancel_generation": value.cancel_generation,
        "authority_epoch": value.authority_epoch,
        "state": value.state,
        "started_effect": value.started_effect,
        "stop_requested": value.stop_requested,
        "stop_confirmed": value.stop_confirmed,
        "late_result": value.late_result,
        "reason": value.reason,
    }));
    value
}

#[test]
fn cancelled_and_result_unknown_are_distinct_visible_states() {
    assert!(fact(SwarmCancelState::Cancelled).validate().is_ok());
    let mut unknown = fact(SwarmCancelState::ResultUnknown);
    unknown.stop_confirmed = false;
    unknown.fact_digest = kiana_domain::json_digest(&serde_json::json!({
        "schema": unknown.schema,
        "parent_run_id": unknown.parent_run_id,
        "child_run_id": unknown.child_run_id,
        "attempt_id": unknown.attempt_id,
        "cancel_generation": unknown.cancel_generation,
        "authority_epoch": unknown.authority_epoch,
        "state": unknown.state,
        "started_effect": unknown.started_effect,
        "stop_requested": unknown.stop_requested,
        "stop_confirmed": unknown.stop_confirmed,
        "late_result": unknown.late_result,
        "reason": unknown.reason,
    }));
    assert!(unknown.validate().is_ok());
}

#[test]
fn late_result_requires_fence_and_cancel_generation() {
    let mut late = fact(SwarmCancelState::Cancelled);
    late.late_result = true;
    late.fact_digest = kiana_domain::json_digest(&json!({
        "schema": late.schema,
        "parent_run_id": late.parent_run_id,
        "child_run_id": late.child_run_id,
        "attempt_id": late.attempt_id,
        "cancel_generation": late.cancel_generation,
        "authority_epoch": late.authority_epoch,
        "state": late.state,
        "started_effect": late.started_effect,
        "stop_requested": late.stop_requested,
        "stop_confirmed": late.stop_confirmed,
        "late_result": late.late_result,
        "reason": late.reason,
    }));
    assert_eq!(late.validate(), Err("swarm_cancellation_fact_invalid"));

    let mut fenced = fact(SwarmCancelState::Fenced);
    fenced.late_result = true;
    fenced.fact_digest = kiana_domain::json_digest(&json!({
        "schema": fenced.schema,
        "parent_run_id": fenced.parent_run_id,
        "child_run_id": fenced.child_run_id,
        "attempt_id": fenced.attempt_id,
        "cancel_generation": fenced.cancel_generation,
        "authority_epoch": fenced.authority_epoch,
        "state": fenced.state,
        "started_effect": fenced.started_effect,
        "stop_requested": fenced.stop_requested,
        "stop_confirmed": fenced.stop_confirmed,
        "late_result": fenced.late_result,
        "reason": fenced.reason,
    }));
    assert!(fenced.validate().is_ok());
}

#[test]
fn unknown_fields_and_unconfirmed_cancel_fail_closed() {
    let mut value = serde_json::to_value(fact(SwarmCancelState::Cancelled)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<SwarmCancellationFact>(value).is_err());

    let mut invalid = fact(SwarmCancelState::Cancelled);
    invalid.stop_confirmed = false;
    invalid.fact_digest = kiana_domain::json_digest(&serde_json::json!({
        "schema": invalid.schema,
        "parent_run_id": invalid.parent_run_id,
        "child_run_id": invalid.child_run_id,
        "attempt_id": invalid.attempt_id,
        "cancel_generation": invalid.cancel_generation,
        "authority_epoch": invalid.authority_epoch,
        "state": invalid.state,
        "started_effect": invalid.started_effect,
        "stop_requested": invalid.stop_requested,
        "stop_confirmed": invalid.stop_confirmed,
        "late_result": invalid.late_result,
        "reason": invalid.reason,
    }));
    assert_eq!(invalid.validate(), Err("swarm_cancellation_fact_invalid"));
}
