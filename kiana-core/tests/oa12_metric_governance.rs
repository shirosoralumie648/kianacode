//! OA-12 metric catalog/reducer/cardinality governance fixtures.
//!
//! The order of this file is the order of the card's argument: a raw identifier, path, prompt or
//! secret cannot become a label; cardinality overflow is refused and counted instead of swallowed;
//! an estimate cannot be reported as a measurement; a counter reset cannot fake success; and only
//! then do the replay and incremental (live) reducers have to agree on the same cursor with stable
//! kinds, units, label allowlist and catalog version.

use kiana_core::{
    project_operational_metrics, MetricCardinalityError, MetricCardinalityGuard, MetricReducer,
    MetricReducerError,
};
use kiana_domain::{
    EventId, MetricCatalog, MetricDefinition, MetricKind, MetricPoint, MetricQuality,
    MetricSnapshot, MetricSource, MetricStability, RequestId, RuntimeEvent,
};
use serde_json::json;
use std::collections::BTreeMap;

const GOVERNED_METRIC: &str = "kiana.test.governed_total";
const SERIES_METRIC: &str = "kiana.test.series_total";
const REGISTERED_METRIC: &str = "kiana.test.registered_total";
const DERIVED_METRIC: &str = "kiana.test.work_derived_total";
const MEASURED_METRIC: &str = "kiana.test.work_measured_total";
const OBSERVED_METRIC: &str = "kiana.test.work_observed_total";
const COMMIT_TOTAL_METRIC: &str = "kiana.eventlog.commit_total";
const OVERFLOW_METRIC: &str = kiana_core::METRIC_CARDINALITY_OVERFLOW_TOTAL;
const LABEL_NOT_REGISTERED: &str = "metric_label_not_registered";
const DEFINITION_MISMATCH: &str = "metric_definition_mismatch";
const ESTIMATE_AS_MEASURED: &str = "metric_estimate_reported_as_measured";
const ESTIMATE_SOURCE_CONFLICT: &str = "metric_estimate_source_conflict";
const ESTIMATE_DEFINITION_CONFLICT: &str = "metric_estimated_definition_source_conflict";
const MEASURED_DEFINITION_CONFLICT: &str = "metric_measured_definition_source_conflict";
const CATALOG_DIGEST_MISMATCH: &str = "metric_catalog_digest_mismatch";
const PROJECTOR_CURSOR_AHEAD: &str = "metric_projector_cursor_ahead";

fn source_ids() -> Vec<EventId> {
    vec![EventId::new()]
}

fn point(
    name: &str,
    kind: MetricKind,
    value: f64,
    unit: &str,
    labels: BTreeMap<String, String>,
    source: MetricSource,
    cursor: u64,
) -> MetricPoint {
    MetricPoint::new(
        name,
        kind,
        value,
        unit,
        labels,
        source,
        cursor,
        source_ids(),
    )
    .unwrap()
}

fn counter_point(name: &str, value: f64, cursor: u64) -> MetricPoint {
    point(
        name,
        MetricKind::Counter,
        value,
        "count",
        BTreeMap::new(),
        MetricSource::EventReducer,
        cursor,
    )
}

fn labelled_point(name: &str, label: &str, value: &str, cursor: u64) -> MetricPoint {
    let mut labels = BTreeMap::new();
    labels.insert(label.to_owned(), value.to_owned());
    point(
        name,
        MetricKind::Counter,
        1.0,
        "count",
        labels,
        MetricSource::EventReducer,
        cursor,
    )
}

fn series_point(component: &str, shard: &str, replica: u8, cursor: u64) -> MetricPoint {
    let mut labels = BTreeMap::new();
    labels.insert("component".to_owned(), component.to_owned());
    labels.insert("shard".to_owned(), shard.to_owned());
    labels.insert("replica".to_owned(), format!("replica-{replica}"));
    point(
        SERIES_METRIC,
        MetricKind::Counter,
        1.0,
        "count",
        labels,
        MetricSource::EventReducer,
        cursor,
    )
}

