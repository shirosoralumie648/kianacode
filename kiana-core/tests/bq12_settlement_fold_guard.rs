//! BQ-12 settlement-fold projection guard (deny first, success after).
//!
//! The projector consumes committed `usage.*` facts only and is deliberately read-only.  The
//! fixtures below first pin the refusals (automatic zeroing of an unknown attempt, releasing a
//! held reservation, double settlement/release, identity/digest/revision drift and malformed
//! envelopes) and then prove that the known, partial and unknown paths rebuild a deterministic,
//! conservative ledger from the EventLog alone.

use kiana_core::{
    project_settlement_folds, SettlementFoldProjection, SettlementFoldProjectionError,
};
use kiana_domain::*;
use serde_json::json;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// A fresh event identity, fresh source reference and explicit revision: exactly what a second,
/// conflicting fact looks like on the wire.
fn fact(
    kind: SettlementFoldEventKind,
    base: &SettlementFoldEvent,
    revision: u64,
) -> SettlementFoldEvent {
    let mut event = base.clone();
    event.kind = kind;
    event.event_id = EventId::new();
    event.revision = revision;
    event.source_refs = vec![SettlementSourceRef::new(EventId::new(), digest('f')).unwrap()];
    event.event_digest = event.digest();
    event
}

fn reserved_fact() -> SettlementFoldEvent {
    let mut event = SettlementFoldEvent {
        schema: SETTLEMENT_FOLD_EVENT_SCHEMA.to_owned(),
        version: SETTLEMENT_FOLD_VERSION,
        kind: SettlementFoldEventKind::Reserved,
        event_id: EventId::new(),
        run_id: RunId::new(),
        model_attempt_id: ModelAttemptId::new(),
        attempt_id: AttemptId::new(),
        reservation_id: QuotaReservationId::new(),
        reservation_digest: digest('a'),
        usage_class: None,
        usage_digest: None,
        receipt_id: None,
        reserved: ReservationUnits {
            requests: 1,
            tokens: 100,
            concurrency: 1,
        },
        consumed: None,
        released: ReservationUnits::default(),
        unknown_reason: None,
        reconciliation_required: false,
        source_refs: vec![SettlementSourceRef::new(EventId::new(), digest('c')).unwrap()],
        revision: 1,
        event_digest: String::new(),
    };
    event.event_digest = event.digest();
    event.validate().unwrap();
    event
}

fn observed_fact(
    reserved: &SettlementFoldEvent,
    class: SettlementUsageClass,
    tokens: u64,
) -> SettlementFoldEvent {
    let mut event = fact(SettlementFoldEventKind::Observed, reserved, 2);
    event.usage_class = Some(class);
    event.usage_digest = Some(digest('b'));
    event.consumed = Some(ConsumedUnits {
        requests: 1,
        tokens: Some(tokens),
        concurrency: 1,
    });
    event.event_digest = event.digest();
    event
}

fn settled_fact(observed: &SettlementFoldEvent, revision: u64) -> SettlementFoldEvent {
    let mut event = fact(SettlementFoldEventKind::Consumed, observed, revision);
    event.receipt_id = Some(ReceiptId::new());
    event.event_digest = event.digest();
    event
}

fn released_fact(
    settled: &SettlementFoldEvent,
    revision: u64,
    released_tokens: u64,
) -> SettlementFoldEvent {
    let mut event = fact(SettlementFoldEventKind::Released, settled, revision);
    event.released = ReservationUnits {
        requests: 0,
        tokens: released_tokens,
        concurrency: 0,
    };
    event.event_digest = event.digest();
    event
}

