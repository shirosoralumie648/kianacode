use kiana_core::{project_settlement_folds, SettlementFoldProjectionError};
use kiana_domain::*;
use serde_json::json;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn consumed_event() -> SettlementFoldEvent {
    let mut event = SettlementFoldEvent {
        schema: SETTLEMENT_FOLD_EVENT_SCHEMA.to_owned(),
        version: SETTLEMENT_FOLD_VERSION,
        kind: SettlementFoldEventKind::Consumed,
        event_id: EventId::new(),
        run_id: RunId::new(),
        model_attempt_id: ModelAttemptId::new(),
        attempt_id: AttemptId::new(),
        reservation_id: QuotaReservationId::new(),
        reservation_digest: digest('a'),
        usage_class: Some(SettlementUsageClass::Known),
        usage_digest: Some(digest('b')),
        receipt_id: Some(ReceiptId::new()),
        reserved: ReservationUnits {
            requests: 1,
            tokens: 100,
            concurrency: 1,
        },
        consumed: Some(ConsumedUnits {
            requests: 1,
            tokens: Some(12),
            concurrency: 1,
        }),
        released: ReservationUnits::default(),
        unknown_reason: None,
        reconciliation_required: false,
        source_refs: vec![SettlementSourceRef::new(EventId::new(), digest('c')).unwrap()],
        revision: 1,
        event_digest: String::new(),
    };
    event.event_digest = event.digest();
    event
}

fn reserved_event(consumed: &SettlementFoldEvent) -> SettlementFoldEvent {
    let mut event = consumed.clone();
    event.kind = SettlementFoldEventKind::Reserved;
    event.event_id = EventId::new();
    event.usage_class = None;
    event.usage_digest = None;
    event.receipt_id = None;
    event.consumed = None;
    event.unknown_reason = None;
    event.reconciliation_required = false;
    event.source_refs = vec![SettlementSourceRef::new(EventId::new(), digest('d')).unwrap()];
    event.revision = 1;
    event.event_digest = event.digest();
    event
}

fn runtime(fact: &SettlementFoldEvent, sequence: u64) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        fact.kind.as_str(),
        serde_json::to_value(fact).unwrap(),
    )
    .unwrap()
    .with_stream_metadata("billing_attempt", fact.attempt_id.to_string(), sequence)
}

#[test]
fn projector_folds_committed_settlement_and_keeps_source_cursor() {
    let mut fact = consumed_event();
    let reserved = reserved_event(&fact);
    fact.event_id = EventId::new();
    fact.source_refs = vec![SettlementSourceRef::new(EventId::new(), digest('e')).unwrap()];
    fact.revision = 2;
    fact.event_digest = fact.digest();
    let projection = project_settlement_folds(&[runtime(&reserved, 1), runtime(&fact, 2)]).unwrap();
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.records[0].state, SettlementFoldState::Consumed);
}

#[test]
fn projector_rejects_unknown_without_reconciliation_and_bad_schema() {
    let mut fact = consumed_event();
    fact.kind = SettlementFoldEventKind::Unknown;
    fact.receipt_id = None;
    fact.usage_class = Some(SettlementUsageClass::Unknown);
    fact.unknown_reason = Some(BillingUnknownReason::ResultUnknown);
    fact.reconciliation_required = false;
    fact.event_digest = fact.digest();
    let reserved = reserved_event(&fact);
    fact.event_id = EventId::new();
    fact.source_refs = vec![SettlementSourceRef::new(EventId::new(), digest('e')).unwrap()];
    fact.revision = 2;
    fact.event_digest = fact.digest();
    let event = runtime(&fact, 2);
    assert!(matches!(
        project_settlement_folds(&[runtime(&reserved, 1), event]),
        Err(SettlementFoldProjectionError::Transition(_))
            | Err(SettlementFoldProjectionError::Invalid(_))
    ));

    let bad = RuntimeEvent::new(
        RequestId::new(),
        1,
        "usage.settled",
        json!({"schema":"legacy"}),
    )
    .unwrap();
    assert!(matches!(
        project_settlement_folds(&[bad]),
        Err(SettlementFoldProjectionError::Invalid(_))
    ));
}

#[test]
fn source_guard_keeps_fold_projector_read_only_and_conservative() {
    let domain = include_str!("../../kiana-domain/src/billing_settlement_fold.rs");
    let core = include_str!("../src/billing_settlement_fold.rs");
    for marker in [
        "SettlementFoldState",
        "SettlementUsageClass",
        "settlement_duplicate_settlement",
        "settlement_duplicate_release",
        "settlement_requires_reserved_fact",
        "settlement_unknown_cannot_release",
        "settlement_partial_release_requires_reconciliation",
        "reservation_digest",
        "usage_digest",
        "receipt_id",
        "source_refs",
        "reconciliation_required",
        "project_settlement_folds",
        "apply_settlement_fold_event",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "BQ-12 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ProviderClient",
        "EventStore::append",
        "tokio::spawn",
        "reqwest::",
        "std::process::Command",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "BQ-12 fold gained effect authority: {forbidden}"
        );
    }
}

#[test]
fn event_registry_requires_versioned_usage_fold_facts() {
    let registry = include_str!("../../kiana-domain/src/event_contracts.rs");
    for kind in [
        "usage.reserved",
        "usage.observed",
        "usage.settled",
        "usage.released",
        "usage.unknown",
    ] {
        let spec = event_kind_spec(kind).expect("registered settlement event");
        assert_eq!(spec.aggregate_type, "billing_attempt");
        assert!(spec.required_ids.contains(&"reservation_id"));
        assert!(spec.allowed_fields.contains(&"reservation_digest"));
        assert!(spec.allowed_fields.contains(&"source_refs"));
    }
    assert!(registry.contains("SETTLEMENT_FOLD_FIELDS"));
}
