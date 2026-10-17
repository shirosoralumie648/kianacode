//! BQ-12 settlement/release/unknown fold fixtures (deny first, success after).
//!
//! The fixtures drive the pure `SettlementFoldLedger` the way an adapter would: a committed BQ-11
//! lifecycle record is folded into reservation facts.  Every refusal below (automatic zeroing of a
//! cancelled/timed-out/EOF attempt, releasing an unknown attempt, refunding an already consumed
//! portion, double settlement/release and identity/digest drift) is asserted before the matching
//! happy path.  The final fixtures prove that the known, partial and unknown paths produce
//! deterministic ledgers whose occupancy stays distinguishable.

use kiana_domain::*;
use uuid::Uuid;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

struct Fixture {
    run_id: RunId,
    attempt_id: AttemptId,
    reservation: QuotaReservation,
    identity: ModelAttemptIdentity,
    permit: RequestId,
    /// `Some(seed)` produces deterministic identities so two independently built folds are
    /// comparable; `None` keeps the original random-identity fixtures.
    seed: Option<u128>,
}

fn uuid_for(seed: Option<u128>, offset: u128) -> Uuid {
    match seed {
        Some(base) => Uuid::from_u128(base.saturating_mul(1_000).saturating_add(offset)),
        None => Uuid::new_v4(),
    }
}

fn fixture_with(seed: Option<u128>) -> Fixture {
    let run_id = RunId::from_uuid(uuid_for(seed, 1));
    let attempt_id = AttemptId::from_uuid(uuid_for(seed, 2));
    let clock = ClockObservation::observe("bq12-domain", 10_000, 10_000, None, 1).unwrap();
    let reservation = QuotaReservation::new(
        QuotaReservationId::from_uuid(uuid_for(seed, 8)),
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
            TurnId::from_uuid(uuid_for(seed, 4)),
            StepId::from_uuid(uuid_for(seed, 5)),
            ModelAttemptId::from_uuid(uuid_for(seed, 3)),
            RequestId::from_uuid(uuid_for(seed, 6)),
            1,
        )
        .unwrap(),
        permit: RequestId::from_uuid(uuid_for(seed, 7)),
        seed,
    }
}

fn fixture() -> Fixture {
    fixture_with(None)
}

fn pinned_fixture(seed: u128) -> Fixture {
    fixture_with(Some(seed))
}

fn usage(fixture: &Fixture, confidence: UsageConfidence) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::from_uuid(uuid_for(fixture.seed, 9)),
        fixture.attempt_id,
        fixture.run_id,
        Some("provider".to_owned()),
        "model",
        "route",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(7),
            output_tokens: (confidence == UsageConfidence::Known).then_some(5),
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

fn source() -> SettlementSourceRef {
    SettlementSourceRef::new(EventId::new(), digest('b')).unwrap()
}

/// The committed reservation fact for this attempt, plus the ledger that produced it so a caller
/// can continue the same attempt into observe/dispatch/unknown.
fn reserved_pair(fixture: &Fixture) -> (SettlementFoldEvent, ModelAttemptLifecycleLedger) {
    let mut lifecycle = ModelAttemptLifecycleLedger::new(
        fixture.identity.clone(),
        fixture.attempt_id,
        &fixture.reservation,
    )
    .unwrap();
    lifecycle.prepare(Some(fixture.permit)).unwrap();
    let reserved = SettlementFoldRecord::from_lifecycle(
        &fixture.reservation,
        &lifecycle.record().unwrap(),
        source(),
    )
    .unwrap();
    (reserved, lifecycle)
}

