//! BQ-11 model-attempt lifecycle fixtures (deny first, success after).
//!
//! The fixtures drive the pure `ModelAttemptLifecycleLedger` exactly like an adapter would:
//! prepare → flush acknowledgement → dispatch → observe → settle/unknown.  Every refusal below
//! is asserted before the corresponding happy path, and the happy path additionally proves that
//! the committed fact carries the complete run → turn → attempt → usage → receipt linkage.

use kiana_domain::{
    validate_event_payload, validate_runtime_event, AttemptId, BillingUnknownReason,
    ClockObservation, ModelAttemptError, ModelAttemptId, ModelAttemptIdentity,
    ModelAttemptLifecycleLedger, ModelAttemptState, NormalizedUsage, QuotaGroupKey,
    QuotaReservation, QuotaReservationId, QuotaWindow, ReceiptId, RequestId, RunId, StepId, TurnId,
    UsageConfidence, UsageId, UsageObservation, UsageSource, UsageVector,
    MODEL_ATTEMPT_EVENT_SCHEMA,
};

const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

struct Fixture {
    ledger: ModelAttemptLifecycleLedger,
    run_id: RunId,
    turn_id: TurnId,
    model_attempt_id: ModelAttemptId,
    attempt_id: AttemptId,
    reservation_id: QuotaReservationId,
    permit: RequestId,
    usage: NormalizedUsage,
}