/// The label a forbidden-key refusal names, or a panic when the refusal has another shape.
fn forbidden_label(error: MetricCardinalityError) -> String {
    match error {
        MetricCardinalityError::ForbiddenLabel(label) => label,
        other => panic!("expected a forbidden label refusal, got {other:?}"),
    }
}

/// The label a forbidden-value refusal names, or a panic when the refusal has another shape.
fn invalid_label_value(error: MetricCardinalityError) -> String {
    match error {
        MetricCardinalityError::LabelValueInvalid(label) => label,
        other => panic!("expected a label value refusal, got {other:?}"),
    }
}

fn quality_point(
    name: &str,
    quality: MetricQuality,
    source: MetricSource,
    cursor: u64,
) -> MetricPoint {
    MetricPoint::new_with_quality(
        name,
        MetricKind::Counter,
        1.0,
        "count",
        quality,
        BTreeMap::new(),
        source,
        cursor,
        source_ids(),
    )
    .unwrap()
}

fn counter_catalog(name: &str, allowed_labels: &[&str]) -> MetricCatalog {
    let mut definition = MetricDefinition::counter(name, "count");
    for label in allowed_labels {
        definition.allowed_labels.push((*label).to_owned());
    }
    MetricCatalog::new(vec![definition]).expect("valid test catalog")
}

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

/// The full fact slice a live observer would be fed: two events at two cursors.
fn governed_events() -> Vec<RuntimeEvent> {
    vec![
        event(
            1,
            "eventlog.commit",
            json!({"source_cursor":1,"projector_cursor":1,"commit_id":"commit-1","append_latency_ms":4,"flush_latency_ms":2}),
        ),
        event(
            2,
            "receipt.query",
            json!({"source_cursor":2,"projector_cursor":2,"elapsed_ms":3,"success":true}),
        ),
    ]
}

/// Points keyed by identity, so two snapshots compare without depending on point order.
fn points_by_name(snapshot: &MetricSnapshot) -> BTreeMap<String, (MetricKind, String, f64)> {
    let mut points = BTreeMap::new();
    for point in &snapshot.points {
        let identity = (point.kind, point.unit.clone(), point.value);
        points.insert(point.name.clone(), identity);
    }
    points
}

#[test]
fn builtin_catalog_declares_typed_operational_metrics_and_stable_digest() {
    let first = MetricCatalog::builtin();
    let second = MetricCatalog::builtin();
    assert_eq!(first.digest(), second.digest());
    assert_eq!(
        first
            .metric("kiana.eventlog.append_latency_ms")
            .unwrap()
            .kind,
        MetricKind::Histogram
    );
    assert_eq!(
        first.metric("kiana.eventlog.durable_cursor").unwrap().unit,
        "events"
    );
    assert_eq!(
        first.metric(OVERFLOW_METRIC).unwrap().kind,
        MetricKind::Counter
    );
}

#[test]
fn builtin_catalog_kinds_units_labels_and_version_are_stable() {
    let catalog = MetricCatalog::builtin();
    assert_eq!(catalog.schema, kiana_domain::METRIC_CATALOG_SCHEMA);
    assert_eq!(catalog.version, kiana_domain::METRIC_CATALOG_SCHEMA_VERSION);
    for (name, kind, unit) in [
        ("kiana.eventlog.commit_total", MetricKind::Counter, "count"),
        (
            "kiana.eventlog.append_latency_ms",
            MetricKind::Histogram,
            "ms",
        ),
        (
            "kiana.eventlog.flush_latency_ms",
            MetricKind::Histogram,
            "ms",
        ),
        ("kiana.eventlog.durable_cursor", MetricKind::Gauge, "events"),
        ("kiana.projector.lag_events", MetricKind::Gauge, "events"),
        ("kiana.artifacts.bytes_total", MetricKind::Gauge, "bytes"),
        (
            "kiana.recovery.last_error_present",
            MetricKind::Gauge,
            "bool",
        ),
        (OVERFLOW_METRIC, MetricKind::Counter, "count"),
    ] {
        let definition = catalog.metric(name).expect("registered builtin metric");
        assert_eq!(definition.kind, kind, "{name} kind drifted");
        assert_eq!(definition.unit, unit, "{name} unit drifted");
        assert_eq!(definition.stability, MetricStability::Stable);
        assert_eq!(definition.source, MetricSource::EventReducer);
        assert!(
            definition.allowed_labels.is_empty(),
            "{name} gained a label"
        );
    }

    // Bound to the same digest across instances and across serialization, so another process can
    // compare a snapshot without trusting its producer.
    let encoded = serde_json::to_string(&catalog).unwrap();
    let decoded: MetricCatalog = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.digest(), catalog.digest());
    assert_eq!(decoded.metrics, catalog.metrics);
}

