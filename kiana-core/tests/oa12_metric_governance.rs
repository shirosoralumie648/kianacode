use kiana_core::{
    project_metrics, MetricCardinalityError, MetricCardinalityGuard, MetricReducer,
    MetricReducerError,
};
use kiana_domain::{
    EventId, MetricCatalog, MetricDefinition, MetricKind, MetricPoint, MetricQuality, MetricSource,
    RequestId, RuntimeEvent,
};
use serde_json::json;
use std::collections::BTreeMap;

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

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
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
        first
            .metric("kiana.metrics.cardinality_overflow_total")
            .unwrap()
            .kind,
        MetricKind::Counter
    );
}

#[test]
fn cardinality_guard_rejects_sensitive_labels_and_bounds_series_values() {
    let mut definition = MetricDefinition::counter("kiana.test.series_total", "count");
    definition.allowed_labels = vec!["component".to_owned(), "request_id".to_owned()];
    let catalog = MetricCatalog::new(vec![definition]).unwrap();
    let mut guard = MetricCardinalityGuard::new(catalog).unwrap();
    let mut sensitive = BTreeMap::new();
    sensitive.insert("request_id".to_owned(), "request-raw".to_owned());
    assert!(matches!(
        guard.observe(&point(
            "kiana.test.series_total",
            MetricKind::Counter,
            1.0,
            "count",
            sensitive,
            MetricSource::EventReducer,
            1,
        )),
        Err(MetricCardinalityError::ForbiddenLabel(label)) if label == "request_id"
    ));

    let mut definition = MetricDefinition::counter("kiana.test.values_total", "count");
    definition.allowed_labels = vec!["component".to_owned()];
    let catalog = MetricCatalog::new(vec![definition]).unwrap();
    let mut guard = MetricCardinalityGuard::new(catalog).unwrap();
    for value in 0..kiana_domain::MAX_METRIC_LABEL_VALUES {
        let mut labels = BTreeMap::new();
        labels.insert("component".to_owned(), format!("component-{value}"));
        guard
            .observe(&point(
                "kiana.test.values_total",
                MetricKind::Counter,
                value as f64,
                "count",
                labels,
                MetricSource::EventReducer,
                value as u64 + 1,
            ))
            .unwrap();
    }
    let mut overflow_labels = BTreeMap::new();
    overflow_labels.insert("component".to_owned(), "component-overflow".to_owned());
    assert!(matches!(
        guard.observe(&point(
            "kiana.test.values_total",
            MetricKind::Counter,
            65.0,
            "count",
            overflow_labels,
            MetricSource::EventReducer,
            66,
        )),
        Err(MetricCardinalityError::LabelValueLimit(label)) if label == "component"
    ));
    assert_eq!(guard.overflow_total(), 2);
}

#[test]
fn incremental_reducer_rejects_counter_reset_and_replay_matches_live_points() {
    let mut reducer = MetricReducer::builtin().unwrap();
    reducer
        .apply_point(point(
            "kiana.eventlog.commit_total",
            MetricKind::Counter,
            3.0,
            "count",
            BTreeMap::new(),
            MetricSource::EventReducer,
            1,
        ))
        .unwrap();
    assert!(matches!(
        reducer.apply_point(point(
            "kiana.eventlog.commit_total",
            MetricKind::Counter,
            2.0,
            "count",
            BTreeMap::new(),
            MetricSource::EventReducer,
            2,
        )),
        Err(MetricReducerError::CounterReset(name)) if name == "kiana.eventlog.commit_total"
    ));

    let events = vec![
        event(
            1,
            "eventlog.commit",
            json!({"source_cursor":1,"projector_cursor":1,"commit_id":"commit-1","append_latency_ms":4,"flush_latency_ms":2}),
        ),
        event(
            2,
            "receipt.query",
            json!({"source_cursor":2,"elapsed_ms":3,"success":true}),
        ),
    ];
    let replay = MetricReducer::replay(&events, Some(2)).unwrap();
    let snapshot = project_metrics(&events).unwrap();
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
fn estimated_and_measured_metrics_cannot_cross_quality_boundaries() {
    let mut estimated = MetricDefinition::counter("kiana.test.estimated_total", "count");
    estimated.source = MetricSource::Derived;
    let catalog = MetricCatalog::new(vec![estimated]).unwrap();
    let estimated_point = MetricPoint::new_with_quality(
        "kiana.test.estimated_total",
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
        "kiana.test.estimated_total",
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