/// A settled BQ-11 record for this attempt; the lifecycle itself refuses a drifted usage before
/// the fold ever sees it.
fn settled_record(fixture: &Fixture, confidence: UsageConfidence) -> ModelAttemptLifecycleRecord {
    let mut lifecycle = ModelAttemptLifecycleLedger::new(
        fixture.identity.clone(),
        fixture.attempt_id,
        &fixture.reservation,
    )
    .unwrap();
    lifecycle.prepare(Some(fixture.permit)).unwrap();
    lifecycle.mark_prepared_flushed(1).unwrap();
    lifecycle.dispatch(Some(fixture.permit)).unwrap();
    let usage = usage(fixture, confidence);
    lifecycle.observe(usage.clone()).unwrap();
    lifecycle.settle(ReceiptId::new(), Some(usage)).unwrap();
    lifecycle.record().unwrap()
}

fn lifecycle_pair(
    fixture: &Fixture,
    confidence: UsageConfidence,
) -> (SettlementFoldEvent, ModelAttemptLifecycleRecord) {
    let (reserved, _) = reserved_pair(fixture);
    (reserved, settled_record(fixture, confidence))
}

fn replay(facts: &[SettlementFoldEvent]) -> SettlementFoldLedger {
    let mut ledger = SettlementFoldLedger::default();
    for fact in facts {
        ledger.apply(fact.clone()).unwrap();
    }
    ledger
}

/// The event sequence one attempt produces on the way to its terminal fold.  `retain_usage` keeps
/// the observed portion on the unknown path; `with_release` adds the explicit unused release that
/// only a complete known settlement can produce.
fn fold_path_facts(
    fixture: &Fixture,
    confidence: UsageConfidence,
    retain_usage: bool,
    with_release: bool,
) -> Vec<SettlementFoldEvent> {
    let mut builder = SettlementFoldLedger::default();
    let (reserved, mut lifecycle) = reserved_pair(fixture);
    assert_eq!(
        builder.apply(reserved.clone()).unwrap(),
        SettlementFoldApplyOutcome::Applied
    );
    let mut facts = vec![reserved];
    lifecycle.mark_prepared_flushed(1).unwrap();
    lifecycle.dispatch(Some(fixture.permit)).unwrap();
    if confidence == UsageConfidence::Unknown {
        let observed = retain_usage.then(|| usage(fixture, UsageConfidence::Partial));
        lifecycle
            .unknown(
                observed,
                ModelAttemptError::new("provider_eof", "response", true).unwrap(),
            )
            .unwrap();
    } else {
        let observed_usage = usage(fixture, confidence);
        lifecycle.observe(observed_usage.clone()).unwrap();
        facts.push(
            builder
                .observe_from_lifecycle(
                    &fixture.reservation,
                    &lifecycle.record().unwrap(),
                    source(),
                )
                .unwrap(),
        );
        lifecycle
            .settle(ReceiptId::new(), Some(observed_usage))
            .unwrap();
    }
    facts.push(
        builder
            .settle_from_lifecycle(&fixture.reservation, &lifecycle.record().unwrap(), source())
            .unwrap(),
    );
    if with_release {
        facts.push(
            builder
                .release_unused(fixture.attempt_id, source())
                .unwrap(),
        );
    }
    facts
}

/// A fresh event identity, fresh source reference and explicit revision: exactly what a second,
/// conflicting fact looks like on the wire.
fn restamped(mut event: SettlementFoldEvent, revision: u64) -> SettlementFoldEvent {
    event.event_id = EventId::new();
    event.source_refs = vec![SettlementSourceRef::new(EventId::new(), digest('f')).unwrap()];
    event.revision = revision;
    event.event_digest = event.digest();
    event
}

