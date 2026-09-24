use kiana_domain::{
    AttemptId, ClockObservation, ModelAttemptError, ModelAttemptIdentity,
    ModelAttemptLifecycleLedger, ModelAttemptState, NormalizedUsage, QuotaGroupKey,
    QuotaReservation, QuotaReservationId, QuotaWindow, ReceiptId, RequestId, RunId,
    UsageConfidence, UsageId, UsageObservation, UsageSource, UsageVector,
};

fn fixture() -> (
    ModelAttemptLifecycleLedger,
    AttemptId,
    RequestId,
    NormalizedUsage,
) {
    let run_id = RunId::new();
    let turn_id = kiana_domain::TurnId::new();
    let attempt_id = AttemptId::new();
    let identity = ModelAttemptIdentity::new(
        run_id,
        turn_id,
        kiana_domain::StepId::new(),
        kiana_domain::ModelAttemptId::new(),
        RequestId::new(),
        1,
    )
    .unwrap();
    let clock = ClockObservation::observe("bq11", 10_000, 10_000, None, 1).unwrap();
    let window = QuotaWindow::from_clock(&clock, 60_000).unwrap();
    let reservation = QuotaReservation::new(
        QuotaReservationId::new(),
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap(),
        window,
        run_id,
        attempt_id,
        1,
        "config:v1",
        1,
        100,
        1,
        "bq11:fixture",
        70_000,
    )
    .unwrap();
    let usage = NormalizedUsage::new(
        UsageId::new(),
        attempt_id,
        run_id,
        Some("provider".to_owned()),
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Provider,
        UsageObservation::Snapshot,
        Some(1),
        UsageConfidence::Known,
        None,
        "bq11_fixture",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();
    (
        ModelAttemptLifecycleLedger::new(identity, attempt_id, &reservation).unwrap(),
        attempt_id,
        RequestId::new(),
        usage,
    )
}

#[test]
fn lifecycle_links_run_turn_attempt_usage_and_receipt() {
    let (mut ledger, _, permit, usage) = fixture();
    assert_eq!(
        ledger.prepare(Some(permit)).unwrap().kind.as_str(),
        "model.prepared"
    );
    assert_eq!(
        ledger.dispatch(Some(permit)).unwrap_err(),
        "model_attempt_prepared_not_flushed"
    );
    ledger.mark_prepared_flushed(7).unwrap();
    assert_eq!(
        ledger.dispatch(Some(permit)).unwrap().kind.as_str(),
        "model.dispatching"
    );
    ledger.observe(usage.clone()).unwrap();
    ledger.settle(ReceiptId::new(), Some(usage)).unwrap();
    let record = ledger.record().unwrap();
    assert_eq!(record.state, ModelAttemptState::Settled);
    assert!(record.receipt_id.is_some());
    assert_eq!(record.source_event_ids.len(), 4);
    record.validate().unwrap();
}

#[test]
fn deny_unprepared_missing_permit_and_duplicate_settlement() {
    let (mut unprepared, _, permit, _) = fixture();
    assert_eq!(
        unprepared.dispatch(Some(permit)).unwrap_err(),
        "model_attempt_not_prepared"
    );

    let (mut missing_permit, _, _, _) = fixture();
    missing_permit.prepare(Option::<RequestId>::None).unwrap();
    missing_permit.mark_prepared_flushed(1).unwrap();
    assert_eq!(
        missing_permit
            .dispatch(Option::<RequestId>::None)
            .unwrap_err(),
        "model_attempt_permit_required"
    );

    let (mut duplicate, _, permit, usage) = fixture();
    duplicate.prepare(Some(permit)).unwrap();
    duplicate.mark_prepared_flushed(1).unwrap();
    duplicate.dispatch(Some(permit)).unwrap();
    duplicate.observe(usage.clone()).unwrap();
    duplicate
        .settle(ReceiptId::new(), Some(usage.clone()))
        .unwrap();
    assert_eq!(
        duplicate.settle(ReceiptId::new(), Some(usage)).unwrap_err(),
        "model_attempt_duplicate_settlement"
    );
}

#[test]
fn unknown_attempt_retains_usage_and_error_for_reconciliation() {
    let (mut ledger, _, permit, usage) = fixture();
    ledger.prepare(Some(permit)).unwrap();
    ledger.mark_prepared_flushed(1).unwrap();
    ledger.dispatch(Some(permit)).unwrap();
    let error = ModelAttemptError::new("provider_eof", "response", true).unwrap();
    ledger.unknown(Some(usage), error).unwrap();
    let record = ledger.record().unwrap();
    assert_eq!(record.state, ModelAttemptState::Unknown);
    assert!(record.usage.is_some());
    assert_eq!(
        record.error.as_ref().map(|error| error.code.as_str()),
        Some("provider_eof")
    );
    assert!(record.receipt_id.is_none());
}
