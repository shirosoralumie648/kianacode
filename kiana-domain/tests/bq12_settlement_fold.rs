use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

struct Fixture {
    run_id: RunId,
    attempt_id: AttemptId,
    reservation: QuotaReservation,
    identity: ModelAttemptIdentity,
    permit: RequestId,
}

fn fixture() -> Fixture {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let clock = ClockObservation::observe("bq12-domain", 10_000, 10_000, None, 1).unwrap();
    let reservation = QuotaReservation::new(
        QuotaReservationId::new(),
        QuotaGroupKey::new("provider", "credential", Some("model".to_owned()), None).unwrap(),
        QuotaWindow::from_clock(&clock, 60_000).unwrap(),
        run_id,
        attempt_id,
        1,
        "config:bq12",
        1,
        100,
        1,
        "bq12:fixture",
        70_000,
    )
    .unwrap();
    Fixture {
        run_id,
        attempt_id,
        reservation,
        identity: ModelAttemptIdentity::new(
            run_id,
            TurnId::new(),
            StepId::new(),
            ModelAttemptId::new(),
            RequestId::new(),
            1,
        )
        .unwrap(),
        permit: RequestId::new(),
    }
}

fn usage(fixture: &Fixture, confidence: UsageConfidence) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        fixture.attempt_id,
        fixture.run_id,
        Some("provider".to_owned()),
        "model",
        "route",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(7),
            output_tokens: if confidence == UsageConfidence::Known {
                Some(5)
            } else {
                None
            },
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        (confidence != UsageConfidence::Known).then_some(BillingUnknownReason::Partial),
        "bq12 fixture",
        digest('a'),
    )
    .unwrap()
}

fn lifecycle_pair(
    fixture: &Fixture,
    confidence: UsageConfidence,
) -> (SettlementFoldEvent, ModelAttemptLifecycleRecord) {
    let mut lifecycle = ModelAttemptLifecycleLedger::new(
        fixture.identity.clone(),
        fixture.attempt_id,
        &fixture.reservation,
    )
    .unwrap();
    lifecycle.prepare(Some(fixture.permit)).unwrap();
    let prepared_record = lifecycle.record().unwrap();
    let reserved =
        SettlementFoldRecord::from_lifecycle(&fixture.reservation, &prepared_record, source())
            .unwrap();
    lifecycle.mark_prepared_flushed(1).unwrap();
    lifecycle.dispatch(Some(fixture.permit)).unwrap();
    let observed = usage(fixture, confidence);
    lifecycle.observe(observed.clone()).unwrap();
    lifecycle.settle(ReceiptId::new(), Some(observed)).unwrap();
    (reserved, lifecycle.record().unwrap())
}

fn source() -> SettlementSourceRef {
    SettlementSourceRef::new(EventId::new(), digest('b')).unwrap()
}

#[test]
fn known_usage_consumes_then_releases_only_unused_reservation() {
    let fixture = fixture();
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved).unwrap();
    let event = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    assert_eq!(event.kind, SettlementFoldEventKind::Consumed);
    assert_eq!(
        ledger.apply(event).unwrap(),
        SettlementFoldApplyOutcome::Applied
    );
    let release = ledger.release_unused(fixture.attempt_id, source()).unwrap();
    assert_eq!(release.kind, SettlementFoldEventKind::Released);
    let released = ledger.get(fixture.attempt_id).unwrap();
    assert_eq!(released.state, SettlementFoldState::Released);
    assert_eq!(released.consumed.unwrap().tokens, Some(12));
    assert_eq!(released.released.tokens, 88);
    assert!(ledger.release_unused(fixture.attempt_id, source()).is_err());
}

#[test]
fn partial_usage_stays_consumed_and_cannot_be_refunded() {
    let fixture = fixture();
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Partial);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved).unwrap();
    let event = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    assert_eq!(event.usage_class, Some(SettlementUsageClass::Partial));
    ledger.apply(event).unwrap();
    assert_eq!(
        ledger
            .release_unused(fixture.attempt_id, source())
            .unwrap_err(),
        "settlement_partial_release_requires_reconciliation"
    );
}

#[test]
fn unknown_result_retains_reservation_and_rejects_release() {
    let fixture = fixture();
    let mut lifecycle = ModelAttemptLifecycleLedger::new(
        fixture.identity.clone(),
        fixture.attempt_id,
        &fixture.reservation,
    )
    .unwrap();
    lifecycle.prepare(Some(fixture.permit)).unwrap();
    lifecycle.mark_prepared_flushed(1).unwrap();
    lifecycle.dispatch(Some(fixture.permit)).unwrap();
    lifecycle
        .unknown(
            None,
            ModelAttemptError::new("provider_eof", "response", true).unwrap(),
        )
        .unwrap();
    let record = lifecycle.record().unwrap();
    let mut ledger = SettlementFoldLedger::default();
    let mut prepared = ModelAttemptLifecycleLedger::new(
        fixture.identity.clone(),
        fixture.attempt_id,
        &fixture.reservation,
    )
    .unwrap();
    prepared.prepare(Some(fixture.permit)).unwrap();
    let reserved = SettlementFoldRecord::from_lifecycle(
        &fixture.reservation,
        &prepared.record().unwrap(),
        source(),
    )
    .unwrap();
    ledger.apply(reserved).unwrap();
    let event = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    assert_eq!(event.kind, SettlementFoldEventKind::Unknown);
    assert!(event.reconciliation_required);
    ledger.apply(event).unwrap();
    assert_eq!(
        ledger
            .release_unused(fixture.attempt_id, source())
            .unwrap_err(),
        "settlement_unknown_cannot_release"
    );
}

#[test]
fn duplicate_settlement_with_changed_digest_is_rejected() {
    let fixture = fixture();
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved).unwrap();
    let first = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    let mut conflicting = first;
    conflicting.event_id = EventId::new();
    conflicting.reserved.tokens = 99;
    conflicting.event_digest = conflicting.digest();
    assert!(ledger.apply(conflicting).is_err());
}

#[test]
fn terminal_fact_without_reserved_fact_is_rejected() {
    let fixture = fixture();
    let (_, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let event =
        SettlementFoldRecord::from_lifecycle(&fixture.reservation, &record, source()).unwrap();
    assert_eq!(
        SettlementFoldLedger::default().apply(event).unwrap_err(),
        "settlement_requires_reserved_fact"
    );
}