#[test]
fn known_usage_consumes_then_releases_only_unused_reservation() {
    let fixture = fixture();
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved.clone()).unwrap();
    let event = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    assert_eq!(event.kind, SettlementFoldEventKind::Consumed);
    assert_eq!(
        ledger.get(fixture.attempt_id).unwrap().state,
        SettlementFoldState::Consumed
    );

    // The command already applies its event. A fresh projection first consumes it once,
    // then treats the exact same event as an idempotent replay without charging again.
    let mut replay = SettlementFoldLedger::default();
    replay.apply(reserved).unwrap();
    assert_eq!(
        replay.apply(event.clone()).unwrap(),
        SettlementFoldApplyOutcome::Applied
    );
    let consumed = replay.get(fixture.attempt_id).unwrap().clone();
    assert_eq!(
        replay.apply(event).unwrap(),
        SettlementFoldApplyOutcome::Duplicate
    );
    assert_eq!(replay.get(fixture.attempt_id), Some(&consumed));
    let release = ledger.release_unused(fixture.attempt_id, source()).unwrap();
    assert_eq!(release.kind, SettlementFoldEventKind::Released);
    assert_eq!(
        replay.apply(release.clone()).unwrap(),
        SettlementFoldApplyOutcome::Applied
    );
    let released = ledger.get(fixture.attempt_id).unwrap();
    assert_eq!(released.state, SettlementFoldState::Released);
    assert_eq!(released.consumed.unwrap().tokens, Some(12));
    assert_eq!(released.released.tokens, 88);
    assert_eq!(replay.get(fixture.attempt_id), Some(released));
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

#[test]
fn cancel_timeout_and_eof_never_become_an_automatic_zero_or_release() {
    for (index, code) in ["provider_cancelled", "provider_timeout", "provider_eof"]
        .into_iter()
        .enumerate()
    {
        let fixture = pinned_fixture(101 + index as u128);
        let (reserved, mut lifecycle) = reserved_pair(&fixture);
        lifecycle.mark_prepared_flushed(1).unwrap();
        lifecycle.dispatch(Some(fixture.permit)).unwrap();
        lifecycle
            .unknown(
                None,
                ModelAttemptError::new(code, "response", true).unwrap(),
            )
            .unwrap();
        let terminal = lifecycle.record().unwrap();

        let mut ledger = SettlementFoldLedger::default();
        ledger.apply(reserved).unwrap();
        let event = ledger
            .settle_from_lifecycle(&fixture.reservation, &terminal, source())
            .unwrap();

        // A cancelled, timed-out or EOF attempt is `unknown`, not a zero-valued settlement: no
        // consumption is invented and the whole reservation stays held for reconciliation.
        assert_eq!(event.kind, SettlementFoldEventKind::Unknown, "{code}");
        assert_eq!(
            event.usage_class,
            Some(SettlementUsageClass::Unknown),
            "{code}"
        );
        assert_eq!(event.consumed, None, "{code}");
        assert_eq!(event.released, ReservationUnits::default(), "{code}");
        assert_eq!(
            event.reserved,
            ReservationUnits {
                requests: 1,
                tokens: 100,
                concurrency: 1,
            },
            "{code}"
        );
        assert_eq!(
            event.unknown_reason,
            Some(BillingUnknownReason::ResultUnknown),
            "{code}"
        );
        assert!(event.reconciliation_required, "{code}");
        assert!(event.receipt_id.is_none(), "{code}");

        let record = ledger.get(fixture.attempt_id).unwrap();
        assert_eq!(record.state, SettlementFoldState::Unknown, "{code}");
        assert_eq!(record.released, ReservationUnits::default(), "{code}");
        assert!(record.reconciliation_required, "{code}");

        // No automatic release, no late settlement, and no silent second unknown.
        assert_eq!(
            ledger
                .release_unused(fixture.attempt_id, source())
                .unwrap_err(),
            "settlement_unknown_cannot_release",
            "{code}"
        );
        let mut late_settlement = event.clone();
        late_settlement.kind = SettlementFoldEventKind::Consumed;
        late_settlement.receipt_id = Some(ReceiptId::new());
        late_settlement.usage_class = Some(SettlementUsageClass::Known);
        late_settlement.usage_digest = Some(digest('b'));
        late_settlement.reconciliation_required = false;
        late_settlement.consumed = Some(ConsumedUnits {
            requests: 1,
            tokens: Some(12),
            concurrency: 1,
        });
        let late_settlement = restamped(late_settlement, event.revision + 1);
        assert_eq!(
            ledger.apply(late_settlement).unwrap_err(),
            "settlement_unknown_requires_reconciliation",
            "{code}"
        );
        assert_eq!(
            ledger
                .settle_from_lifecycle(
                    &fixture.reservation,
                    &settled_record(&fixture, UsageConfidence::Known),
                    source(),
                )
                .unwrap_err(),
            "settlement_attempt_not_reserved",
            "{code}"
        );
        assert_eq!(
            ledger.get(fixture.attempt_id).unwrap().state,
            SettlementFoldState::Unknown,
            "{code}"
        );
    }
}

