use kiana_core::project_capability_attempts;
use kiana_domain::{
    CapabilityAdmissionState, CapabilityApprovalState, CapabilityAttemptRecord,
    CapabilityEffectState, CapabilityStopState, ExecutionId, InvocationId, RequestId, RunId,
    RuntimeEvent, SpanId, TraceId, TraceStatus, TurnId,
};
use serde_json::{json, Value};

fn event(
    run_id: RunId,
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    data: Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), sequence)
}

fn action_digest(byte: char) -> String {
    format!(
        "sha256:{}",
        std::iter::repeat(byte).take(64).collect::<String>()
    )
}

fn request_facts(
    run_id: RunId,
    request_id: RequestId,
    operation: &str,
    digest: &str,
) -> Vec<RuntimeEvent> {
    vec![
        event(
            run_id,
            request_id,
            1,
            "run.capability_requested",
            json!({
                "run_id": run_id,
                "request_id": request_id,
                "capability": "process",
                "operation": operation,
                "risk": "local_write",
                "action_digest": digest,
                "attempt": 1,
                "effect_started": false,
                "effect_known": true,
                "zero_effect": true,
                "stop_state": "not_requested",
                "fenced": false,
                "arguments": {"command": "secret-command", "token": "Bearer raw-secret"}
            }),
        ),
        event(
            run_id,
            request_id,
            2,
            "capability.decision",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "policy": {"decision": "allow", "authorization_id": "policy:test"},
                "gate": {"decision": "allowed", "authorization_id": "policy:test"},
                "action_digest": digest,
                "attempt": 1,
                "effect_started": false,
                "effect_known": true,
                "zero_effect": true,
                "stop_state": "not_requested",
                "fenced": false
            }),
        ),
    ]
}

fn prepared_event(
    run_id: RunId,
    request_id: RequestId,
    invocation_id: InvocationId,
    execution_id: ExecutionId,
    turn_id: TurnId,
    digest: &str,
) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        1,
        "execution.prepared",
        json!({
            "permit": {
                "schema": "kiana.dispatch-permit.v1",
                "execution_id": execution_id,
                "invocation_id": invocation_id,
                "request_id": request_id,
                "run_id": run_id,
                "turn_id": turn_id,
                "action_digest": digest
            },
            "attempt": 1,
            "effect_started": false,
            "effect_known": true,
            "zero_effect": true,
            "fenced": true
        }),
    )
    .unwrap()
    .with_stream_metadata("execution_permit", execution_id.to_string(), 1)
}

#[test]
fn successful_attempt_has_complete_admission_to_effect_evidence_without_raw_payloads() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let turn_id = TurnId::new();
    let digest = action_digest('a');
    let approval_id = kiana_domain::ApprovalId::new();
    let mut events = request_facts(run_id, request_id, "shell.exec", &digest);
    events.extend([
        event(
            run_id,
            request_id,
            3,
            "approval.requested",
            json!({"run_id": run_id, "approval_id": approval_id, "capability_request_id": request_id}),
        ),
        RuntimeEvent::new(
            RequestId::new(),
            2,
            "approval.approved",
            json!({"approval_id": approval_id, "subject_request_id": request_id}),
        )
        .unwrap()
        .with_stream_metadata("approval", approval_id.to_string(), 2),
        prepared_event(run_id, request_id, invocation_id, execution_id, turn_id, &digest),
        event(
            run_id,
            request_id,
            4,
            "invocation.dispatching",
            json!({
                "run_id": run_id,
                "invocation_id": invocation_id,
                "execution_id": execution_id,
                "capability_request_id": request_id,
                "operation": "shell.exec",
                "attempt": 1,
                "started": false,
                "effect_started": false,
                "effect_known": true,
                "zero_effect": true,
                "fenced": true
            }),
        ),
        event(
            run_id,
            request_id,
            5,
            "invocation.executing",
            json!({
                "run_id": run_id,
                "invocation_id": invocation_id,
                "execution_id": execution_id,
                "capability_request_id": request_id,
                "operation": "shell.exec",
                "attempt": 1,
                "started": true,
                "effect_started": true,
                "effect_known": true,
                "zero_effect": false,
                "fenced": true
            }),
        ),
        event(
            run_id,
            request_id,
            6,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "invocation_id": invocation_id,
                "execution_id": execution_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "effect_known": true,
                "stop_requested": false,
                "stop_confirmed": null,
                "fenced": false,
                "result": {"request_id": request_id, "success": true, "output": {"exit_code": 0, "raw_response": "secret"}}
            }),
        ),
    ]);
    let first = project_capability_attempts(run_id, &events).unwrap();
    let second = project_capability_attempts(run_id, &events).unwrap();
    assert_eq!(first, second);
    let record = first.first().expect("capability attempt");
    assert_eq!(record.status, TraceStatus::Ok);
    assert_eq!(record.admission, CapabilityAdmissionState::Allowed);
    assert_eq!(record.approval, CapabilityApprovalState::Approved);
    assert_eq!(record.effect, CapabilityEffectState::Succeeded);
    assert_eq!(record.stop, CapabilityStopState::NotRequested);
    assert!(!record.zero_effect);
    assert!(record.fenced);
    let serialized = serde_json::to_string(record).unwrap();
    for sentinel in ["secret-command", "raw-secret", "raw_response", "Bearer"] {
        assert!(
            !serialized.contains(sentinel),
            "attempt leaked {sentinel}: {serialized}"
        );
    }
    record.validate().unwrap();
}