fn usage_for(
    attempt_id: AttemptId,
    run_id: RunId,
    confidence: UsageConfidence,
    sequence: Option<u64>,
) -> NormalizedUsage {
    let unknown_reason = match confidence {
        UsageConfidence::Known => None,
        UsageConfidence::Partial => Some(BillingUnknownReason::Partial),
        UsageConfidence::Unknown => Some(BillingUnknownReason::ProviderUnreported),
    };
    NormalizedUsage::new(
        UsageId::new(),
        attempt_id,
        run_id,
        Some("provider".to_owned()),
        "model",
        "route",
        UsageVector::zero(),
        UsageSource::Provider,
        UsageObservation::Snapshot,
        sequence,
        confidence,
        unknown_reason,
        "bq11_fixture",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

fn fixture() -> Fixture {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let model_attempt_id = ModelAttemptId::new();
    let attempt_id = AttemptId::new();
    let identity = ModelAttemptIdentity::new(
        run_id,
        turn_id,
        StepId::new(),
        model_attempt_id,
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
    let reservation_id = reservation.reservation_id;
    let usage = usage_for(attempt_id, run_id, UsageConfidence::Known, Some(1));
    Fixture {
        ledger: ModelAttemptLifecycleLedger::new(identity, attempt_id, &reservation).unwrap(),
        run_id,
        turn_id,
        model_attempt_id,
        attempt_id,
        reservation_id,
        permit: RequestId::new(),
        usage,
    }
}

fn dispatch_attempt(fixture: &mut Fixture) {
    fixture.ledger.prepare(Some(fixture.permit)).unwrap();
    fixture.ledger.mark_prepared_flushed(1).unwrap();
    fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
}

#[test]
fn lifecycle_links_run_turn_attempt_usage_and_receipt() {
    let mut fixture = fixture();
    assert_eq!(
        fixture
            .ledger
            .prepare(Some(fixture.permit))
            .unwrap()
            .kind
            .as_str(),
        "model.prepared"
    );
    assert_eq!(
        fixture.ledger.dispatch(Some(fixture.permit)).unwrap_err(),
        "model_attempt_prepared_not_flushed"
    );
    fixture.ledger.mark_prepared_flushed(7).unwrap();
    let dispatching = fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    assert_eq!(dispatching.kind.as_str(), "model.dispatching");
    assert_eq!(dispatching.permit_id, Some(fixture.permit));
    assert!(dispatching.prepared_flushed);
    assert_eq!(dispatching.flush_sequence, Some(7));
    fixture.ledger.observe(fixture.usage.clone()).unwrap();
    let receipt_id = ReceiptId::new();
    fixture
        .ledger
        .settle(receipt_id, Some(fixture.usage.clone()))
        .unwrap();

    let record = fixture.ledger.record().unwrap();
    assert_eq!(record.state, ModelAttemptState::Settled);
    assert_eq!(record.run_id, fixture.run_id);
    assert_eq!(record.turn_id, fixture.turn_id);
    assert_eq!(record.model_attempt_id, fixture.model_attempt_id);
    assert_eq!(record.attempt_id, fixture.attempt_id);
    assert_eq!(record.reservation_id, fixture.reservation_id);
    assert_eq!(record.permit_id, Some(fixture.permit));
    assert_eq!(record.receipt_id, Some(receipt_id));
    assert_eq!(
        record
            .usage
            .as_ref()
            .map(|usage| usage.usage_digest.as_str()),
        Some(fixture.usage.usage_digest.as_str())
    );
    assert!(record.prepared_flushed);
    assert_eq!(record.flush_sequence, Some(7));
    assert_eq!(record.revision, 4);
    assert_eq!(record.source_event_ids.len(), 4);
    record.validate().unwrap();

    let kinds = fixture
        .ledger
        .events()
        .iter()
        .map(|fact| fact.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            "model.prepared",
            "model.dispatching",
            "model.observed",
            "model.settled"
        ]
    );
    for fact in fixture.ledger.events() {
        let event = fact
            .into_runtime_event(RequestId::new(), fact.revision)
            .unwrap();
        validate_runtime_event(&event).unwrap();
        validate_event_payload(&event.kind, &event.data).unwrap();
        let mut mixed = event.data.clone();
        mixed["model_request_id"] = serde_json::json!(RequestId::new());
        assert!(validate_event_payload(&event.kind, &mixed).is_err());
    }
}

#[test]
fn deny_unprepared_missing_permit_and_duplicate_settlement() {
    let mut unprepared = fixture();
    assert_eq!(
        unprepared
            .ledger
            .dispatch(Some(unprepared.permit))
            .unwrap_err(),
        "model_attempt_not_prepared"
    );

    let mut missing_permit = fixture();
    missing_permit
        .ledger
        .prepare(Option::<RequestId>::None)
        .unwrap();
    missing_permit.ledger.mark_prepared_flushed(1).unwrap();
    assert_eq!(
        missing_permit
            .ledger
            .dispatch(Option::<RequestId>::None)
            .unwrap_err(),
        "model_attempt_permit_required"
    );

    let mut duplicate = fixture();
    dispatch_attempt(&mut duplicate);
    duplicate.ledger.observe(duplicate.usage.clone()).unwrap();
    duplicate
        .ledger
        .settle(ReceiptId::new(), Some(duplicate.usage.clone()))
        .unwrap();
    assert_eq!(
        duplicate
            .ledger
            .settle(ReceiptId::new(), Some(duplicate.usage.clone()))
            .unwrap_err(),
        "model_attempt_duplicate_settlement"
    );
}

#[test]
fn deny_flush_without_append_and_mismatched_permit() {
    let mut partial = fixture();
    assert_eq!(
        partial.ledger.mark_prepared_flushed(1).unwrap_err(),
        "model_attempt_prepared_append_required"
    );
    assert!(!partial.ledger.prepared_flushed());

    partial.ledger.prepare(Some(partial.permit)).unwrap();
    assert_eq!(
        partial.ledger.mark_prepared_flushed(0).unwrap_err(),
        "model_attempt_flush_sequence_invalid"
    );
    assert!(!partial.ledger.prepared_flushed());
    assert_eq!(
        partial.ledger.dispatch(Some(RequestId::new())).unwrap_err(),
        "model_attempt_permit_mismatch"
    );
    assert_eq!(partial.ledger.state(), Some(ModelAttemptState::Prepared));

    partial.ledger.mark_prepared_flushed(2).unwrap();
    assert!(partial.ledger.prepared_flushed());
    assert_eq!(
        partial.ledger.prepare(Some(partial.permit)).unwrap_err(),
        "model_attempt_already_prepared"
    );

    let mut nil_permit = fixture();
    assert_eq!(
        nil_permit
            .ledger
            .prepare(Some(RequestId::parse_str(NIL_UUID).unwrap()))
            .unwrap_err(),
        "model_attempt_permit_invalid"
    );
    assert_eq!(nil_permit.ledger.state(), None);
}

#[test]
fn deny_terminal_lifecycle_replay() {
    let mut settled = fixture();
    dispatch_attempt(&mut settled);
    settled.ledger.observe(settled.usage.clone()).unwrap();
    settled
        .ledger
        .settle(ReceiptId::new(), Some(settled.usage.clone()))
        .unwrap();
    assert_eq!(
        settled.ledger.dispatch(Some(settled.permit)).unwrap_err(),
        "model_attempt_dispatch_state_invalid"
    );
    assert_eq!(
        settled.ledger.observe(settled.usage.clone()).unwrap_err(),
        "model_attempt_observe_state_invalid"
    );
    assert_eq!(
        settled
            .ledger
            .unknown(
                Some(settled.usage.clone()),
                ModelAttemptError::new("provider_eof", "response", true).unwrap(),
            )
            .unwrap_err(),
        "model_attempt_unknown_state_invalid"
    );
    assert_eq!(
        settled.ledger.prepare(Some(settled.permit)).unwrap_err(),
        "model_attempt_already_prepared"
    );

    let mut unknown = fixture();
    dispatch_attempt(&mut unknown);
    unknown
        .ledger
        .unknown(
            Some(unknown.usage.clone()),
            ModelAttemptError::new("provider_eof", "response", true).unwrap(),
        )
        .unwrap();
    assert_eq!(
        unknown
            .ledger
            .unknown(
                None,
                ModelAttemptError::new("provider_eof", "response", true).unwrap(),
            )
            .unwrap_err(),
        "model_attempt_unknown_state_invalid"
    );
    assert_eq!(
        unknown
            .ledger
            .settle(ReceiptId::new(), Some(unknown.usage.clone()))
            .unwrap_err(),
        "model_attempt_settle_state_invalid"
    );
    assert_eq!(unknown.ledger.record().unwrap().receipt_id, None);
}

#[test]
fn deny_usage_identity_unknown_confidence_and_settlement_mismatch() {
    let mut fixture = fixture();
    dispatch_attempt(&mut fixture);

    let foreign_attempt = usage_for(
        AttemptId::new(),
        fixture.run_id,
        UsageConfidence::Known,
        Some(1),
    );
    assert_eq!(
        fixture.ledger.observe(foreign_attempt).unwrap_err(),
        "model_attempt_usage_identity_mismatch"
    );
    let foreign_run = usage_for(
        fixture.attempt_id,
        RunId::new(),
        UsageConfidence::Known,
        Some(1),
    );
    assert_eq!(
        fixture.ledger.observe(foreign_run).unwrap_err(),
        "model_attempt_usage_identity_mismatch"
    );
    let unknown_confidence = usage_for(
        fixture.attempt_id,
        fixture.run_id,
        UsageConfidence::Unknown,
        Some(1),
    );
    assert_eq!(
        fixture.ledger.observe(unknown_confidence).unwrap_err(),
        "model_attempt_unknown_usage_requires_unknown_state"
    );
    assert_eq!(fixture.ledger.state(), Some(ModelAttemptState::Dispatching));

    fixture.ledger.observe(fixture.usage.clone()).unwrap();
    let drifted = usage_for(
        fixture.attempt_id,
        fixture.run_id,
        UsageConfidence::Known,
        Some(2),
    );
    assert_eq!(
        fixture
            .ledger
            .settle(ReceiptId::new(), Some(drifted))
            .unwrap_err(),
        "model_attempt_settlement_usage_mismatch"
    );
    let nil_receipt = ReceiptId::parse_str(NIL_UUID).unwrap();
    assert_eq!(
        fixture
            .ledger
            .settle(nil_receipt, Some(fixture.usage.clone()))
            .unwrap_err(),
        "model_attempt_receipt_required"
    );
    fixture.ledger.settle(ReceiptId::new(), None).unwrap();
    assert_eq!(fixture.ledger.state(), Some(ModelAttemptState::Settled));
}

#[test]
fn unknown_attempt_retains_usage_and_error_for_reconciliation() {
    let mut fixture = fixture();
    dispatch_attempt(&mut fixture);
    let error = ModelAttemptError::new("provider_eof", "response", true).unwrap();
    fixture
        .ledger
        .unknown(Some(fixture.usage.clone()), error)
        .unwrap();
    let record = fixture.ledger.record().unwrap();
    assert_eq!(record.state, ModelAttemptState::Unknown);
    assert_eq!(
        record
            .usage
            .as_ref()
            .map(|usage| usage.usage_digest.as_str()),
        Some(fixture.usage.usage_digest.as_str())
    );
    assert_eq!(
        record.error.as_ref().map(|error| error.code.as_str()),
        Some("provider_eof")
    );
    assert!(record
        .error
        .as_ref()
        .is_some_and(|error| error.request_sent));
    assert_eq!(
        record.error.as_ref().map(|error| error.phase.as_str()),
        Some("response")
    );
    assert!(record.receipt_id.is_none());
    assert_eq!(record.revision, 3);
    record.validate().unwrap();
}

#[test]
fn event_contract_refuses_unflushed_and_malformed_lifecycle_payloads() {
    let mut fixture = fixture();
    let prepared = fixture.ledger.prepare(Some(fixture.permit)).unwrap();

    let mut bad_revision = prepared.clone();
    bad_revision.revision = 2;
    bad_revision.event_digest = bad_revision.digest();
    assert_eq!(
        bad_revision.validate().unwrap_err(),
        "model_attempt_event_header_invalid"
    );

    let mut tampered = prepared.clone();
    tampered.event_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    assert_eq!(
        tampered
            .into_runtime_event(RequestId::new(), 1)
            .unwrap_err(),
        "model_attempt_event_digest_mismatch"
    );
    assert_eq!(
        prepared
            .clone()
            .into_runtime_event(RequestId::parse_str(NIL_UUID).unwrap(), 1)
            .unwrap_err(),
        "model_attempt_runtime_event_identity_invalid"
    );
    assert_eq!(
        prepared
            .clone()
            .into_runtime_event(RequestId::new(), 0)
            .unwrap_err(),
        "model_attempt_runtime_event_identity_invalid"
    );

    fixture.ledger.mark_prepared_flushed(1).unwrap();
    let dispatch = fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    let mut unflushed = dispatch.clone();
    unflushed.prepared_flushed = false;
    unflushed.flush_sequence = None;
    unflushed.event_digest = unflushed.digest();
    assert_eq!(
        unflushed.validate().unwrap_err(),
        "model_attempt_dispatch_requires_prepared_flush"
    );
    // The registry keeps shape-only validation, so an unflushed dispatch envelope can still be
    // encoded; the domain validator and the Core projector are the fences that refuse it.
    let unflushed_event = kiana_domain::RuntimeEvent::new(
        RequestId::new(),
        2,
        "model.dispatching",
        serde_json::to_value(&unflushed).unwrap(),
    )
    .unwrap();
    validate_event_payload("model.dispatching", &unflushed_event.data).unwrap();

    // A lifecycle kind without the BQ-11 schema is refused at the event contract, and the
    // schema-bearing shape is accepted for the same kind.
    let schema_less = serde_json::json!({
        "run_id": fixture.run_id,
        "turn_id": fixture.turn_id,
        "model_attempt_id": fixture.model_attempt_id,
        "attempt_id": fixture.attempt_id,
        "reservation_id": fixture.reservation_id,
    });
    for kind in ["model.observed", "model.unknown"] {
        assert_eq!(
            validate_event_payload(kind, &schema_less).unwrap_err(),
            "event_model_payload_schema_invalid",
            "{kind}"
        );
        let mut lifecycle_schema = schema_less.clone();
        lifecycle_schema["schema"] = serde_json::json!(MODEL_ATTEMPT_EVENT_SCHEMA);
        validate_event_payload(kind, &lifecycle_schema).unwrap();
    }
}
