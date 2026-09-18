use kiana_daemon::eval_runtime::{
    EvalCaptureStatus, EvalEvidenceCapture, EVAL_EVIDENCE_CAPTURE_SCHEMA,
};
use kiana_domain::{CommandReceipt, EventId, RequestId, RunId, RuntimeEvent};
use serde_json::json;

fn event(request_id: RequestId, sequence: u64) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, "run.started", json!({"safe": true})).unwrap()
}

#[test]
fn evidence_capture_collects_refs_and_command_receipt() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let mut capture = EvalEvidenceCapture::new(run_id).unwrap();
    let source_event = event(request_id, 3);
    let event_id = source_event.event_id;
    capture.record_event(source_event).unwrap();
    capture.record_invocation_ref("invocation:eq14:1").unwrap();
    capture
        .record_artifact_ref("artifact:fixture-output")
        .unwrap();
    capture.record_receipt_ref("receipt:run-1").unwrap();
    capture
        .record_command_receipt(CommandReceipt {
            command_id: request_id,
            command_digest: "a".repeat(64),
            commit_id: EventId::new(),
            first_cursor: 3,
            cursor: 3,
            event_ids: vec![event_id],
            versions: Vec::new(),
        })
        .unwrap();

    let receipt = capture.finish(Ok(()));
    assert_eq!(receipt.schema, EVAL_EVIDENCE_CAPTURE_SCHEMA);
    assert_eq!(receipt.status, EvalCaptureStatus::Flushed);
    assert_eq!(receipt.event_count, 1);
    assert_eq!(receipt.invocation_ref_count, 1);
    assert_eq!(receipt.artifact_ref_count, 1);
    assert_eq!(receipt.receipt_ref_count, 2);
    assert_eq!(receipt.source_cursor, 3);
    receipt.validate().unwrap();
    assert_eq!(capture.status(), EvalCaptureStatus::Flushed);
    assert!(capture.record_invocation_ref("late").is_err());
}

#[test]
fn flush_failure_is_infra_unknown_and_secret_events_are_rejected() {
    let mut capture = EvalEvidenceCapture::new(RunId::new()).unwrap();
    let request_id = RequestId::new();
    assert_eq!(
        capture
            .record_event(
                RuntimeEvent::new(
                    request_id,
                    1,
                    "run.output",
                    json!({"api_key": "raw-secret"}),
                )
                .unwrap()
            )
            .unwrap_err(),
        "eval_capture_event_invalid"
    );

    let receipt = capture.finish(Err("event_store_flush_failed".to_owned()));
    assert_eq!(receipt.status, EvalCaptureStatus::InfraUnknown);
    assert_eq!(receipt.failure_code.as_deref(), Some("infra_flush_unknown"));
    receipt.validate().unwrap();
    assert_eq!(capture.status(), EvalCaptureStatus::InfraUnknown);
}