#[test]
fn unknown_keeps_the_observed_portion_but_never_refunds_it() {
    let fixture = pinned_fixture(150);
    let (reserved, mut lifecycle) = reserved_pair(&fixture);
    lifecycle.mark_prepared_flushed(1).unwrap();
    lifecycle.dispatch(Some(fixture.permit)).unwrap();
    // A partial observation that then ends in an unknown result: the known portion is retained,
    // the reservation stays occupied and nothing is refunded before reconciliation.
    lifecycle
        .unknown(
            Some(usage(&fixture, UsageConfidence::Partial)),
            ModelAttemptError::new("provider_eof", "response", true).unwrap(),
        )
        .unwrap();
    let terminal = lifecycle.record().unwrap();

    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved).unwrap();
    let event = ledger
        .settle_from_lifecycle(&fixture.reservation, &terminal, source())
        .unwrap();
    assert_eq!(event.kind, SettlementFoldEventKind::Unknown);
    assert_eq!(event.usage_class, Some(SettlementUsageClass::Unknown));
    assert_eq!(
        event.consumed,
        Some(ConsumedUnits {
            requests: 1,
            tokens: Some(7),
            concurrency: 1,
        })
    );
    assert_eq!(event.released, ReservationUnits::default());
    assert!(event.reconciliation_required);

    ledger.apply(event).unwrap();
    let record = ledger.get(fixture.attempt_id).unwrap();
    assert_eq!(record.state, SettlementFoldState::Unknown);
    assert_eq!(record.consumed.unwrap().tokens, Some(7));
    assert_eq!(record.released, ReservationUnits::default());
    assert_eq!(
        ledger
            .release_unused(fixture.attempt_id, source())
            .unwrap_err(),
        "settlement_unknown_cannot_release"
    );
}

#[test]
fn observed_or_reserved_attempts_cannot_release_without_a_known_settlement() {
    let fixture = pinned_fixture(200);
    let (reserved, mut lifecycle) = reserved_pair(&fixture);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved.clone()).unwrap();

    // Nothing is consumed yet: a release would mint a refund from an unsettled reservation.
    assert_eq!(
        ledger
            .release_unused(fixture.attempt_id, source())
            .unwrap_err(),
        "settlement_not_consumed"
    );
    let mut forged_release = reserved;
    forged_release.kind = SettlementFoldEventKind::Released;
    forged_release.usage_class = Some(SettlementUsageClass::Known);
    forged_release.usage_digest = Some(digest('b'));
    forged_release.receipt_id = Some(ReceiptId::new());
    forged_release.consumed = Some(ConsumedUnits {
        requests: 1,
        tokens: Some(12),
        concurrency: 1,
    });
    forged_release.released = ReservationUnits {
        requests: 0,
        tokens: 88,
        concurrency: 0,
    };
    let forged_release = restamped(forged_release, 2);
    assert_eq!(
        ledger.apply(forged_release).unwrap_err(),
        "settlement_state_transition_invalid"
    );

    // Observed but unsettled: still no release until a known terminal fact exists.
    lifecycle.mark_prepared_flushed(1).unwrap();
    lifecycle.dispatch(Some(fixture.permit)).unwrap();
    lifecycle
        .observe(usage(&fixture, UsageConfidence::Known))
        .unwrap();
    let observed = ledger
        .observe_from_lifecycle(&fixture.reservation, &lifecycle.record().unwrap(), source())
        .unwrap();
    assert_eq!(observed.kind, SettlementFoldEventKind::Observed);
    assert_eq!(
        ledger.get(fixture.attempt_id).unwrap().state,
        SettlementFoldState::Reserved
    );
    assert_eq!(
        ledger
            .release_unused(fixture.attempt_id, source())
            .unwrap_err(),
        "settlement_not_consumed"
    );
    assert!(ledger.get(fixture.attempt_id).unwrap().consumed.is_some());
    assert_eq!(
        ledger.get(fixture.attempt_id).unwrap().released,
        ReservationUnits::default()
    );
}