#[test]
fn remote_catalog_is_digest_bound_and_unknown_fields_fail_closed() {
    let catalog = MetricCatalog::builtin();
    let encoded = serde_json::to_value(&catalog).unwrap();

    // A catalog whose content disagrees with its digest is refused rather than trusted.
    let mut tampered = encoded.clone();
    tampered["metrics"][0]["unit"] = json!("seconds");
    let remote: MetricCatalog = serde_json::from_value(tampered).unwrap();
    assert_eq!(remote.validate().unwrap_err(), CATALOG_DIGEST_MISMATCH);

    // Unknown fields fail closed instead of being silently ignored.
    let mut extra = encoded;
    extra["unexpected_field"] = json!(true);
    assert!(serde_json::from_value::<MetricCatalog>(extra).is_err());
}

#[test]
fn guard_rejects_path_prompt_and_secret_labels_and_values() {
    let catalog = counter_catalog(
        GOVERNED_METRIC,
        &["component", "file_path", "prompt_text", "raw_response"],
    );
    let mut guard = MetricCardinalityGuard::new(catalog).unwrap();

    for key in ["file_path", "prompt_text", "raw_response"] {
        let candidate = labelled_point(GOVERNED_METRIC, key, "bounded", 1);
        let error = guard.observe(&candidate).unwrap_err();
        assert_eq!(forbidden_label(error), key);
    }
    assert_eq!(guard.overflow_total(), 3);

    let raw_id = EventId::new().to_string();
    let digest_like = format!("sha256:{}", "a".repeat(64));
    let raw_values = [
        "/var/lib/kiana/journal.jsonl".to_owned(),
        "~/kiana/state/journal.jsonl".to_owned(),
        "C:\\kiana\\journal.jsonl".to_owned(),
        "api_key=sk-live-abcd1234".to_owned(),
        "{\"prompt\": \"explain the diff\"}".to_owned(),
        digest_like,
        raw_id,
    ];
    for value in raw_values {
        let candidate = labelled_point(GOVERNED_METRIC, "component", &value, 2);
        let error = guard.observe(&candidate).unwrap_err();
        assert_eq!(invalid_label_value(error), "component");
    }
    assert_eq!(guard.overflow_total(), 10);

    // Overflow is published, not swallowed: the counter is a first-class registered catalog metric.
    let overflow = guard.overflow_point().unwrap().expect("observed boundary");
    assert_eq!(overflow.name, OVERFLOW_METRIC);
    assert_eq!(overflow.kind, MetricKind::Counter);
    assert_eq!(overflow.unit, "count");
    assert_eq!(overflow.value, 10.0);
    assert_eq!(overflow.source_cursor, 2);
    assert!(overflow.labels.is_empty());
    let mut fresh = MetricCardinalityGuard::new(MetricCatalog::builtin()).unwrap();
    fresh.observe(&overflow).unwrap();
}

