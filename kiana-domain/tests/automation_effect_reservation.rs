use kiana_domain::{
    AutomationEffectReservation, AutomationReservationLedger, AutomationReservationState,
    AUTOMATION_RESERVATION_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn reservation() -> AutomationEffectReservation {
    let mut value = AutomationEffectReservation {
        schema: AUTOMATION_RESERVATION_SCHEMA.to_owned(),
        reservation_id: "reservation-1".to_owned(),
        idempotency_key: "idem-1".to_owned(),
        execution_id: "execution-1".to_owned(),
        action_digest: D.to_owned(),
        capability: "shell".to_owned(),
        authority_epoch: 4,
        config_revision: "config-1".to_owned(),
        policy_revision: "policy-1".to_owned(),
        budget_digest: D.to_owned(),
        path_scope_digest: D.to_owned(),
        approval_digest: Some(D.to_owned()),
        state: AutomationReservationState::Reserved,
        reservation_digest: String::new(),
    };
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
    value
}

#[test]
fn reservation_replays_idempotently_and_fences_revision_drift() {
    let mut ledger =
        AutomationReservationLedger::new(4, "config-1".to_owned(), "policy-1".to_owned());
    let first = ledger.reserve(reservation()).unwrap();
    let replay = ledger.reserve(reservation()).unwrap();
    assert_eq!(first.reservation_id, replay.reservation_id);

    let mut drift = reservation();
    drift.config_revision = "config-2".to_owned();
    drift.reservation_digest = kiana_domain::json_digest(&json!({
        "schema": drift.schema,
        "reservation_id": drift.reservation_id,
        "idempotency_key": drift.idempotency_key,
        "execution_id": drift.execution_id,
        "action_digest": drift.action_digest,
        "capability": drift.capability,
        "authority_epoch": drift.authority_epoch,
        "config_revision": drift.config_revision,
        "policy_revision": drift.policy_revision,
        "budget_digest": drift.budget_digest,
        "path_scope_digest": drift.path_scope_digest,
        "approval_digest": drift.approval_digest,
        "state": drift.state,
    }));
    assert_eq!(
        ledger.reserve(drift),
        Err("automation_effect_reservation_fence_drift")
    );
}

#[test]
fn unknown_fields_and_payload_conflict_fail_closed() {
    let mut value = serde_json::to_value(reservation()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationEffectReservation>(value).is_err());

    let mut ledger =
        AutomationReservationLedger::new(4, "config-1".to_owned(), "policy-1".to_owned());
    ledger.reserve(reservation()).unwrap();
    let mut conflict = reservation();
    conflict.action_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    conflict.reservation_digest = kiana_domain::json_digest(&json!({
        "schema": conflict.schema,
        "reservation_id": conflict.reservation_id,
        "idempotency_key": conflict.idempotency_key,
        "execution_id": conflict.execution_id,
        "action_digest": conflict.action_digest,
        "capability": conflict.capability,
        "authority_epoch": conflict.authority_epoch,
        "config_revision": conflict.config_revision,
        "policy_revision": conflict.policy_revision,
        "budget_digest": conflict.budget_digest,
        "path_scope_digest": conflict.path_scope_digest,
        "approval_digest": conflict.approval_digest,
        "state": conflict.state,
    }));
    assert_eq!(
        ledger.reserve(conflict),
        Err("automation_effect_reservation_idempotency_conflict")
    );
}