#[test]
fn known_usage_is_never_refunded_only_the_unused_remainder_is_released() {
    let fixture = pinned_fixture(300);
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved).unwrap();
    let consumed = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    assert_eq!(consumed.kind, SettlementFoldEventKind::Consumed);
    assert_eq!(consumed.consumed.as_ref().unwrap().tokens, Some(12));
    assert_eq!(consumed.released, ReservationUnits::default());

    // A release that refunds everything (including the consumed tokens) fails closed.
    let mut full_refund = consumed.clone();
    full_refund.kind = SettlementFoldEventKind::Released;
    full_refund.released = full_refund.reserved;
    let full_refund = restamped(full_refund, consumed.revision + 1);
    assert_eq!(
        ledger.apply(full_refund.clone()).unwrap_err(),
        "settlement_release_must_be_unused_reservation"
    );

    // Releasing more than was reserved is refused too.
    let mut over_release = full_refund.clone();
    over_release.released.tokens = over_release.reserved.tokens + 1;
    let over_release = restamped(over_release, consumed.revision + 1);
    assert_eq!(
        ledger.apply(over_release).unwrap_err(),
        "settlement_release_exceeds_reservation"
    );

    // Claiming more consumption than the reservation held is refused before any refund is derived.
    let mut over_consumed = consumed.clone();
    over_consumed.consumed = Some(ConsumedUnits {
        requests: 1,
        tokens: Some(120),
        concurrency: 1,
    });
    let over_consumed = restamped(over_consumed, consumed.revision + 1);
    assert_eq!(
        ledger.apply(over_consumed).unwrap_err(),
        "settlement_consumption_exceeds_reservation"
    );

    // A release without known tokens cannot compute an unused remainder.
    let mut unknown_tokens = full_refund;
    unknown_tokens.released = ReservationUnits::default();
    unknown_tokens.consumed = Some(ConsumedUnits {
        requests: 1,
        tokens: None,
        concurrency: 1,
    });
    let unknown_tokens = restamped(unknown_tokens, consumed.revision + 1);
    assert_eq!(
        ledger.apply(unknown_tokens).unwrap_err(),
        "settlement_release_requires_known_tokens"
    );

    // The refusals above left the fold exactly where the known settlement put it: same revision,
    // same retained consumption and still no refund.
    let after_refusals = ledger.get(fixture.attempt_id).unwrap();
    assert_eq!(after_refusals.state, SettlementFoldState::Consumed);
    assert_eq!(after_refusals.revision, consumed.revision);
    assert_eq!(after_refusals.consumed.as_ref().unwrap().tokens, Some(12));
    assert_eq!(after_refusals.released, ReservationUnits::default());

    // The only accepted release is exactly reserved - consumed, and it happens once.
    let release = ledger.release_unused(fixture.attempt_id, source()).unwrap();
    assert_eq!(
        release.released,
        ReservationUnits {
            requests: 0,
            tokens: 88,
            concurrency: 0,
        }
    );
    let released = ledger.get(fixture.attempt_id).unwrap();
    assert_eq!(released.state, SettlementFoldState::Released);
    assert_eq!(released.consumed.unwrap().tokens, Some(12));
    assert_eq!(
        ledger
            .release_unused(fixture.attempt_id, source())
            .unwrap_err(),
        "settlement_duplicate_release"
    );
}

