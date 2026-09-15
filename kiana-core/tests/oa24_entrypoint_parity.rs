use kiana_core::{project_entrypoint_parity, ParityProjectionError};
use kiana_domain::{EntryPointKind, ExecutionStatus, RequestId, RunId, RuntimeEvent};
use serde_json::json;

fn event(
    run_id: RunId,
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    data: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn completed_facts() -> (RunId, Vec<RuntimeEvent>) {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({
                "run_id": run_id,
                "session_id": "session-1",
                "actor_id": "local-user",
                "project_root": "/repo",
                "authority_epoch": 1,
                "data_epoch": 1
            }),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.completed",
            json!({
                "run_id": run_id,
                "authority_epoch": 1,
                "data_epoch": 1,
                "result": "ok"
            }),
        ),
    ];
    (run_id, events)
}

#[test]
fn all_entrypoints_project_the_same_committed_facts() {
    let (run_id, events) = completed_facts();
    let snapshots = [
        EntryPointKind::Cli,
        EntryPointKind::Web,
        EntryPointKind::Workbench,
        EntryPointKind::Desktop,
    ]
    .into_iter()
    .map(|entrypoint| project_entrypoint_parity(entrypoint, Some(run_id), &events).unwrap())
    .collect::<Vec<_>>();

    let first = &snapshots[0];
    assert_eq!(first.status, ExecutionStatus::Completed);
    assert!(first.receipt_digest.is_some());
    assert!(first.audit_digest.is_some());
    assert!(first.health_digest.is_some());
    for snapshot in &snapshots[1..] {
        assert_eq!(snapshot.source_cursor, first.source_cursor);
        assert_eq!(snapshot.source_event_ids, first.source_event_ids);
        assert_eq!(snapshot.projection_version, first.projection_version);
        assert_eq!(snapshot.status, first.status);
        assert_eq!(snapshot.receipt_digest, first.receipt_digest);
        assert_eq!(snapshot.audit_digest, first.audit_digest);
        assert_eq!(snapshot.health_digest, first.health_digest);
        assert_eq!(snapshot.limitations, first.limitations);
        assert_ne!(snapshot.entrypoint, first.entrypoint);
    }
}

#[test]
fn terminal_conflict_and_source_gap_remain_unknown_or_rejected() {
    let (run_id, mut events) = completed_facts();
    events.push(event(
        run_id,
        RequestId::new(),
        3,
        "run.failed",
        json!({"run_id":run_id,"authority_epoch":1,"data_epoch":1,"error":"conflict"}),
    ));
    let snapshot = project_entrypoint_parity(EntryPointKind::Cli, Some(run_id), &events).unwrap();
    assert_eq!(snapshot.status, ExecutionStatus::ResultUnknown);
    assert!(snapshot.receipt_digest.is_none());
    assert!(snapshot
        .limitations
        .iter()
        .any(|limitation| limitation == "receipt_projection_unavailable"));

    let (run_id, mut gapped) = completed_facts();
    gapped[1].data["source_cursor"] = json!(3);
    assert!(matches!(
        project_entrypoint_parity(EntryPointKind::Web, Some(run_id), &gapped),
        Err(ParityProjectionError::ProjectionFailed(reason)) if reason == "source_cursor_gap"
    ));
}

#[test]
fn parity_snapshot_rejects_unknown_fields_and_completed_without_receipt() {
    let (run_id, events) = completed_facts();
    let snapshot =
        project_entrypoint_parity(EntryPointKind::Workbench, Some(run_id), &events).unwrap();
    let mut encoded = serde_json::to_value(&snapshot).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<kiana_domain::EntryPointParitySnapshot>(encoded).is_err());

    let mut invalid = snapshot.clone();
    invalid.receipt_digest = None;
    invalid.snapshot_digest = invalid.digest();
    assert_eq!(
        invalid.validate().unwrap_err(),
        "entrypoint_parity_completed_receipt_required"
    );
}