/// An unknown terminal fact; `retained_tokens` keeps a partially observed portion, which stays
/// occupied and is never refunded before reconciliation.
fn unknown_fact(
    reserved: &SettlementFoldEvent,
    revision: u64,
    retained_tokens: Option<u64>,
) -> SettlementFoldEvent {
    let mut event = fact(SettlementFoldEventKind::Unknown, reserved, revision);
    event.usage_class = Some(SettlementUsageClass::Unknown);
    event.unknown_reason = Some(BillingUnknownReason::ResultUnknown);
    event.reconciliation_required = true;
    if let Some(tokens) = retained_tokens {
        event.usage_digest = Some(digest('b'));
        event.consumed = Some(ConsumedUnits {
            requests: 1,
            tokens: Some(tokens),
            concurrency: 1,
        });
    }
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

/// The stable refusal reason behind a projection error, whichever boundary caught it.
fn refusal(result: Result<SettlementFoldProjection, SettlementFoldProjectionError>) -> String {
    match result {
        Err(SettlementFoldProjectionError::Invalid(reason))
        | Err(SettlementFoldProjectionError::Transition(reason)) => reason,
        Err(SettlementFoldProjectionError::SourceEmpty) => {
            "settlement_fold_source_empty".to_owned()
        }
        Ok(projection) => panic!(
            "expected a refusal, projected {} settlement fold records",
            projection.records.len()
        ),
    }
}

#[test]
fn projector_folds_committed_settlement_and_keeps_source_cursor() {
    let reserved = reserved_fact();
    let observed = observed_fact(&reserved, SettlementUsageClass::Known, 12);
    let settled = settled_fact(&observed, 3);
    let projection = project_settlement_folds(&[
        runtime(&reserved, 1),
        runtime(&observed, 2),
        runtime(&settled, 3),
    ])
    .unwrap();
    assert_eq!(projection.source_cursor, 3);
    assert_eq!(projection.source_event_ids.len(), 3);
    assert_eq!(projection.records.len(), 1);
    assert_eq!(projection.records[0].state, SettlementFoldState::Consumed);
    assert_eq!(
        projection.records[0].consumed.as_ref().unwrap().tokens,
        Some(12)
    );
    assert_eq!(projection.records[0].released, ReservationUnits::default());
    assert!(!projection.records[0].reconciliation_required);
}

#[test]
fn projector_rejects_unknown_without_reconciliation_and_bad_schema() {
    let reserved = reserved_fact();
    let mut unknown = unknown_fact(&reserved, 2, None);
    unknown.reconciliation_required = false;
    unknown.event_digest = unknown.digest();
    assert!(matches!(
        project_settlement_folds(&[runtime(&reserved, 1), runtime(&unknown, 2)]),
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
fn projector_keeps_known_partial_and_unknown_paths_conservative_and_deterministic() {
    // Known: consumed, then explicitly released; only the unused remainder comes back.
    let reserved = reserved_fact();
    let observed = observed_fact(&reserved, SettlementUsageClass::Known, 12);
    let settled = settled_fact(&observed, 3);
    let released = released_fact(&settled, 4, 88);
    let events = [
        runtime(&reserved, 1),
        runtime(&observed, 2),
        runtime(&settled, 3),
        runtime(&released, 4),
    ];
    let projection = project_settlement_folds(&events).unwrap();
    // Rebuilding from the same committed facts is a pure function of the log.
    assert_eq!(project_settlement_folds(&events).unwrap(), projection);
    let record = projection
        .attempt(reserved.attempt_id)
        .expect("folded attempt");
    assert_eq!(record.state, SettlementFoldState::Released);
    assert_eq!(record.usage_class, Some(SettlementUsageClass::Known));
    assert_eq!(record.consumed.as_ref().unwrap().tokens, Some(12));
    assert_eq!(record.released.tokens, 88);
    assert_eq!(record.revision, 4);
    assert!(!record.reconciliation_required);

    // Partial: the known portion is retained, but the fold stays occupied and cannot refund it.
    let reserved = reserved_fact();
    let partial = observed_fact(&reserved, SettlementUsageClass::Partial, 7);
    let settled = settled_fact(&partial, 3);
    let projection = project_settlement_folds(&[
        runtime(&reserved, 1),
        runtime(&partial, 2),
        runtime(&settled, 3),
    ])
    .unwrap();
    let record = projection.attempt(reserved.attempt_id).unwrap();
    assert_eq!(record.state, SettlementFoldState::Consumed);
    assert_eq!(record.usage_class, Some(SettlementUsageClass::Partial));
    assert_eq!(record.consumed.as_ref().unwrap().tokens, Some(7));
    assert_eq!(record.released, ReservationUnits::default());
    let mut partial_release = released_fact(&settled, 4, 93);
    partial_release.usage_class = Some(SettlementUsageClass::Partial);
    partial_release.event_digest = partial_release.digest();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&partial, 2),
            runtime(&settled, 3),
            runtime(&partial_release, 4),
        ])),
        "settlement_released_payload_invalid"
    );

    // Unknown: the whole reservation stays held and reconciliation stays required.
    let reserved = reserved_fact();
    let unknown = unknown_fact(&reserved, 2, None);
    let projection =
        project_settlement_folds(&[runtime(&reserved, 1), runtime(&unknown, 2)]).unwrap();
    let record = projection.attempt(reserved.attempt_id).unwrap();
    assert_eq!(record.state, SettlementFoldState::Unknown);
    assert_eq!(record.usage_class, Some(SettlementUsageClass::Unknown));
    assert!(record.reconciliation_required);
    assert_eq!(record.released, ReservationUnits::default());
    assert!(record.consumed.is_none());

    // An unknown attempt cannot be auto-released or settled by a later fold fact.
    let mut release = fact(SettlementFoldEventKind::Released, &unknown, 3);
    release.usage_class = Some(SettlementUsageClass::Known);
    release.usage_digest = Some(digest('b'));
    release.receipt_id = Some(ReceiptId::new());
    release.consumed = Some(ConsumedUnits {
        requests: 1,
        tokens: Some(12),
        concurrency: 1,
    });
    release.released = ReservationUnits {
        requests: 0,
        tokens: 88,
        concurrency: 0,
    };
    release.unknown_reason = None;
    release.reconciliation_required = false;
    release.event_digest = release.digest();
    release.validate().unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&unknown, 2),
            runtime(&release, 3),
        ])),
        "settlement_unknown_requires_reconciliation"
    );

    // A retained observation on the unknown path is reported, not refunded.
    let reserved = reserved_fact();
    let unknown = unknown_fact(&reserved, 2, Some(7));
    let projection =
        project_settlement_folds(&[runtime(&reserved, 1), runtime(&unknown, 2)]).unwrap();
    let record = projection.attempt(reserved.attempt_id).unwrap();
    assert_eq!(record.consumed.as_ref().unwrap().tokens, Some(7));
    assert_eq!(record.released, ReservationUnits::default());
    assert!(record.reconciliation_required);
}