#[test]
fn duplicate_settlement_and_release_cannot_charge_or_refund_twice() {
    let fixture = pinned_fixture(400);
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved).unwrap();
    let first = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    let after_first = ledger.get(fixture.attempt_id).unwrap().clone();

    // Replaying the committed fact is idempotent: the record is returned unchanged.
    assert_eq!(
        ledger.apply(first.clone()).unwrap(),
        SettlementFoldApplyOutcome::Duplicate
    );
    assert_eq!(ledger.get(fixture.attempt_id), Some(&after_first));

    // A second settlement carrying fresh event identity is resisted by the transition fence, so
    // the usage cannot be charged twice.
    let second = restamped(first.clone(), first.revision + 1);
    assert_eq!(
        ledger.apply(second.clone()).unwrap_err(),
        "settlement_duplicate_settlement"
    );
    assert_eq!(ledger.get(fixture.attempt_id), Some(&after_first));

    // After the explicit release the record is terminal for both settlement and release.
    let release = ledger.release_unused(fixture.attempt_id, source()).unwrap();
    let released = ledger.get(fixture.attempt_id).unwrap().clone();
    assert_eq!(
        ledger.apply(release.clone()).unwrap(),
        SettlementFoldApplyOutcome::Duplicate
    );
    assert_eq!(ledger.get(fixture.attempt_id), Some(&released));
    let late_settlement = restamped(second, release.revision + 1);
    assert_eq!(
        ledger.apply(late_settlement).unwrap_err(),
        "settlement_duplicate_release"
    );
    let second_release = restamped(release, released.revision + 1);
    assert_eq!(
        ledger.apply(second_release).unwrap_err(),
        "settlement_duplicate_release"
    );
    assert_eq!(ledger.get(fixture.attempt_id), Some(&released));
}

#[test]
fn identity_usage_digest_and_revision_drift_fail_closed() {
    let fixture = pinned_fixture(500);
    let (reserved, record) = lifecycle_pair(&fixture, UsageConfidence::Known);
    let mut ledger = SettlementFoldLedger::default();
    ledger.apply(reserved.clone()).unwrap();
    let settled = ledger
        .settle_from_lifecycle(&fixture.reservation, &record, source())
        .unwrap();
    let terminal_record = ledger.get(fixture.attempt_id).unwrap().clone();

    // A fact carrying another reservation digest cannot join this attempt.
    let mut foreign_reservation = reserved.clone();
    foreign_reservation.reservation_digest = digest('7');
    let foreign_reservation = restamped(foreign_reservation, settled.revision + 1);
    assert_eq!(
        ledger.apply(foreign_reservation).unwrap_err(),
        "settlement_identity_or_reservation_conflict"
    );

    // A usage digest that drifts from the observed one cannot be settled.
    let mut observed = reserved.clone();
    observed.kind = SettlementFoldEventKind::Observed;
    observed.usage_class = Some(SettlementUsageClass::Known);
    observed.usage_digest = Some(digest('b'));
    observed.consumed = Some(ConsumedUnits {
        requests: 1,
        tokens: Some(12),
        concurrency: 1,
    });
    let observed = restamped(observed, 2);
    let mut partial_ledger = SettlementFoldLedger::default();
    partial_ledger.apply(reserved.clone()).unwrap();
    assert_eq!(
        partial_ledger.apply(observed.clone()).unwrap(),
        SettlementFoldApplyOutcome::Applied
    );
    let mut drifted = observed;
    drifted.kind = SettlementFoldEventKind::Consumed;
    drifted.receipt_id = Some(ReceiptId::new());
    drifted.usage_digest = Some(digest('9'));
    let drifted = restamped(drifted, 3);
    assert_eq!(
        partial_ledger.apply(drifted).unwrap_err(),
        "settlement_usage_digest_conflict"
    );
    assert_eq!(
        partial_ledger.get(fixture.attempt_id).unwrap().state,
        SettlementFoldState::Reserved
    );

    // A revision gap is not an implicit retry or replay.
    let gap = restamped(reserved.clone(), settled.revision + 2);
    assert_eq!(
        ledger.apply(gap).unwrap_err(),
        "settlement_revision_conflict"
    );

    // Reusing a source event reference would let one committed fact drive two folds.
    let mut replayed_source = reserved.clone();
    replayed_source.source_refs = ledger.get(fixture.attempt_id).unwrap().source_refs.clone();
    let replayed_source = restamped(replayed_source, settled.revision + 1);
    assert_eq!(
        ledger.apply(replayed_source).unwrap_err(),
        "settlement_source_event_replay_conflict"
    );

    // The fold is unchanged by every refusal above.
    assert_eq!(ledger.get(fixture.attempt_id), Some(&terminal_record));
}

