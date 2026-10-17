//! BQ-11 Core-side fixtures: the read-only model-attempt lifecycle projector, the shared
//! dispatch fence and the source/lane guards.  Denials run before the success path.

use kiana_core::{
    project_model_attempt_lifecycle, validate_model_attempt_dispatch,
    ModelAttemptLifecycleProjectionError,
};
use kiana_domain::{
    AttemptId, ClockObservation, EventId, ModelAttemptError, ModelAttemptEventKind,
    ModelAttemptIdentity, ModelAttemptLifecycleEvent, ModelAttemptLifecycleLedger,
    ModelAttemptState, NormalizedUsage, QuotaGroupKey, QuotaReservation, QuotaReservationId,
    QuotaWindow, RequestId, RunId, RuntimeEvent, StepId, TurnId, UsageConfidence, UsageId,
    UsageObservation, UsageSource, UsageVector, MODEL_ATTEMPT_EVENT_SCHEMA,
};

struct Fixture {
    ledger: ModelAttemptLifecycleLedger,
    run_id: RunId,
    attempt_id: AttemptId,
    permit: RequestId,
    usage: NormalizedUsage,
}

fn usage_for(attempt_id: AttemptId, run_id: RunId) -> NormalizedUsage {
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
        Some(1),
        UsageConfidence::Known,
        None,
        "bq11_core_fixture",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap()
}

fn ledger() -> Fixture {
    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let identity = ModelAttemptIdentity::new(
        run_id,
        TurnId::new(),
        StepId::new(),
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
    let usage = usage_for(attempt_id, run_id);
    Fixture {
        ledger: ModelAttemptLifecycleLedger::new(identity, attempt_id, &reservation).unwrap(),
        run_id,
        attempt_id,
        permit: RequestId::new(),
        usage,
    }
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

/// Encode a lifecycle fact into the event envelope *without* running the domain validator, the
/// way a buggy adapter or a tampered log entry would look.
fn runtime_event(fact: &ModelAttemptLifecycleEvent) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        fact.revision,
        fact.kind.as_str(),
        serde_json::to_value(fact).unwrap(),
    )
    .unwrap()
}

fn restamped(mut fact: ModelAttemptLifecycleEvent) -> ModelAttemptLifecycleEvent {
    fact.event_id = EventId::new();
    fact.event_digest = fact.digest();
    fact
}

#[test]
fn projection_rebuilds_only_prepared_flushed_lifecycle() {
    let mut fixture = ledger();
    let prepared = fixture.ledger.prepare(Some(fixture.permit)).unwrap();
    fixture.ledger.mark_prepared_flushed(9).unwrap();
    let dispatching = fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    let events = committed_events(&fixture.ledger);
    assert_eq!(events.len(), 2);

    let projection = project_model_attempt_lifecycle(&events).unwrap();
    assert_eq!(projection.attempts.len(), 1);
    let record = &projection.attempts[0];
    assert_eq!(record.state, ModelAttemptState::Dispatching);
    assert_eq!(record.run_id, fixture.run_id);
    assert_eq!(record.turn_id, prepared.turn_id);
    assert_eq!(record.model_attempt_id, prepared.model_attempt_id);
    assert_eq!(record.attempt_id, fixture.attempt_id);
    assert_eq!(record.reservation_id, prepared.reservation_id);
    assert_eq!(record.permit_id, Some(fixture.permit));
    assert!(record.prepared_flushed);
    assert_eq!(record.flush_sequence, Some(9));
    assert_eq!(record.revision, dispatching.revision);
    assert_eq!(record.source_event_ids.len(), events.len());
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.source_event_ids.len(), events.len());
    assert!(projection.attempt(record.attempt_id).is_some());
    assert!(projection.attempt(AttemptId::new()).is_none());
    // Replaying the same committed facts rebuilds the same record: the read model is derived.
    assert_eq!(
        project_model_attempt_lifecycle(&events).unwrap(),
        projection
    );
    // A dispatched attempt is not "prepared", so the fence refuses a second dispatch.
    let error = validate_model_attempt_dispatch(record, Some(fixture.permit), true).unwrap_err();
    assert!(matches!(
        error,
        ModelAttemptLifecycleProjectionError::Transition(ref reason)
            if reason == "model_attempt_dispatch_state_invalid"
    ));
}

#[test]
fn projection_rejects_dispatch_without_prepared_fact() {
    let mut fixture = ledger();
    fixture.ledger.prepare(Some(fixture.permit)).unwrap();
    fixture.ledger.mark_prepared_flushed(1).unwrap();
    let dispatch = fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    let event = runtime_event(&dispatch);
    assert!(matches!(
        project_model_attempt_lifecycle(&[event]),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_event_requires_prepared"
    ));
}

