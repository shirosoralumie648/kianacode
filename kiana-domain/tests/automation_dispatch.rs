use kiana_domain::*;
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn intent() -> AutomationDispatchIntent {
    let mut value = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: D.to_owned(),
        worker_id: Some("worker-1".to_owned()),
        attempt: 1,
        authority_epoch: 4,
        state: AutomationDispatchState::Claimed,
        observation_digest: None,
        intent_digest: String::new(),
    };
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
    value
}

fn observation(outcome: AutomationObservationOutcome) -> AutomationObservation {
    AutomationObservation {
        execution_id: "execution-1".to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: D.to_owned(),
        worker_id: "worker-1".to_owned(),
        authority_epoch: 4,
        outcome,
        result_digest: D.to_owned(),
    }
}

#[test]
fn observation_is_separate_and_unknown_stays_unknown() {
    let success = validate_automation_observation(
        &intent(),
        &observation(AutomationObservationOutcome::Succeeded),
    )
    .unwrap();
    assert_eq!(success.state, AutomationDispatchState::Observed);
    let unknown = validate_automation_observation(
        &intent(),
        &observation(AutomationObservationOutcome::Unknown),
    )
    .unwrap();
    assert_eq!(unknown.state, AutomationDispatchState::Unknown);
}

#[test]
fn worker_binding_and_prepared_claim_fences_fail_closed() {
    let mut wrong = observation(AutomationObservationOutcome::Succeeded);
    wrong.worker_id = "worker-2".to_owned();
    assert_eq!(
        validate_automation_observation(&intent(), &wrong),
        Err("automation_observation_binding_drift")
    );
    let mut prepared = intent();
    prepared.state = AutomationDispatchState::Prepared;
    prepared.worker_id = None;
    prepared.intent_digest = kiana_domain::json_digest(&json!({
        "schema": prepared.schema,
        "execution_id": prepared.execution_id,
        "reservation_id": prepared.reservation_id,
        "action_digest": prepared.action_digest,
        "worker_id": prepared.worker_id,
        "attempt": prepared.attempt,
        "authority_epoch": prepared.authority_epoch,
        "state": prepared.state,
        "observation_digest": prepared.observation_digest,
    }));
    assert_eq!(
        validate_automation_observation(
            &prepared,
            &observation(AutomationObservationOutcome::Succeeded)
        ),
        Err("automation_observation_binding_drift")
    );
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(intent()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationDispatchIntent>(value).is_err());
}