#[test]
fn known_partial_and_unknown_paths_produce_a_deterministic_ledger() {
    #[derive(Clone, Copy)]
    struct Case {
        label: &'static str,
        seed: u128,
        confidence: UsageConfidence,
        retain_usage: bool,
        with_release: bool,
        state: SettlementFoldState,
        usage_class: Option<SettlementUsageClass>,
        consumed_tokens: Option<Option<u64>>,
        released_tokens: u64,
        reconciliation_required: bool,
    }

    let cases = [
        Case {
            label: "known_released",
            seed: 501,
            confidence: UsageConfidence::Known,
            retain_usage: false,
            with_release: true,
            state: SettlementFoldState::Released,
            usage_class: Some(SettlementUsageClass::Known),
            consumed_tokens: Some(Some(12)),
            released_tokens: 88,
            reconciliation_required: false,
        },
        Case {
            label: "known_held",
            seed: 502,
            confidence: UsageConfidence::Known,
            retain_usage: false,
            with_release: false,
            state: SettlementFoldState::Consumed,
            usage_class: Some(SettlementUsageClass::Known),
            consumed_tokens: Some(Some(12)),
            released_tokens: 0,
            reconciliation_required: false,
        },
        Case {
            label: "partial_held",
            seed: 503,
            confidence: UsageConfidence::Partial,
            retain_usage: false,
            with_release: false,
            state: SettlementFoldState::Consumed,
            usage_class: Some(SettlementUsageClass::Partial),
            consumed_tokens: Some(Some(7)),
            released_tokens: 0,
            reconciliation_required: false,
        },
        Case {
            label: "unknown_retained",
            seed: 504,
            confidence: UsageConfidence::Unknown,
            retain_usage: true,
            with_release: false,
            state: SettlementFoldState::Unknown,
            usage_class: Some(SettlementUsageClass::Unknown),
            consumed_tokens: Some(Some(7)),
            released_tokens: 0,
            reconciliation_required: true,
        },
        Case {
            label: "unknown_without_usage",
            seed: 505,
            confidence: UsageConfidence::Unknown,
            retain_usage: false,
            with_release: false,
            state: SettlementFoldState::Unknown,
            usage_class: Some(SettlementUsageClass::Unknown),
            consumed_tokens: None,
            released_tokens: 0,
            reconciliation_required: true,
        },
    ];

    for case in cases {
        let fixture = pinned_fixture(case.seed);
        let facts = fold_path_facts(
            &fixture,
            case.confidence,
            case.retain_usage,
            case.with_release,
        );
        let first = replay(&facts);
        let second = replay(&facts);
        let a = first.get(fixture.attempt_id).unwrap().clone();
        let b = second.get(fixture.attempt_id).unwrap().clone();

        // Folding the same committed facts twice is a pure function: identical records, digests,
        // source cursor and revision.  Nothing in the fold depends on wall clock or iteration order.
        assert_eq!(a, b, "{} path must be deterministic", case.label);
        assert_eq!(a.record_digest, b.record_digest, "{}", case.label);
        assert_eq!(a.revision, facts.len() as u64, "{}", case.label);
        assert_eq!(a.state, case.state, "{}", case.label);
        assert_eq!(a.usage_class, case.usage_class, "{}", case.label);
        assert_eq!(
            a.consumed.map(|units| units.tokens),
            case.consumed_tokens,
            "{}",
            case.label
        );
        assert_eq!(a.released.tokens, case.released_tokens, "{}", case.label);
        assert_eq!(
            a.reconciliation_required, case.reconciliation_required,
            "{}",
            case.label
        );
        // The reservation itself is never rewritten by a fold: only the occupancy moves.
        assert_eq!(
            a.reserved,
            ReservationUnits {
                requests: 1,
                tokens: 100,
                concurrency: 1,
            },
            "{}",
            case.label
        );
        assert_eq!(a.released.requests, 0, "{}", case.label);
        assert_eq!(a.released.concurrency, 0, "{}", case.label);
    }

    // One identity, three outcomes: known, partial and unknown stay distinguishable so a reader
    // cannot silently merge them (or turn "unknown" into "zero").
    let shared = pinned_fixture(601);
    let ledgers = [
        replay(&fold_path_facts(
            &shared,
            UsageConfidence::Known,
            false,
            false,
        )),
        replay(&fold_path_facts(
            &shared,
            UsageConfidence::Partial,
            false,
            false,
        )),
        replay(&fold_path_facts(
            &shared,
            UsageConfidence::Unknown,
            false,
            false,
        )),
    ];
    let digests: Vec<String> = ledgers
        .iter()
        .map(|ledger| ledger.get(shared.attempt_id).unwrap().record_digest.clone())
        .collect();
    assert_ne!(digests[0], digests[1]);
    assert_ne!(digests[1], digests[2]);
    assert_ne!(digests[0], digests[2]);
    assert_eq!(
        ledgers[0].get(shared.attempt_id).unwrap().state,
        SettlementFoldState::Consumed
    );
    assert_eq!(
        ledgers[1].get(shared.attempt_id).unwrap().usage_class,
        Some(SettlementUsageClass::Partial)
    );
    assert!(
        ledgers[2]
            .get(shared.attempt_id)
            .unwrap()
            .reconciliation_required
    );
}

