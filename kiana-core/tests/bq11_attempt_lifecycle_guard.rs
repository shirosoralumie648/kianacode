use kiana_core::{
    project_model_attempt_lifecycle, validate_model_attempt_dispatch,
    ModelAttemptLifecycleProjectionError,
};
use kiana_domain::{
    AttemptId, ClockObservation, ModelAttemptIdentity, ModelAttemptLifecycleLedger, QuotaGroupKey,
    QuotaReservation, QuotaReservationId, QuotaWindow, RequestId, RuntimeEvent,
};

fn ledger() -> (ModelAttemptLifecycleLedger, RequestId) {
    let run_id = kiana_domain::RunId::new();
    let attempt_id = AttemptId::new();
    let identity = ModelAttemptIdentity::new(
        run_id,
        kiana_domain::TurnId::new(),
        kiana_domain::StepId::new(),
        kiana_domain::ModelAttemptId::new(),
        RequestId::new(),
        1,
    )
    .unwrap();
    let clock = ClockObservation::observe("bq11-core", 10_000, 10_000, None, 1).unwrap();
    let reservation = QuotaReservation::new(
        QuotaReservationId::new(),
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap(),
        QuotaWindow::from_clock(&clock, 60_000).unwrap(),
        run_id,
        attempt_id,
        1,
        "config:v1",
        1,
        100,
        1,
        "bq11-core:fixture",
        70_000,
    )
    .unwrap();
    (
        ModelAttemptLifecycleLedger::new(identity, attempt_id, &reservation).unwrap(),
        RequestId::new(),
    )
}

fn committed_events(ledger: &ModelAttemptLifecycleLedger) -> Vec<RuntimeEvent> {
    ledger
        .events()
        .iter()
        .map(|fact| {
            fact.into_runtime_event(RequestId::new(), fact.revision)
                .unwrap()
        })
        .collect()
}

#[test]
fn projection_rebuilds_only_prepared_flushed_lifecycle() {
    let (mut ledger, permit) = ledger();
    ledger.prepare(Some(permit)).unwrap();
    ledger.mark_prepared_flushed(9).unwrap();
    ledger.dispatch(Some(permit)).unwrap();
    let events = committed_events(&ledger);
    let projection = project_model_attempt_lifecycle(&events).unwrap();
    assert_eq!(projection.attempts.len(), 1);
    let record = &projection.attempts[0];
    assert_eq!(record.state, kiana_domain::ModelAttemptState::Dispatching);
    assert_eq!(record.source_event_ids.len(), events.len());
    validate_model_attempt_dispatch(record, Some(permit), true).unwrap_err();
}

#[test]
fn projection_rejects_dispatch_without_prepared_fact() {
    let (mut ledger, permit) = ledger();
    ledger.prepare(Some(permit)).unwrap();
    ledger.mark_prepared_flushed(1).unwrap();
    let dispatch = ledger.dispatch(Some(permit)).unwrap();
    let event = RuntimeEvent::new(
        RequestId::new(),
        dispatch.revision,
        dispatch.kind.as_str(),
        serde_json::to_value(dispatch).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        project_model_attempt_lifecycle(&[event]),
        Err(ModelAttemptLifecycleProjectionError::Transition(_))
    ));
}

#[test]
fn source_guard_keeps_lifecycle_projection_read_only() {
    let domain = include_str!("../../kiana-domain/src/model_attempt_lifecycle.rs");
    let core = include_str!("../src/model_attempt_lifecycle.rs");
    for marker in [
        "ModelAttemptState",
        "model_attempt_prepared_not_flushed",
        "model_attempt_permit_required",
        "model_attempt_duplicate_settlement",
        "model_attempt_unknown_error_required",
        "NormalizedUsage",
        "QuotaReservation",
        "ModelAttemptLifecycleEvent",
        "apply_model_attempt_event",
        "project_model_attempt_lifecycle",
        "validate_model_attempt_dispatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "BQ-11 marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest::",
        "std::process::Command",
        "tokio::spawn",
        "CapabilityBroker",
        "ProviderClient",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "BQ-11 lifecycle boundary gained effect authority: {forbidden}"
        );
    }
}