#[test]
fn guard_counts_series_overflow_at_the_bound_and_never_silently_drops_it() {
    let catalog = counter_catalog(SERIES_METRIC, &["component", "shard", "replica"]);
    let mut guard = MetricCardinalityGuard::new(catalog).unwrap();
    let mut cursor = 0_u64;
    for component in 0..kiana_domain::MAX_METRIC_LABEL_VALUES {
        for shard in 0..16 {
            cursor += 1;
            let component_label = format!("component-{component}");
            let shard_label = format!("shard-{shard}");
            guard
                .observe(&series_point(&component_label, &shard_label, 0, cursor))
                .unwrap();
        }
    }
    assert_eq!(guard.series_count(), kiana_domain::MAX_METRIC_SERIES);
    assert_eq!(guard.overflow_total(), 0);

    // One more series beyond the bound: every label value is still within its own budget, so the
    // refusal is the series bound rather than a hidden value limit.
    cursor += 1;
    let candidate = series_point("component-0", "shard-0", 1, cursor);
    let error = guard.observe(&candidate).unwrap_err();
    assert_eq!(error.to_string(), "metric_cardinality_series_limit");
    assert_eq!(guard.series_count(), kiana_domain::MAX_METRIC_SERIES);
    assert_eq!(guard.overflow_total(), 1);

    let overflow = guard.overflow_point().unwrap().expect("observed boundary");
    assert_eq!(overflow.value, 1.0);
    assert_eq!(overflow.source_cursor, cursor);
}

#[test]
fn guard_refuses_unregistered_labels_and_kind_unit_or_source_drift() {
    let catalog = counter_catalog(REGISTERED_METRIC, &["component", "trace_id"]);
    let mut guard = MetricCardinalityGuard::new(catalog).unwrap();

    let candidate = labelled_point(REGISTERED_METRIC, "unregistered_dimension", "bounded", 1);
    let error = guard.observe(&candidate).unwrap_err();
    assert!(error.to_string().ends_with(LABEL_NOT_REGISTERED));

    // A catalog cannot allowlist an identifier: the guard's label rule refuses it even when the
    // declaration admits it.
    let candidate = labelled_point(REGISTERED_METRIC, "trace_id", "bounded", 1);
    let error = guard.observe(&candidate).unwrap_err();
    assert_eq!(forbidden_label(error), "trace_id");

    let unit_drift = point(
        REGISTERED_METRIC,
        MetricKind::Counter,
        1.0,
        "seconds",
        BTreeMap::new(),
        MetricSource::EventReducer,
        1,
    );
    let error = guard.observe(&unit_drift).unwrap_err();
    assert!(error.to_string().ends_with(DEFINITION_MISMATCH));

    let kind_drift = point(
        REGISTERED_METRIC,
        MetricKind::Gauge,
        1.0,
        "count",
        BTreeMap::new(),
        MetricSource::EventReducer,
        1,
    );
    let error = guard.observe(&kind_drift).unwrap_err();
    assert!(error.to_string().ends_with(DEFINITION_MISMATCH));

    let source_drift = point(
        REGISTERED_METRIC,
        MetricKind::Counter,
        1.0,
        "count",
        BTreeMap::new(),
        MetricSource::RuntimeGauge,
        1,
    );
    let error = guard.observe(&source_drift).unwrap_err();
    assert!(error.to_string().ends_with(DEFINITION_MISMATCH));

    // Shape refusals are not cardinality overflow; only the identifier label was counted.
    assert_eq!(guard.overflow_total(), 1);
}

#[test]
fn estimated_and_measured_metrics_cannot_cross_quality_boundaries() {
    let mut estimated = MetricDefinition::counter("kiana.test.work_estimated_total", "count");
    estimated.source = MetricSource::Derived;
    let catalog = MetricCatalog::new(vec![estimated]).unwrap();
    let estimated_point = MetricPoint::new_with_quality(
        "kiana.test.work_estimated_total",
        MetricKind::Counter,
        1.0,
        "count",
        MetricQuality::Estimated,
        BTreeMap::new(),
        MetricSource::Derived,
        1,
        source_ids(),
    )
    .unwrap();
    estimated_point.validate_with_catalog(&catalog).unwrap();
    assert!(MetricPoint::new(
        "kiana.test.work_estimated_total",
        MetricKind::Counter,
        1.0,
        "count",
        BTreeMap::new(),
        MetricSource::Derived,
        1,
        source_ids(),
    )
    .is_err());
}