#[test]
fn projector_rejects_duplicate_settlement_release_and_digest_drift() {
    let reserved = reserved_fact();
    let observed = observed_fact(&reserved, SettlementUsageClass::Known, 12);
    let settled = settled_fact(&observed, 3);

    // A second settlement with fresh event identity cannot charge the usage twice.
    let second_settlement = fact(SettlementFoldEventKind::Consumed, &settled, 4);
    second_settlement.validate().unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            runtime(&settled, 3),
            runtime(&second_settlement, 4),
        ])),
        "settlement_duplicate_settlement"
    );

    // A second release cannot refund the unused remainder twice.
    let released = released_fact(&settled, 4, 88);
    let second_release = released_fact(&settled, 5, 88);
    second_release.validate().unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            runtime(&settled, 3),
            runtime(&released, 4),
            runtime(&second_release, 5),
        ])),
        "settlement_duplicate_release"
    );

    // Refunding more than the unused remainder is refused before any accounting moves.
    let over_refund = released_fact(&settled, 4, 100);
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            runtime(&settled, 3),
            runtime(&over_refund, 4),
        ])),
        "settlement_release_must_be_unused_reservation"
    );

    // A usage digest that drifts from the observed one cannot be settled.
    let mut drifted = settled_fact(&observed, 3);
    drifted.usage_digest = Some(digest('9'));
    drifted.event_digest = drifted.digest();
    drifted.validate().unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            runtime(&drifted, 3),
        ])),
        "settlement_usage_digest_conflict"
    );

    // A revision gap is neither an implicit replay nor a retry.
    let gap = settled_fact(&observed, 4);
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            runtime(&gap, 3),
        ])),
        "settlement_revision_conflict"
    );

    // Reusing a source event reference would let one committed fact drive two folds.
    let mut reused_source = settled_fact(&observed, 3);
    reused_source.source_refs = reserved.source_refs.clone();
    reused_source.event_digest = reused_source.digest();
    reused_source.validate().unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            runtime(&reused_source, 3),
        ])),
        "settlement_source_event_replay_conflict"
    );

    // Re-using a runtime event id would double count the same committed fact.
    let first = runtime(&settled, 3);
    let mut duplicate_event_id = first.clone();
    duplicate_event_id.sequence = 4;
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&observed, 2),
            first,
            duplicate_event_id,
        ])),
        "settlement_duplicate_source_event"
    );

    // A terminal fact keyed to another attempt cannot attach itself to this reservation.
    let mut foreign = settled_fact(&observed, 2);
    foreign.attempt_id = AttemptId::new();
    foreign.event_digest = foreign.digest();
    foreign.validate().unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            runtime(&foreign, 2),
        ])),
        "settlement_requires_reserved_fact"
    );
}

