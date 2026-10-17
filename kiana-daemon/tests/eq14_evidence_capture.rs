use kiana_daemon::eval_runtime::{
    EvalBoundaryEvidence, EvalCaptureStatus, EvalEvidenceCapture, EVAL_EVIDENCE_CAPTURE_SCHEMA,
};
use kiana_domain::{CommandReceipt, EvalVerdict, EventId, RequestId, RunId, RuntimeEvent};
use serde_json::json;

fn event(request_id: RequestId, sequence: u64) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, "run.started", json!({"safe": true})).unwrap()
}

fn command_receipt(command_id: RequestId, event_id: EventId) -> CommandReceipt {
    CommandReceipt {
        command_id,
        command_digest: "a".repeat(64),
        commit_id: EventId::new(),
        first_cursor: 3,
        cursor: 3,
        event_ids: vec![event_id],
        versions: Vec::new(),
    }
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
        .record_command_receipt(command_receipt(request_id, event_id))
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

#[test]
fn event_flush_failure_never_returns_eval_pass() {
    // A cleanly flushed capture is the only state that clears the pass gate.
    let mut flushed = EvalEvidenceCapture::new(RunId::new()).unwrap();
    let receipt = flushed.finish(Ok(()));
    assert_eq!(receipt.status, EvalCaptureStatus::Flushed);
    assert!(!receipt.status.blocks_eval_pass());
    assert_eq!(
        receipt.apply_flush_invariant(EvalVerdict::Pass),
        EvalVerdict::Pass
    );

    // A failed flush is classified infra/Unknown and can never yield a pass.
    let mut failed = EvalEvidenceCapture::new(RunId::new()).unwrap();
    let request_id = RequestId::new();
    failed.record_event(event(request_id, 1)).unwrap();
    let receipt = failed.finish(Err("event_store_flush_failed".to_owned()));
    assert_eq!(receipt.status, EvalCaptureStatus::InfraUnknown);
    assert_eq!(receipt.failure_code.as_deref(), Some("infra_flush_unknown"));
    assert!(receipt.status.blocks_eval_pass());
    assert_eq!(
        receipt.apply_flush_invariant(EvalVerdict::Pass),
        EvalVerdict::Blocked
    );
    // Non-passing verdicts are preserved unchanged; only `Pass` is downgraded.
    assert_eq!(
        receipt.apply_flush_invariant(EvalVerdict::Fail),
        EvalVerdict::Fail
    );
    // An unfinished collector is itself blocking.
    assert!(EvalCaptureStatus::Open.blocks_eval_pass());

    // A safety violation blocks an otherwise clean flush.
    let mut unsafe_capture = EvalEvidenceCapture::new(RunId::new()).unwrap();
    let evidence = EvalBoundaryEvidence::new(Vec::new(), Vec::new(), 2, Vec::new()).unwrap();
    unsafe_capture.attach_safety_evidence(evidence).unwrap();
    let receipt = unsafe_capture.finish(Ok(()));
    assert_eq!(receipt.status, EvalCaptureStatus::SafetyViolation);
    assert!(receipt.status.blocks_eval_pass());
    assert_eq!(
        receipt.apply_flush_invariant(EvalVerdict::Pass),
        EvalVerdict::Blocked
    );
}

#[test]
fn evidence_capture_rejects_invalid_events_references_and_receipts() {
    let mut capture = EvalEvidenceCapture::new(RunId::new()).unwrap();
    let request_id = RequestId::new();
    let first = event(request_id, 1);
    let event_id = first.event_id;
    capture.record_event(first.clone()).unwrap();

    // Duplicate event identity is rejected before it can shadow a fact.
    assert_eq!(
        capture.record_event(first).unwrap_err(),
        "eval_capture_event_invalid"
    );
    // A zero sequence cannot be captured.
    let mut zero_sequence = event(request_id, 2);
    zero_sequence.sequence = 0;
    assert_eq!(
        capture.record_event(zero_sequence).unwrap_err(),
        "eval_capture_event_invalid"
    );
    // Oversize payloads are bounded out.
    let oversize = RuntimeEvent::new(
        request_id,
        3,
        "run.output",
        json!({"blob": "x".repeat(300 * 1024)}),
    )
    .unwrap();
    assert_eq!(
        capture.record_event(oversize).unwrap_err(),
        "eval_capture_event_too_large"
    );

    // Reference smuggling secret material is rejected.
    assert_eq!(
        capture
            .record_invocation_ref("{\"api_key\":\"raw-secret\"}")
            .unwrap_err(),
        "eval_capture_invocation_ref_invalid"
    );

    // A cursor that runs backwards is not a valid command receipt.
    let mut backwards = command_receipt(request_id, event_id);
    backwards.first_cursor = 5;
    backwards.cursor = 4;
    assert_eq!(
        capture.record_command_receipt(backwards).unwrap_err(),
        "eval_capture_command_receipt_invalid"
    );
    // A non-hex command digest is rejected.
    let mut non_hex = command_receipt(request_id, event_id);
    non_hex.command_digest = "z".repeat(64);
    assert_eq!(
        capture.record_command_receipt(non_hex).unwrap_err(),
        "eval_capture_command_receipt_invalid"
    );
    // A valid receipt is still accepted after the rejected attempts.
    capture
        .record_command_receipt(command_receipt(request_id, event_id))
        .unwrap();
    let receipt = capture.finish(Ok(()));
    assert_eq!(receipt.status, EvalCaptureStatus::Flushed);
    assert_eq!(receipt.receipt_ref_count, 1);
}