#[test]
fn projection_rejects_unflushed_schema_less_and_duplicate_facts() {
    let mut fixture = ledger();
    fixture.ledger.prepare(Some(fixture.permit)).unwrap();
    fixture.ledger.mark_prepared_flushed(1).unwrap();
    let prepared = runtime_event(&fixture.ledger.events()[0]);
    let dispatch = fixture.ledger.dispatch(Some(fixture.permit)).unwrap();

    // A dispatch fact that claims no prepared-flush acknowledgement cannot enter the fold.
    let mut unflushed = dispatch.clone();
    unflushed.prepared_flushed = false;
    unflushed.flush_sequence = None;
    let unflushed = restamped(unflushed);
    assert!(matches!(
        project_model_attempt_lifecycle(&[prepared.clone(), runtime_event(&unflushed)]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_dispatch_requires_prepared_flush"
    ));

    // A lifecycle kind without the BQ-11 schema is not silently folded as an old fact.
    let mut schema_less = serde_json::to_value(&dispatch).unwrap();
    schema_less.as_object_mut().unwrap().remove("schema");
    let schema_less =
        RuntimeEvent::new(RequestId::new(), 2, "model.dispatching", schema_less).unwrap();
    assert!(matches!(
        project_model_attempt_lifecycle(&[prepared.clone(), schema_less]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_lifecycle_schema_required"
    ));

    // A schema-bearing payload that cannot be decoded, or that disagrees with the event kind,
    // fails closed instead of being dropped.
    let malformed = RuntimeEvent::new(
        RequestId::new(),
        2,
        "model.observed",
        serde_json::json!({ "schema": MODEL_ATTEMPT_EVENT_SCHEMA }),
    )
    .unwrap();
    assert!(matches!(
        project_model_attempt_lifecycle(&[prepared.clone(), malformed]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_lifecycle_decode_failed"
    ));
    let mismatched = RuntimeEvent::new(
        RequestId::new(),
        2,
        "model.settled",
        serde_json::to_value(&dispatch).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        project_model_attempt_lifecycle(&[prepared.clone(), mismatched]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_lifecycle_kind_mismatch"
    ));

    // Re-reading the same event id is a duplicate fact, not a second transition.
    assert!(matches!(
        project_model_attempt_lifecycle(&[prepared.clone(), prepared]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_duplicate_source_event"
    ));
}

#[test]
fn projection_rejects_duplicate_settlement_revision_and_usage_identity() {
    let mut fixture = ledger();
    fixture.ledger.prepare(Some(fixture.permit)).unwrap();
    fixture.ledger.mark_prepared_flushed(1).unwrap();
    fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    fixture.ledger.observe(fixture.usage.clone()).unwrap();
    fixture
        .ledger
        .settle(kiana_domain::ReceiptId::new(), Some(fixture.usage.clone()))
        .unwrap();
    let facts = fixture.ledger.events().to_vec();
    let events = committed_events(&fixture.ledger);
    assert_eq!(facts.len(), 4);

    // Same attempt must settle once: a second terminal settled fact is refused.
    let mut second_settlement = facts[3].clone();
    second_settlement.revision = 5;
    let second_settlement = restamped(second_settlement);
    let mut replay = events.clone();
    replay.push(runtime_event(&second_settlement));
    assert!(matches!(
        project_model_attempt_lifecycle(&replay),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_event_transition_invalid"
    ));

    // A revision gap is refused, and identity fields that are not part of the attempt key
    // (here the run) cannot drift between facts of one attempt.
    let mut revision_gap = facts[1].clone();
    revision_gap.revision = 3;
    let revision_gap = restamped(revision_gap);
    assert!(matches!(
        project_model_attempt_lifecycle(&[
            events[0].clone(),
            runtime_event(&revision_gap)
        ]),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_event_identity_or_revision_conflict"
    ));
    let mut run_drift = facts[1].clone();
    run_drift.run_id = RunId::new();
    let run_drift = restamped(run_drift);
    assert!(matches!(
        project_model_attempt_lifecycle(&[events[0].clone(), runtime_event(&run_drift)]),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_event_identity_or_revision_conflict"
    ));
    // A drifted attempt key is a *first* fact for another attempt, so it is refused as an
    // unprepared dispatch instead of being folded into the wrong record.
    let mut attempt_drift = facts[1].clone();
    attempt_drift.attempt_id = AttemptId::new();
    let attempt_drift = restamped(attempt_drift);
    assert!(matches!(
        project_model_attempt_lifecycle(&[events[0].clone(), runtime_event(&attempt_drift)]),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_event_requires_prepared"
    ));

    // Usage observed or settled under a different run/attempt identity is never folded.
    let mut foreign_usage = facts[2].clone();
    foreign_usage.usage = Some(usage_for(AttemptId::new(), fixture.run_id));
    let foreign_usage = restamped(foreign_usage);
    assert!(matches!(
        project_model_attempt_lifecycle(&[
            events[0].clone(),
            events[1].clone(),
            runtime_event(&foreign_usage)
        ]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_usage_identity_mismatch"
    ));

    // Settlement without usage, and an `unknown` fact without a bounded error, are refused.
    let mut no_usage_settlement = facts[3].clone();
    no_usage_settlement.usage = None;
    let no_usage_settlement = restamped(no_usage_settlement);
    let mut no_usage_replay = events[..3].to_vec();
    no_usage_replay.push(runtime_event(&no_usage_settlement));
    assert!(matches!(
        project_model_attempt_lifecycle(&no_usage_replay),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_settlement_usage_required"
    ));
    let mut unknown_without_error = facts[1].clone();
    unknown_without_error.kind = ModelAttemptEventKind::Unknown;
    unknown_without_error.revision = 3;
    let unknown_without_error = restamped(unknown_without_error);
    assert!(matches!(
        project_model_attempt_lifecycle(&[
            events[0].clone(),
            events[1].clone(),
            runtime_event(&unknown_without_error)
        ]),
        Err(ModelAttemptLifecycleProjectionError::Invalid(ref reason))
            if reason == "model_attempt_unknown_error_required"
    ));

    // A terminal attempt cannot be prepared again after a restart.
    let mut duplicate_prepared = facts[0].clone();
    duplicate_prepared.revision = 5;
    let duplicate_prepared = restamped(duplicate_prepared);
    let mut terminal_replay = events.clone();
    terminal_replay.push(runtime_event(&duplicate_prepared));
    assert!(matches!(
        project_model_attempt_lifecycle(&terminal_replay),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_duplicate_prepared"
    ));
}

#[test]
fn projection_folds_unknown_attempts_and_ignores_legacy_prepared() {
    let mut fixture = ledger();
    let legacy_prepared = RuntimeEvent::new(
        RequestId::new(),
        1,
        "model.prepared",
        serde_json::json!({
            "permit": { "run_id": fixture.run_id, "model_request_id": RequestId::new() },
            "prepared": { "request_hash": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" },
            "authority_versions": [],
        }),
    )
    .unwrap();
    fixture.ledger.prepare(Some(fixture.permit)).unwrap();
    fixture.ledger.mark_prepared_flushed(1).unwrap();
    fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    fixture
        .ledger
        .unknown(
            Some(fixture.usage.clone()),
            ModelAttemptError::new("provider_eof", "response", true).unwrap(),
        )
        .unwrap();

    // A legacy `model.prepared` envelope without the BQ-11 schema stays readable and is not
    // silently promoted into a billing fact; unrelated kinds are ignored too.
    let run_started = RuntimeEvent::new(
        RequestId::new(),
        42,
        "run.started",
        serde_json::json!({ "run_id": fixture.run_id }),
    )
    .unwrap();
    let mut events = vec![legacy_prepared, run_started];
    events.extend(committed_events(&fixture.ledger));

    let projection = project_model_attempt_lifecycle(&events).unwrap();
    assert_eq!(projection.attempts.len(), 1);
    let record = &projection.attempts[0];
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
    assert!(record.receipt_id.is_none());
    assert_eq!(record.source_event_ids.len(), 3);
    // The legacy and unrelated events do not move the lifecycle source cursor.
    assert_eq!(projection.source_cursor, 3);

    // An empty or lifecycle-free log is not a valid projection.
    assert!(matches!(
        project_model_attempt_lifecycle(&[]),
        Err(ModelAttemptLifecycleProjectionError::SourceEmpty)
    ));
    assert!(matches!(
        project_model_attempt_lifecycle(&[RuntimeEvent::new(
            RequestId::new(),
            1,
            "run.started",
            serde_json::json!({})
        )
        .unwrap()]),
        Err(ModelAttemptLifecycleProjectionError::SourceEmpty)
    ));
}

#[test]
fn dispatch_fence_requires_matching_permit_and_flushed_prepared_record() {
    let mut fixture = ledger();
    let prepared = fixture.ledger.prepare(Some(fixture.permit)).unwrap();

    // The prepared fact is written before the flush acknowledgement, so it cannot dispatch yet.
    let unflushed_record = ModelAttemptLifecycleRecord::from_event(&prepared).unwrap();
    assert!(matches!(
        validate_model_attempt_dispatch(&unflushed_record, Some(fixture.permit), true),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_prepared_not_flushed"
    ));

    let mut flushed = prepared.clone();
    flushed.prepared_flushed = true;
    flushed.flush_sequence = Some(1);
    let flushed = restamped(flushed);
    flushed.validate().unwrap();
    let record = ModelAttemptLifecycleRecord::from_event(&flushed).unwrap();

    // The exact permit that prepared the attempt is the only one that may dispatch it.
    assert!(validate_model_attempt_dispatch(&record, Some(fixture.permit), true).is_ok());
    assert!(matches!(
        validate_model_attempt_dispatch(&record, Some(RequestId::new()), true),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_permit_mismatch"
    ));
    assert!(matches!(
        validate_model_attempt_dispatch(&record, None, true),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_permit_required"
    ));
    assert!(matches!(
        validate_model_attempt_dispatch(&record, Some(fixture.permit), false),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_prepared_not_flushed"
    ));

    // A dispatched attempt is past the fence.
    fixture.ledger.mark_prepared_flushed(1).unwrap();
    fixture.ledger.dispatch(Some(fixture.permit)).unwrap();
    let dispatched = ModelAttemptLifecycleRecord::from_event(&fixture.ledger.events()[1]).unwrap();
    assert!(matches!(
        validate_model_attempt_dispatch(&dispatched, Some(fixture.permit), true),
        Err(ModelAttemptLifecycleProjectionError::Transition(ref reason))
            if reason == "model_attempt_dispatch_state_invalid"
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
        "model_attempt_permit_mismatch",
        "model_attempt_duplicate_settlement",
        "model_attempt_unknown_error_required",
        "model_attempt_dispatch_requires_prepared_flush",
        "model_attempt_event_identity_or_revision_conflict",
        "model_attempt_usage_identity_mismatch",
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
        "commit_transition",
        "append_event(",
        "#[ignore",
        "std::fs",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "BQ-11 lifecycle boundary gained effect authority: {forbidden}"
        );
    }
    assert!(!domain.contains("#[ignore"));
}

#[test]
fn bq11_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    let lane = include_str!("../../.github/workflows/bq11-attempt-lifecycle.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    let domain_fixtures = include_str!("../../kiana-domain/tests/bq11_attempt_lifecycle.rs");
    let core_fixtures = include_str!("./bq11_attempt_lifecycle_guard.rs");

    // The manual lane stays manual-only and keeps the bounded profile.
    assert!(lane.contains("workflow_dispatch:"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "BQ-11 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq11_attempt_lifecycle --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq11_attempt_lifecycle_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test bq11_attempt_lifecycle --no-run --locked",
        "cargo test -p kiana-core --test bq11_attempt_lifecycle_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "BQ-11 lane step missing: {step}");
    }
    for target in [
        "\"bq11_attempt_lifecycle\"",
        "\"bq11_attempt_lifecycle_guard\"",
    ] {
        assert!(
            shards.contains(target),
            "BQ-11 target missing from the automatic shard matrix: {target}"
        );
    }
    for fixture in [
        "lifecycle_links_run_turn_attempt_usage_and_receipt",
        "deny_unprepared_missing_permit_and_duplicate_settlement",
        "deny_flush_without_append_and_mismatched_permit",
        "deny_terminal_lifecycle_replay",
        "deny_usage_identity_unknown_confidence_and_settlement_mismatch",
        "unknown_attempt_retains_usage_and_error_for_reconciliation",
        "event_contract_refuses_unflushed_and_malformed_lifecycle_payloads",
    ] {
        assert!(
            domain_fixtures.contains(fixture),
            "BQ-11 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "projection_rebuilds_only_prepared_flushed_lifecycle",
        "projection_rejects_dispatch_without_prepared_fact",
        "projection_rejects_unflushed_schema_less_and_duplicate_facts",
        "projection_rejects_duplicate_settlement_revision_and_usage_identity",
        "projection_folds_unknown_attempts_and_ignores_legacy_prepared",
        "dispatch_fence_requires_matching_permit_and_flushed_prepared_record",
        "source_guard_keeps_lifecycle_projection_read_only",
    ] {
        assert!(
            core_fixtures.contains(fixture),
            "BQ-11 Core fixture missing: {fixture}"
        );
    }
}