#[test]
fn derived_catalog_metrics_cannot_be_reported_as_measured() {
    // The `_estimated_total` suffix only covers names that carry it. The catalog source is the
    // binding that covers every derived metric name.
    let mut derived = MetricDefinition::counter(DERIVED_METRIC, "count");
    derived.source = MetricSource::Derived;
    let catalog = MetricCatalog::new(vec![derived]).unwrap();

    let measured = quality_point(
        DERIVED_METRIC,
        MetricQuality::Measured,
        MetricSource::Derived,
        1,
    );
    assert_eq!(
        measured.validate_with_catalog(&catalog).unwrap_err(),
        ESTIMATE_AS_MEASURED
    );
    let mut guard = MetricCardinalityGuard::new(catalog.clone()).unwrap();
    assert_eq!(
        guard.observe(&measured).unwrap_err().to_string(),
        "metric_cardinality_point_invalid:metric_estimate_reported_as_measured"
    );
    assert_eq!(guard.overflow_total(), 0);

    let estimated = quality_point(
        DERIVED_METRIC,
        MetricQuality::Estimated,
        MetricSource::Derived,
        2,
    );
    estimated.validate_with_catalog(&catalog).unwrap();
    guard.observe(&estimated).unwrap();

    // The other direction: an estimate on an event-reducer measurement is refused too. The name
    // carries no quality suffix, so only the catalog binding can produce this refusal.
    let measured_catalog = counter_catalog(OBSERVED_METRIC, &[]);
    let estimate_on_measured = quality_point(
        OBSERVED_METRIC,
        MetricQuality::Estimated,
        MetricSource::EventReducer,
        1,
    );
    let refusal = estimate_on_measured
        .validate_with_catalog(&measured_catalog)
        .unwrap_err();
    assert_eq!(refusal, ESTIMATE_SOURCE_CONFLICT);

    // A catalog cannot declare an estimated name on a measured source, or the reverse: the
    // definition is refused rather than points being adjusted to fit it.
    let inconsistent = MetricDefinition::counter("kiana.test.work_estimated_total", "count");
    assert_eq!(
        MetricCatalog::new(vec![inconsistent]).unwrap_err(),
        ESTIMATE_DEFINITION_CONFLICT
    );
    let mut measured_on_derived = MetricDefinition::counter(MEASURED_METRIC, "count");
    measured_on_derived.source = MetricSource::Derived;
    assert_eq!(
        MetricCatalog::new(vec![measured_on_derived]).unwrap_err(),
        MEASURED_DEFINITION_CONFLICT
    );
}

#[test]
fn guard_does_not_manufacture_an_overflow_point_without_a_boundary() {
    let guard = MetricCardinalityGuard::new(MetricCatalog::builtin()).unwrap();
    assert_eq!(guard.overflow_total(), 0);
    assert!(guard.overflow_point().unwrap().is_none());

    // A catalog without the registered overflow metric refuses to invent one.
    let catalog = counter_catalog("kiana.test.other_total", &[]);
    let mut custom = MetricCardinalityGuard::new(catalog).unwrap();
    let candidate = counter_point("kiana.test.other_total", 1.0, 1);
    custom.observe(&candidate).unwrap();
    assert!(matches!(
        custom.overflow_point(),
        Err(MetricCardinalityError::CatalogMissing(name)) if name == OVERFLOW_METRIC
    ));
}

#[test]
fn incremental_reducer_rejects_counter_reset_and_replay_matches_live_points() {
    let mut reducer = MetricReducer::builtin().unwrap();
    reducer
        .apply_point(counter_point(COMMIT_TOTAL_METRIC, 3.0, 1))
        .unwrap();
    assert!(matches!(
        reducer.apply_point(counter_point(COMMIT_TOTAL_METRIC, 2.0, 2)),
        Err(MetricReducerError::CounterReset(name)) if name == COMMIT_TOTAL_METRIC
    ));

    let events = governed_events();
    let replay = MetricReducer::replay(&events, Some(2)).unwrap();
    let snapshot = project_operational_metrics(&events, Some(2)).unwrap();
    let mut live = MetricReducer::builtin().unwrap();
    live.apply_snapshot(&snapshot).unwrap();
    assert_eq!(
        replay.snapshot(2).unwrap().points,
        live.snapshot(2).unwrap().points
    );
    assert_eq!(
        replay.snapshot(2).unwrap().catalog_digest,
        Some(MetricCatalog::builtin().digest())
    );
}