#[test]
fn deny_expired_approval_and_toctou_never_claim_an_effect() {
    let run_id = RunId::new();
    let denied_request = RequestId::new();
    let denied_digest = action_digest('b');
    let mut denied = request_facts(run_id, denied_request, "shell.exec", &denied_digest);
    denied.push(event(
        run_id,
        denied_request,
        3,
        "run.capability_blocked",
        json!({"run_id": run_id, "capability_request_id": denied_request, "reason": "hook_blocked:policy", "attempt": 1}),
    ));

    let expired_request = RequestId::new();
    let expired_approval = kiana_domain::ApprovalId::new();
    let mut expired = request_facts(run_id, expired_request, "apply_patch", &action_digest('c'));
    expired.push(event(
        run_id,
        expired_request,
        3,
        "approval.requested",
        json!({"run_id": run_id, "approval_id": expired_approval, "capability_request_id": expired_request}),
    ));
    expired.push(
        RuntimeEvent::new(
            RequestId::new(),
            2,
            "approval.expired",
            json!({"approval_id": expired_approval, "request_hash": "sha256:expired"}),
        )
        .unwrap()
        .with_stream_metadata("approval", expired_approval.to_string(), 2),
    );

    let unknown_request = RequestId::new();
    let unknown_digest = action_digest('d');
    let mut unknown = request_facts(run_id, unknown_request, "mcp.call", &unknown_digest);
    unknown.push(event(
        run_id,
        unknown_request,
        3,
        "invocation.dispatching",
        json!({"run_id": run_id, "capability_request_id": unknown_request, "attempt": 1, "started": true, "effect_started": true, "fenced": true}),
    ));
    unknown.push(event(
        run_id,
        unknown_request,
        4,
        "capability.result_unknown",
        json!({"run_id": run_id, "capability_request_id": unknown_request, "attempt": 1, "error": "result_unknown:action_authority_changed", "effect_known": false, "fenced": true}),
    ));

    let mut all = denied;
    all.extend(expired);
    all.extend(unknown);
    let records = project_capability_attempts(run_id, &all).unwrap();
    assert_eq!(records.len(), 3);
    let denied_record = records
        .iter()
        .find(|record| record.request_id == denied_request)
        .unwrap();
    assert_eq!(denied_record.status, TraceStatus::Error);
    assert_eq!(denied_record.zero_effect, true);
    assert_eq!(denied_record.effect, CapabilityEffectState::NotStarted);
    let expired_record = records
        .iter()
        .find(|record| record.request_id == expired_request)
        .unwrap();
    assert_eq!(expired_record.approval, CapabilityApprovalState::Expired);
    assert_eq!(expired_record.zero_effect, true);
    let unknown_record = records
        .iter()
        .find(|record| record.request_id == unknown_request)
        .unwrap();
    assert_eq!(unknown_record.status, TraceStatus::Unknown);
    assert_eq!(unknown_record.effect, CapabilityEffectState::Unknown);
    assert!(unknown_record.fenced);
    assert!(!records
        .iter()
        .any(|record| record.status == TraceStatus::Ok && record.zero_effect));
}

#[test]
fn cancel_without_stop_confirmation_remains_unknown_and_fenced() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let digest = action_digest('e');
    let mut events = request_facts(run_id, request_id, "shell.exec", &digest);
    events.push(event(
        run_id,
        request_id,
        3,
        "invocation.executing",
        json!({"run_id": run_id, "capability_request_id": request_id, "attempt": 1, "effect_started": true}),
    ));
    events.push(event(
        run_id,
        request_id,
        4,
        "run.cancelling",
        json!({"run_id": run_id, "reason": "user"}),
    ));
    events.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id": run_id,
            "capability_request_id": request_id,
            "attempt": 1,
            "effect_known": false,
            "stop_requested": true,
            "stop_confirmed": false,
            "result": {"success": false, "output": {"cancelled": true, "error": "result_unknown:cancel_stop_unconfirmed"}}
        }),
    ));
    let record = project_capability_attempts(run_id, &events)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.status, TraceStatus::Unknown);
    assert_eq!(record.stop, CapabilityStopState::Unconfirmed);
    assert_eq!(record.effect_known, false);
    assert_eq!(record.fenced, true);
}

#[test]
fn contract_rejects_ok_without_permit_or_effect_evidence() {
    let record = CapabilityAttemptRecord::new(
        TraceId::new(),
        SpanId::new(),
        Some(RunId::new()),
        Some(TurnId::new()),
        Some(InvocationId::new()),
        None,
        RequestId::new(),
        1,
        "process",
        "shell.exec",
        action_digest('f'),
        CapabilityAdmissionState::Unknown,
        CapabilityApprovalState::NotRequired,
        CapabilityEffectState::NotStarted,
        CapabilityStopState::NotRequested,
        true,
        None,
        false,
        true,
        TraceStatus::Ok,
        1,
        vec![kiana_domain::EventId::new()],
        None,
        std::collections::BTreeMap::new(),
    );
    assert!(record.is_err());
}
