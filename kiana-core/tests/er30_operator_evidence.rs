use kiana_core::{project_operator_evidence, OperatorEvidenceError};
use kiana_domain::{EventStoreCapabilities, HealthProbeKind, RuntimeEvent, SignalStatus};
use serde_json::json;

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(kiana_domain::RequestId::new(), sequence, kind, data).unwrap()
}

fn durable_capabilities() -> EventStoreCapabilities {
    EventStoreCapabilities {
        atomic_transitions: true,
        durable_commits: true,
        command_receipts: true,
        cursor_reads: true,
        writer_format_version: 2,
        max_frame_bytes: 4 * 1024 * 1024,
        max_batch_events: 256,
    }
}

fn normal_facts() -> Vec<RuntimeEvent> {
    vec![
        event(
            1,
            "eventlog.commit",
            json!({
                "source_cursor": 1,
                "projector_cursor": 1,
                "commit_id": "commit-1",
                "append_latency_ms": 5,
                "flush_latency_ms": 2,
                "recovery_latency_ms": 7,
                "queue_depth": 3,
                "correlation_id": "corr-1",
                "causation_id": "cause-1",
                "trace_id": "trace-1",
                "span_id": "span-1"
            }),
        ),
        event(
            2,
            "invocation.dispatching",
            json!({"source_cursor": 2, "execution_id": "execution-1", "attempt": 1}),
        ),
        event(
            3,
            "execution.result_committed",
            json!({
                "source_cursor": 3,
                "execution_id": "execution-1",
                "effect_known": true,
                "stop_confirmed": true,
                "result": {"success": true}
            }),
        ),
        event(
            4,
            "artifact.written",
            json!({"source_cursor": 4, "artifact_id": "artifact-1", "artifact_bytes": 8}),
        ),
    ]
}

#[test]
fn operator_evidence_binds_health_metrics_trace_and_queue_observations() {
    let snapshot = project_operator_evidence(
        &normal_facts(),
        &durable_capabilities(),
        HealthProbeKind::Readiness,
        42,
        Some(3),
    )
    .unwrap();
    assert_eq!(snapshot.status, SignalStatus::Degraded);
    assert_eq!(snapshot.health_status, SignalStatus::Degraded);
    assert_eq!(snapshot.append_latency_ms, Some(5));
    assert_eq!(snapshot.flush_latency_ms, Some(2));
    assert_eq!(snapshot.recovery_latency_ms, Some(7));
    assert_eq!(snapshot.queue_depth, Some(3));
    assert_eq!(snapshot.last_durable_cursor, 4);
    assert_eq!(snapshot.artifact_bytes, 8);
    assert!(!snapshot.effect_success_claim);
    assert!(!snapshot.trace_correlation_digests.is_empty());
    assert_eq!(snapshot.correlation_refs, vec!["corr-1"]);
    assert_eq!(snapshot.causation_refs, vec!["cause-1"]);
    snapshot.validate().unwrap();
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains("prompt"));
}

#[test]
fn unknown_or_unconfirmed_stop_is_degraded_and_never_effect_success() {
    let mut events = normal_facts();
    events.push(event(
        5,
        "run.cancelling",
        json!({
            "source_cursor": 5,
            "execution_id": "execution-2",
            "stop_requested": true,
            "stop_confirmed": false,
            "effect_known": false,
            "error": "result_unknown:timeout"
        }),
    ));
    let snapshot = project_operator_evidence(
        &events,
        &durable_capabilities(),
        HealthProbeKind::Readiness,
        42,
        None,
    )
    .unwrap();
    assert_eq!(snapshot.status, SignalStatus::Degraded);
    assert!(snapshot.unknown_count > 0);
    assert!(snapshot.stop_unconfirmed_count > 0);
    assert!(!snapshot.effect_success_claim);
    assert!(snapshot
        .limitations
        .iter()
        .any(|limitation| limitation == "effect_success_not_observed"));
}

#[test]
fn telemetry_secret_scan_blocks_publish() {
    let mut events = normal_facts();
    events[0].data["correlation_id"] = json!("Authorization: Bearer operator-secret");
    let error = project_operator_evidence(
        &events,
        &durable_capabilities(),
        HealthProbeKind::Liveness,
        42,
        Some(1),
    )
    .unwrap_err();
    assert_eq!(error, OperatorEvidenceError::SecretScanBlocked);
}