#[test]
fn live_incremental_reducer_matches_replay_at_the_same_cursor() {
    let events = governed_events();
    let first = project_operational_metrics(&events[..1], Some(1)).unwrap();
    let second = project_operational_metrics(&events, Some(2)).unwrap();

    let mut live = MetricReducer::builtin().unwrap();
    live.apply_snapshot(&first).unwrap();
    let at_first = live.snapshot(1).unwrap();
    assert_eq!(points_by_name(&at_first), points_by_name(&first));

    live.apply_snapshot(&second).unwrap();
    let live_at_two = live.snapshot(2).unwrap();
    let replay = MetricReducer::replay(&events, Some(2)).unwrap();
    let replay_at_two = replay.snapshot(2).unwrap();

    // Both reducers, fed the same facts at the same cursor, agree metric by metric.
    assert_eq!(points_by_name(&live_at_two), points_by_name(&replay_at_two));
    assert_eq!(points_by_name(&live_at_two), points_by_name(&second));
    assert_eq!(live_at_two.snapshot_digest, replay_at_two.snapshot_digest);
    assert_eq!(live_at_two.limitations, second.limitations);
    assert_eq!(
        live_at_two.catalog_digest,
        Some(MetricCatalog::builtin().digest())
    );
}

#[test]
fn reducer_rejects_counter_reset_and_cursor_regression_without_mutating_state() {
    let mut reducer = MetricReducer::builtin().unwrap();
    reducer
        .apply_point(counter_point(COMMIT_TOTAL_METRIC, 3.0, 4))
        .unwrap();
    assert!(reducer.overflow_point().unwrap().is_some());
    let before = reducer.snapshot(4).unwrap();

    assert!(matches!(
        reducer.apply_point(counter_point(COMMIT_TOTAL_METRIC, 2.0, 5)),
        Err(MetricReducerError::CounterReset(name)) if name == COMMIT_TOTAL_METRIC
    ));
    assert!(matches!(
        reducer.apply_point(counter_point(COMMIT_TOTAL_METRIC, 3.0, 3)),
        Err(MetricReducerError::SourceCursorRegressed(name)) if name == COMMIT_TOTAL_METRIC
    ));
    assert_eq!(
        reducer.snapshot(4).unwrap().snapshot_digest,
        before.snapshot_digest
    );

    // A batch is applied atomically: one refused point cannot leave an earlier point attributed.
    let batch = vec![
        counter_point(COMMIT_TOTAL_METRIC, 4.0, 6),
        counter_point(COMMIT_TOTAL_METRIC, 1.0, 7),
    ];
    assert!(matches!(
        reducer.apply_points(&batch),
        Err(MetricReducerError::CounterReset(_))
    ));
    let after = reducer.snapshot(4).unwrap();
    assert_eq!(after.snapshot_digest, before.snapshot_digest);
    assert_eq!(after.points, before.points);

    // A projector cursor ahead of the source cursor is refused instead of clamped.
    assert!(matches!(
        reducer.snapshot(9),
        Err(MetricReducerError::SnapshotInvalid(message)) if message == PROJECTOR_CURSOR_AHEAD
    ));
}

#[test]
fn reducer_refuses_a_snapshot_bound_to_a_foreign_catalog_digest() {
    let events = governed_events();
    let snapshot = project_operational_metrics(&events, Some(2)).unwrap();
    let foreign_catalog = counter_catalog("kiana.test.foreign_total", &[]);
    let mut foreign = MetricReducer::new(foreign_catalog).unwrap();
    assert!(matches!(
        foreign.apply_snapshot(&snapshot),
        Err(MetricReducerError::SnapshotInvalid(message)) if message == CATALOG_DIGEST_MISMATCH
    ));
    assert_eq!(foreign.overflow_total(), 0);

    let mut builtin = MetricReducer::builtin().unwrap();
    builtin.apply_snapshot(&snapshot).unwrap();
    assert_eq!(
        builtin.snapshot(2).unwrap().catalog_digest,
        Some(MetricCatalog::builtin().digest())
    );
}