#[test]
fn fold_facts_are_registered_versioned_billing_attempt_events() {
    let fixture = pinned_fixture(700);
    let facts = fold_path_facts(&fixture, UsageConfidence::Known, false, true);
    let mut kinds = Vec::new();
    for fact in &facts {
        let runtime = RuntimeEvent::new(
            RequestId::new(),
            1,
            fact.kind.as_str(),
            serde_json::to_value(fact).unwrap(),
        )
        .unwrap()
        .with_stream_metadata("billing_attempt", fact.attempt_id.to_string(), 1);
        validate_runtime_event(&runtime).unwrap();
        validate_event_payload(&runtime.kind, &runtime.data).unwrap();
        kinds.push(runtime.kind.clone());
    }
    assert_eq!(
        kinds,
        [
            "usage.reserved",
            "usage.observed",
            "usage.settled",
            "usage.released",
        ]
    );

    // The registry keeps the fold identity: a fact without the reservation link, and a fact
    // smuggling an unaudited field, cannot enter the log.
    let mut payload = serde_json::to_value(&facts[0]).unwrap();
    assert!(payload
        .as_object_mut()
        .unwrap()
        .remove("reservation_id")
        .is_some());
    assert_eq!(
        validate_event_payload("usage.reserved", &payload).unwrap_err(),
        "event_required_id_missing:reservation_id"
    );
    let mut payload = serde_json::to_value(&facts[0]).unwrap();
    payload
        .as_object_mut()
        .unwrap()
        .insert("permit_id".to_owned(), serde_json::json!("request:1"));
    assert_eq!(
        validate_event_payload("usage.reserved", &payload).unwrap_err(),
        "event_payload_unknown_field"
    );
}
