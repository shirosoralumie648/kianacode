use kiana_core::{project_metrics, project_operational_metrics};
use kiana_domain::{MetricSnapshot, RuntimeEvent, SignalStatus};
use serde_json::{json, Value};

fn event(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(kiana_domain::RequestId::new(), sequence, kind, data).unwrap()
}

fn metric_value(snapshot: &MetricSnapshot, name: &str) -> f64 {
    snapshot
        .points
        .iter()
        .find(|point| point.name == name)
        .map(|point| point.value)
        .expect("registered metric point")
}

#[test]
fn empty_event_source_is_unavailable_not_healthy_zero_work() {
    let error = project_metrics(&[]).unwrap_err();
    assert_eq!(error.to_string(), "metrics_source_empty");
}

#[test]
fn cursor_gap_orphan_and_unknown_effect_are_visible_and_degraded() {
    let events = vec![
        event(
            1,
            "invocation.dispatching",
            json!({
                "source_cursor": 1,
                "request_id": "request-1",
                "execution_id": "execution-1",
                "attempt": 1,
                "effect_started": true,
            }),
        ),
        event(
            2,
            "execution.result_committed",
            json!({
                "source_cursor": 3,
                "request_id": "request-2",
                "execution_id": "execution-2",
                "effect_known": false,
                "result": {"success": false},
                "error": "result_unknown:provider_timeout",
            }),
        ),
        event(
            3,
            "artifact.read",
            json!({
                "source_cursor": 4,
                "artifact_id": "artifact-1",
                "error": "artifact_not_found",
            }),
        ),
    ];
    let snapshot = project_operational_metrics(&events, Some(1)).unwrap();
    assert_eq!(snapshot.status, SignalStatus::Degraded);
    assert_eq!(snapshot.source_cursor, 4);
    assert_eq!(snapshot.projector_cursor, 1);
    assert_eq!(metric_value(&snapshot, "kiana.projector.lag_events"), 3.0);
    assert_eq!(metric_value(&snapshot, "kiana.recovery.orphan_total"), 1.0);
    assert_eq!(metric_value(&snapshot, "kiana.recovery.unknown_total"), 1.0);
    assert_eq!(
        metric_value(&snapshot, "kiana.artifacts.read_failure_total"),
        1.0
    );
    assert!(snapshot
        .limitations
        .iter()
        .any(|limitation| limitation == "eventlog_cursor_gap"));
    snapshot.validate().unwrap();
}

#[test]
fn complete_committed_metrics_snapshot_is_digest_bound_and_secret_free() {
    let events = vec![
        event(
            1,
            "eventlog.commit",
            json!({
                "source_cursor": 1,
                "projector_cursor": 1,
                "commit_id": "commit-1",
                "append_latency_ms": 12,
                "flush_latency_ms": 4,
            }),
        ),
        event(
            2,
            "invocation.dispatching",
            json!({
                "source_cursor": 2,
                "request_id": "request-1",
                "execution_id": "execution-1",
                "attempt": 1,
                "effect_started": true,
            }),
        ),
        event(
            3,
            "execution.result_committed",
            json!({
                "source_cursor": 3,
                "request_id": "request-1",
                "execution_id": "execution-1",
                "effect_known": true,
                "result": {"success": true, "output": "raw-secret-must-not-cross-metric"},
            }),
        ),
        event(
            4,
            "receipt.query",
            json!({"source_cursor": 4, "elapsed_ms": 8, "success": true}),
        ),
        event(
            5,
            "artifact.written",
            json!({"source_cursor": 5, "artifact_id": "artifact-1", "artifact_bytes": 21}),
        ),
    ];
    let snapshot = project_operational_metrics(&events, Some(5)).unwrap();
    assert_eq!(snapshot.status, SignalStatus::Ok);
    assert_eq!(snapshot.source_cursor, 5);
    assert_eq!(
        metric_value(&snapshot, "kiana.eventlog.append_latency_ms"),
        12.0
    );
    assert_eq!(
        metric_value(&snapshot, "kiana.eventlog.flush_latency_ms"),
        4.0
    );
    assert_eq!(
        metric_value(&snapshot, "kiana.receipts.query_latency_ms"),
        8.0
    );
    assert_eq!(metric_value(&snapshot, "kiana.artifacts.bytes_total"), 21.0);
    assert_eq!(metric_value(&snapshot, "kiana.recovery.orphan_total"), 0.0);
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(!serialized.contains("raw-secret-must-not-cross-metric"));
    snapshot.validate().unwrap();
}
