use kiana_core::{project_health_snapshot, project_operational_metrics};
use kiana_domain::{
    ComponentHealthState, EventStoreCapabilities, HealthProbeKind, HealthSnapshot, RuntimeEvent,
    SignalStatus,
};
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

fn healthy_facts() -> Vec<RuntimeEvent> {
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
                "result": {"success": true},
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
fn empty_health_source_fails_closed_without_ready_or_live_claim() {
    let error =
        project_health_snapshot(&[], &durable_capabilities(), HealthProbeKind::Readiness, 1)
            .unwrap_err();
    assert_eq!(error.to_string(), "health_source_empty");
}

#[test]
fn readiness_rejects_unknown_effect_and_missing_durable_projector_checkpoint() {
    let mut events = healthy_facts();
    events.push(event(
        5,
        "execution.result_committed",
        json!({
            "source_cursor": 5,
            "execution_id": "execution-2",
            "effect_known": false,
            "error": "result_unknown:timeout",
            "result": {"success": false},
        }),
    ));
    let snapshot = project_health_snapshot(
        &events,
        &durable_capabilities(),
        HealthProbeKind::Readiness,
        42,
    )
    .unwrap();
    assert_eq!(snapshot.status, SignalStatus::Degraded);
    assert_eq!(snapshot.probe, HealthProbeKind::Readiness);
    assert_eq!(
        snapshot.components["projector"].state,
        ComponentHealthState::Degraded
    );
    assert_eq!(
        snapshot.components["recovery"].state,
        ComponentHealthState::Degraded
    );
    assert!(snapshot
        .limitations
        .iter()
        .any(|limitation| limitation == "effect_unknown_present"));
    snapshot.validate().unwrap();
}

#[test]
fn liveness_is_read_only_and_component_capabilities_are_bounded() {
    let events = healthy_facts();
    let memory_capabilities = EventStoreCapabilities::default();
    let snapshot =
        project_health_snapshot(&events, &memory_capabilities, HealthProbeKind::Liveness, 99)
            .unwrap();
    assert_eq!(snapshot.status, SignalStatus::Ok);
    assert_eq!(snapshot.probe, HealthProbeKind::Liveness);
    assert_eq!(
        snapshot.components["eventlog"].state,
        ComponentHealthState::Unavailable
    );
    assert_eq!(
        snapshot.components["provider"].state,
        ComponentHealthState::Unknown
    );
    assert!(!snapshot.capabilities["eventlog.durable_commits"]);
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains("prompt"));
    assert_eq!(
        serde_json::from_str::<HealthSnapshot>(&encoded).unwrap(),
        snapshot
    );
    let metrics = project_operational_metrics(&events, Some(4)).unwrap();
    assert_eq!(metrics.source_cursor, snapshot.source_cursor);
}