#[test]
fn projector_ignores_non_fold_usage_events_and_reports_empty_sources() {
    let noise = [
        RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap(),
        RuntimeEvent::new(
            RequestId::new(),
            2,
            "usage.cost_estimated",
            json!({"schema":"kiana.cost-estimate.v1"}),
        )
        .unwrap(),
        RuntimeEvent::new(
            RequestId::new(),
            3,
            "usage.cost_measured",
            json!({"schema":"kiana.cost-measure.v1"}),
        )
        .unwrap(),
        RuntimeEvent::new(RequestId::new(), 4, "model.prepared", json!({})).unwrap(),
    ];
    assert!(matches!(
        project_settlement_folds(&[]),
        Err(SettlementFoldProjectionError::SourceEmpty)
    ));
    assert!(matches!(
        project_settlement_folds(&noise),
        Err(SettlementFoldProjectionError::SourceEmpty)
    ));

    // Only the fold facts drive the cursor; unrelated usage/cost facts neither fold nor advance it.
    let reserved = reserved_fact();
    let mixed = [
        noise[0].clone(),
        noise[2].clone(),
        runtime(&reserved, 1),
        noise[1].clone(),
    ];
    let projection = project_settlement_folds(&mixed).unwrap();
    assert_eq!(projection.source_cursor, 1);
    assert_eq!(projection.source_event_ids.len(), 1);
    assert_eq!(projection.records.len(), 1);
    assert_eq!(projection.records[0].state, SettlementFoldState::Reserved);
    assert_eq!(projection.records[0].released, ReservationUnits::default());

    // A fold kind without the versioned schema, an undecodable payload and a kind mismatch all
    // fail closed instead of being read as a partial fold.
    let schema_less = RuntimeEvent::new(RequestId::new(), 1, "usage.settled", json!({})).unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[schema_less])),
        "settlement_fold_schema_required"
    );
    let undecodable = RuntimeEvent::new(
        RequestId::new(),
        1,
        "usage.settled",
        json!({"schema": SETTLEMENT_FOLD_EVENT_SCHEMA, "kind": "consumed"}),
    )
    .unwrap();
    assert_eq!(
        refusal(project_settlement_folds(&[undecodable])),
        "settlement_fold_decode_failed"
    );
    let mut envelope_mismatch = runtime(&unknown_fact(&reserved, 2, None), 2);
    envelope_mismatch.kind = "usage.settled".to_owned();
    assert_eq!(
        refusal(project_settlement_folds(&[
            runtime(&reserved, 1),
            envelope_mismatch,
        ])),
        "settlement_fold_kind_mismatch"
    );
}

