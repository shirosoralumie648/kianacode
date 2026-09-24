use kiana_domain::{
    ExecutionStatus, RequestId, RuntimeEvidenceBundle, RuntimeReceiptRef, WorkflowIncident,
};

fn refs() -> Vec<String> {
    vec!["event:runtime-1".to_owned(), "event:runtime-2".to_owned()]
}

#[test]
fn runtime_receipt_requires_terminal_status_and_bound_event_refs() {
    let request_id = RequestId::new();
    let receipt = RuntimeReceiptRef::new(request_id, ExecutionStatus::Completed, refs()).unwrap();
    assert_eq!(receipt.request_id, request_id);
    assert_eq!(receipt.status, ExecutionStatus::Completed);
    assert!(receipt.validate().is_ok());

    let mut tampered = receipt.clone();
    tampered.status = ExecutionStatus::Running;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "runtime_receipt_terminal_status_required"
    );

    assert!(RuntimeReceiptRef::new(request_id, ExecutionStatus::Running, refs()).is_err());
    let mut digest_tampered = receipt;
    digest_tampered.event_refs.push("event:extra".to_owned());
    assert_eq!(
        digest_tampered.validate().unwrap_err(),
        "runtime_receipt_digest_mismatch"
    );
}

#[test]
fn evidence_bundle_and_unknown_incident_are_explicitly_bound() {
    let request_id = RequestId::new();
    let receipt =
        RuntimeReceiptRef::new(request_id, ExecutionStatus::ResultUnknown, refs()).unwrap();
    let bundle =
        RuntimeEvidenceBundle::new(receipt.clone(), vec!["artifact:build-report".to_owned()])
            .unwrap();
    assert!(bundle.validate().is_ok());

    let incident = WorkflowIncident::new(
        "workflow-unknown:test",
        "instance-1",
        "node-1",
        request_id,
        "workflow_runtime_result_unknown",
        receipt.event_refs.clone(),
    )
    .unwrap();
    assert!(incident.validate().is_ok());

    let mut mismatch = incident;
    mismatch.request_id = RequestId::new();
    assert_eq!(
        mismatch.validate().unwrap_err(),
        "workflow_incident_digest_mismatch"
    );
}

#[test]
fn non_event_and_non_artifact_refs_fail_closed() {
    let request_id = RequestId::new();
    assert_eq!(
        RuntimeReceiptRef::new(
            request_id,
            ExecutionStatus::Completed,
            vec!["artifact:wrong-boundary".to_owned()],
        )
        .unwrap_err(),
        "runtime_event_reference_invalid"
    );
    let receipt = RuntimeReceiptRef::new(
        request_id,
        ExecutionStatus::Completed,
        vec!["event:runtime-1".to_owned()],
    )
    .unwrap();
    assert_eq!(
        RuntimeEvidenceBundle::new(receipt, vec!["event:not-artifact".to_owned()]).unwrap_err(),
        "runtime_artifact_reference_invalid"
    );
}
