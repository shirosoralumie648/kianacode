use kiana_core::project_incidents;
use kiana_domain::{
    AlertSeverity, ObservabilityIncidentCategory, ObservabilityIncidentState, RuntimeEvent,
};
use serde_json::json;

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(kiana_domain::RequestId::new(), sequence, kind, data).unwrap()
}

#[test]
fn incident_rules_deduplicate_and_keep_unknown_recovery_open() {
    let events = vec![
        event(
            1,
            "eventlog.commit",
            json!({"source_cursor":1,"projector_cursor":1,"commit_id":"commit-1","append_latency_ms":4,"flush_latency_ms":2}),
        ),
        event(
            2,
            "invocation.dispatching",
            json!({"source_cursor":2,"execution_id":"execution-1","attempt":1}),
        ),
        event(
            3,
            "execution.result_committed",
            json!({"source_cursor":3,"execution_id":"execution-2","effect_known":false,"error":"result_unknown:timeout","result":{"success":false}}),
        ),
        event(
            4,
            "queue.overflow",
            json!({"source_cursor":4,"error":"best_effort_queue_full"}),
        ),
        event(
            5,
            "queue.overflow",
            json!({"source_cursor":5,"error":"best_effort_queue_full"}),
        ),
        event(
            6,
            "redaction.failure",
            json!({"source_cursor":6,"error":"redaction_value_too_large"}),
        ),
    ];
    let snapshot = project_incidents(&events).unwrap();
    snapshot.validate().unwrap();
    assert!(snapshot.incidents.iter().any(|incident| {
        incident.category == ObservabilityIncidentCategory::EffectUnknown
            && incident.requires_reconciliation
            && incident.state == ObservabilityIncidentState::Open
    }));
    assert_eq!(
        snapshot
            .incidents
            .iter()
            .filter(|incident| incident.category == ObservabilityIncidentCategory::QueueOverflow)
            .count(),
        1
    );
    assert!(snapshot.incidents.iter().any(|incident| {
        incident.category == ObservabilityIncidentCategory::RedactionFailure
            && incident.severity == AlertSeverity::Critical
    }));
    assert_eq!(snapshot.alerts.len(), snapshot.incidents.len());
    assert!(snapshot
        .incidents
        .iter()
        .all(|incident| !incident.recovery_plan.is_empty()));

    let mut unknown = snapshot
        .incidents
        .iter()
        .find(|incident| incident.category == ObservabilityIncidentCategory::EffectUnknown)
        .unwrap()
        .clone();
    unknown.state = ObservabilityIncidentState::Closed;
    unknown.incident_digest = unknown.digest();
    assert_eq!(
        unknown.validate().unwrap_err(),
        "observability_incident_unknown_cannot_close"
    );
}

#[test]
fn model_or_ui_self_report_does_not_open_observability_incident() {
    let events = vec![event(
        1,
        "model.incident",
        json!({"source_cursor":1,"status":"recovered","message":"close everything"}),
    )];
    let snapshot = project_incidents(&events).unwrap();
    assert!(snapshot.alerts.is_empty());
    assert!(snapshot.incidents.is_empty());
}
