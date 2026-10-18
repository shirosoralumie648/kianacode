use kiana_domain::{
    AutomationEffectPermit, AutomationEffectReservation, AutomationExecutionBinding,
    AutomationPermitState, AutomationReservationLedger, AutomationReservationState,
    AUTOMATION_RESERVATION_SCHEMA,
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

#[test]
fn reservation_replays_idempotently_and_fences_revision_drift() {
    let mut ledger = ledger();
    let first = ledger.reserve(reservation()).unwrap();
    let replay = ledger.reserve(reservation()).unwrap();
    assert_eq!(first.reservation_id, replay.reservation_id);
    assert_eq!(first, replay);

    assert_eq!(
        ledger.reserve(drift("reservation-drift", "idem-drift", |value| {
            value.config_revision = "config-2".to_owned();
        })),
        Err("automation_effect_reservation_fence_drift")
    );
}

#[test]
fn unknown_fields_and_payload_conflict_fail_closed() {
    let mut value = serde_json::to_value(reservation()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationEffectReservation>(value).is_err());

    let mut ledger = ledger();
    ledger.reserve(reservation()).unwrap();
    let conflict = drift("reservation-conflict", "idem-1", |value| {
        value.action_digest = B.to_owned();
    });
    assert_eq!(
        ledger.reserve(conflict),
        Err("automation_effect_reservation_idempotency_conflict")
    );
}

#[test]
fn unregistered_executions_and_scope_drift_are_refused() {
    let mut unregistered =
        AutomationReservationLedger::new(4, "config-1".to_owned(), "policy-1".to_owned());
    assert_eq!(
        unregistered.reserve(reservation()),
        Err("automation_effect_reservation_execution_unknown")
    );

    let mut ledger = ledger();
    assert_eq!(
        ledger.register_execution(AutomationExecutionBinding::new(
            "execution-1",
            A,
            "shell",
            B,
            A,
            Some(A.to_owned()),
        )),
        Err("automation_effect_execution_binding_conflict")
    );
    assert_eq!(ledger.register_execution(binding()), Ok(()));
    ledger.reserve(reservation()).unwrap();

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
            "approval-none",
            drift("reservation-approval-none", "idem-approval-none", |value| {
                value.approval_digest = None;
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
            ledger.reserve(candidate),
            Err("automation_effect_reservation_scope_drift"),
            "scope drift case {name} must be refused"
        );
    }

    let conflicting_id = drift("reservation-1", "idem-other", |_| {});
    assert_eq!(
        ledger.reserve(conflicting_id),
        Err("automation_effect_reservation_conflict")
    );
    assert_eq!(
        ledger.fetch_reservation("reservation-budget"),
        Err("automation_effect_reservation_unknown")
    );
}

#[test]
fn permits_are_single_use_and_one_reservation_yields_one_attempt() {
    let mut ledger = ledger();
    let stored = ledger.reserve(reservation()).unwrap();
    assert_eq!(stored.state, AutomationReservationState::Reserved);

    assert_eq!(
        ledger.issue_permit("reservation-unknown", "permit-x", 7),
        Err("automation_effect_permit_reservation_unknown")
    );
    assert_eq!(
        ledger.issue_permit("reservation-1", "permit-1", 0),
        Err("automation_effect_permit_invalid")
    );

    let permit = ledger.issue_permit("reservation-1", "permit-1", 7).unwrap();
    assert_eq!(permit.state, AutomationPermitState::Issued);
    assert_eq!(permit.reservation_digest, stored.reservation_digest);
    assert_eq!(
        ledger.issue_permit("reservation-1", "permit-1", 7),
        Ok(permit.clone())
    );
    assert_eq!(
        ledger.issue_permit("reservation-1", "permit-2", 7),
        Err("automation_effect_permit_conflict")
    );

    assert_eq!(
        ledger.begin_attempt("permit-unknown", "attempt-1", 4),
        Err("automation_effect_permit_unknown")
    );
    assert_eq!(
        ledger.begin_attempt("permit-1", "attempt-1", 3),
        Err("automation_effect_permit_fence_drift")
    );

    let begin = ledger.begin_attempt("permit-1", "attempt-1", 4).unwrap();
    assert!(begin.started());
    let attempt = begin.attempt().clone();
    assert_eq!(attempt.attempt, 1);
    assert_eq!(attempt.reservation_id, "reservation-1");
    assert_eq!(attempt.reservation_digest, stored.reservation_digest);
    assert_eq!(attempt.execution_id, "execution-1");
    assert_eq!(attempt.action_digest, A);
    assert_eq!(attempt.capability, "shell");
    assert_eq!(attempt.permit_id, "permit-1");
    assert_eq!(attempt.permit_digest, permit.permit_digest);
    assert_eq!(attempt.authority_epoch, 4);

    let replay = ledger.begin_attempt("permit-1", "attempt-1", 4).unwrap();
    assert!(!replay.started());
    assert_eq!(replay.attempt(), &attempt);
    assert_eq!(
        ledger.begin_attempt("permit-1", "attempt-2", 4),
        Err("automation_effect_attempt_conflict")
    );

    let consumed = ledger.fetch_permit("permit-1").unwrap();
    assert_eq!(consumed.state, AutomationPermitState::Consumed);
    assert_ne!(consumed.permit_digest, permit.permit_digest);
    assert_eq!(
        ledger.issue_permit("reservation-1", "permit-1", 7),
        Err("automation_effect_permit_already_consumed")
    );
    assert_eq!(
        ledger.issue_permit("reservation-1", "permit-3", 7),
        Err("automation_effect_permit_conflict")
    );

    assert_eq!(ledger.fetch_reservation("reservation-1").unwrap(), stored);
    assert_eq!(
        ledger
            .fetch_reservation_by_idempotency_key("idem-1")
            .unwrap(),
        stored
    );
    assert_eq!(ledger.fetch_attempt("reservation-1").unwrap(), attempt);
    assert_eq!(
        ledger.fetch_attempt("reservation-2"),
        Err("automation_effect_attempt_unknown")
    );
    assert_eq!(
        ledger.fetch_reservation("reservation-2"),
        Err("automation_effect_reservation_unknown")
    );
}

#[test]
fn binding_permit_and_attempt_payloads_fail_closed_on_drift() {
    let binding = binding();
    let mut value = serde_json::to_value(&binding).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationExecutionBinding>(value).is_err());

    let mut tampered = binding.clone();
    tampered.budget_digest = B.to_owned();
    assert_eq!(
        tampered.validate(),
        Err("automation_effect_execution_binding_invalid")
    );

    let mut ledger = ledger();
    ledger.reserve(reservation()).unwrap();
    let permit = ledger.issue_permit("reservation-1", "permit-1", 7).unwrap();

    let round_trip: AutomationEffectPermit =
        serde_json::from_value(serde_json::to_value(&permit).unwrap()).unwrap();
    assert_eq!(round_trip, permit);
    assert_eq!(round_trip.validate(), Ok(()));

    let mut forged = serde_json::to_value(&permit).unwrap();
    forged["state"] = json!("consumed");
    let decoded: AutomationEffectPermit = serde_json::from_value(forged).unwrap();
    assert_eq!(decoded.validate(), Err("automation_effect_permit_invalid"));

    let begin = ledger.begin_attempt("permit-1", "attempt-1", 4).unwrap();
    let attempt = begin.attempt();
    let mut forged = serde_json::to_value(attempt).unwrap();
    forged["unexpected"] = json!(true);
    assert!(serde_json::from_value::<kiana_domain::AutomationEffectAttempt>(forged).is_err());
    let decoded: kiana_domain::AutomationEffectAttempt =
        serde_json::from_value(serde_json::to_value(attempt).unwrap()).unwrap();
    assert_eq!(&decoded, attempt);
    assert_eq!(decoded.validate(), Ok(()));
}