#[test]
fn source_guard_keeps_fold_projector_read_only_and_conservative() {
    let domain = include_str!("../../kiana-domain/src/billing_settlement_fold.rs");
    let core = include_str!("../src/billing_settlement_fold.rs");
    for marker in [
        "SettlementFoldState",
        "SettlementUsageClass",
        "SettlementFoldEventKind",
        "SettlementFoldApplyOutcome",
        "from_lifecycle",
        "observe_from_lifecycle",
        "settle_from_lifecycle",
        "release_unused",
        "settlement_duplicate_settlement",
        "settlement_duplicate_release",
        "settlement_requires_reserved_fact",
        "settlement_unknown_cannot_release",
        "settlement_partial_release_requires_reconciliation",
        "settlement_release_must_be_unused_reservation",
        "settlement_release_requires_known_tokens",
        "settlement_consumption_exceeds_reservation",
        "settlement_identity_or_reservation_conflict",
        "settlement_usage_digest_conflict",
        "settlement_revision_conflict",
        "settlement_source_event_replay_conflict",
        "settlement_unknown_requires_reconciliation",
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
        "commit_transition",
        "append_event(",
        "std::fs",
        "std::net",
        "TcpStream",
        "unsafe",
        "#[ignore",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "BQ-12 fold gained effect authority: {forbidden}"
        );
    }

    // Conservative accounting is spelled out in the source: the only release constructor is the
    // unused remainder, and nothing in the fold mints a fresh identity of its own.
    assert!(domain.contains("checked_difference"));
    assert!(domain.contains("settlement_unknown_requires_reconciliation"));
    assert!(!domain.contains("Uuid::new_v4"));
    assert!(!core.contains("Uuid::new_v4"));
    assert!(core.contains("fn project_settlement_folds"));
    assert!(core.contains("fn project_settlement_fold"));
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

#[test]
fn bq12_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    let lane = include_str!("../../.github/workflows/bq12-settlement-fold.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    let domain_fixtures = include_str!("../../kiana-domain/tests/bq12_settlement_fold.rs");
    let core_fixtures = include_str!("./bq12_settlement_fold_guard.rs");

    // The manual lane stays manual-only and keeps the bounded profile.
    assert!(lane.contains("workflow_dispatch:"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "BQ-12 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq12_settlement_fold --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq12_settlement_fold_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test bq12_settlement_fold --no-run --locked",
        "cargo test -p kiana-core --test bq12_settlement_fold_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "BQ-12 lane step missing: {step}");
    }
    for target in ["\"bq12_settlement_fold\"", "\"bq12_settlement_fold_guard\""] {
        assert!(
            shards.contains(target),
            "BQ-12 target missing from the automatic shard matrix: {target}"
        );
    }
    for fixture in [
        "cancel_timeout_and_eof_never_become_an_automatic_zero_or_release",
        "unknown_keeps_the_observed_portion_but_never_refunds_it",
        "observed_or_reserved_attempts_cannot_release_without_a_known_settlement",
        "known_usage_is_never_refunded_only_the_unused_remainder_is_released",
        "duplicate_settlement_and_release_cannot_charge_or_refund_twice",
        "identity_usage_digest_and_revision_drift_fail_closed",
        "known_partial_and_unknown_paths_produce_a_deterministic_ledger",
        "fold_facts_are_registered_versioned_billing_attempt_events",
    ] {
        assert!(
            domain_fixtures.contains(fixture),
            "BQ-12 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "projector_folds_committed_settlement_and_keeps_source_cursor",
        "projector_rejects_unknown_without_reconciliation_and_bad_schema",
        "projector_keeps_known_partial_and_unknown_paths_conservative_and_deterministic",
        "projector_rejects_duplicate_settlement_release_and_digest_drift",
        "projector_ignores_non_fold_usage_events_and_reports_empty_sources",
        "source_guard_keeps_fold_projector_read_only_and_conservative",
        "event_registry_requires_versioned_usage_fold_facts",
    ] {
        assert!(
            core_fixtures.contains(fixture),
            "BQ-12 Core fixture missing: {fixture}"
        );
    }
}
