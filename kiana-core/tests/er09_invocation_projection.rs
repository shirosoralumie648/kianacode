use kiana_core::{project_capability_attempts, project_invocations};
use kiana_domain::{
    CapabilityEffectState, CapabilityExecutionState, CapabilityResult, CapabilityResultReceipt,
    CapabilityStopState, ExecutionId, InvocationId, RequestId, RunId, RuntimeEvent, TurnId,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

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

fn requested(run_id: RunId, request_id: RequestId, sequence: u64, attempt: u32) -> RuntimeEvent {
    event(
        run_id,
        request_id,
        sequence,
        "run.capability_requested",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "capability":"query",
            "operation":"search",
            "arguments":{"query":"roadmap"},
            "risk":"read_only",
            "action_digest":digest('a'),
            "attempt":attempt,
        }),
    )
}

fn executing_events(run_id: RunId, request_id: RequestId) -> Vec<RuntimeEvent> {
    let execution_id = kiana_domain::ExecutionId::new();
    let invocation_id = kiana_domain::InvocationId::new();
    let turn_id = TurnId::new();
    vec![
        requested(run_id, request_id, 1, 1),
        event(
            run_id,
            request_id,
            2,
            "capability.decision",
            json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"action_digest":digest('a'),"gate":{"decision":"allowed"}}),
        ),
        event(
            run_id,
            request_id,
            3,
            "execution.prepared",
            json!({
                "run_id":run_id,
                "capability_request_id":request_id,
                "execution_id":execution_id,
                "invocation_id":invocation_id,
                "turn_id":turn_id,
                "attempt":1,
                "permit":{"request_id":request_id,"execution_id":execution_id,"invocation_id":invocation_id,"turn_id":turn_id,"action_digest":digest('a')},
            }),
        ),
        event(
            run_id,
            request_id,
            4,
            "invocation.executing",
            json!({"run_id":run_id,"capability_request_id":request_id,"execution_id":execution_id,"invocation_id":invocation_id,"turn_id":turn_id,"attempt":1,"action_digest":digest('a')}),
        ),
    ]
}

#[test]
fn invocation_projection_marks_dispatch_without_result_unknown() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let projections = project_invocations(run_id, &executing_events(run_id, request_id)).unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].state, CapabilityExecutionState::Unknown);
}

#[test]
fn invocation_projection_rejects_attempt_digest_and_terminal_conflicts() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let mut events = executing_events(run_id, request_id);
    events.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_known":true,"result":{"request_id":request_id,"success":true,"output":{"answer":"one"}}}),
    ));
    let mut conflicting = events.clone();
    conflicting.push(event(
        run_id,
        request_id,
        6,
        "execution.result_committed",
        json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_known":true,"result":{"request_id":request_id,"success":true,"output":{"answer":"two"}}}),
    ));
    assert_eq!(
        project_invocations(run_id, &conflicting).unwrap_err(),
        "invocation_terminal_conflict"
    );

    let digest_conflict = event(
        run_id,
        request_id,
        5,
        "invocation.dispatching",
        json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"action_digest":digest('b')}),
    );
    let mut changed = executing_events(run_id, request_id);
    changed.push(digest_conflict);
    assert_eq!(
        project_invocations(run_id, &changed).unwrap_err(),
        "invocation_action_digest_conflict"
    );
}

#[test]
fn invocation_projection_normalizes_and_validates_terminal_result_receipts() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let result = CapabilityResult::success(request_id, json!({ "answer": "one" }));
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::new();
    let dispatch_receipt = CapabilityResultReceipt::from_result(
        &result,
        Some(execution_id),
        Some(invocation_id),
        1,
        true,
    )
    .unwrap();
    let finalizer_receipt =
        CapabilityResultReceipt::from_result(&result, None, None, 1, true).unwrap();
    let mut matching = executing_events(run_id, request_id);
    matching.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":dispatch_receipt.clone(),
        }),
    ));
    matching.push(event(
        run_id,
        request_id,
        6,
        "capability.completed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "attempt":1,
            "effect_started":true,
            "effect_known":true,
            "zero_effect":false,
            "fenced":false,
            "answer":"one",
            "result_receipt":finalizer_receipt.clone(),
        }),
    ));
    let projected = project_invocations(run_id, &matching).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].request_id, request_id);
    assert_eq!(projected[0].state, CapabilityExecutionState::Succeeded);
    assert_eq!(
        projected[0].result,
        Some(matching[4].data["result"].clone())
    );
    assert_eq!(
        projected[0].event_ids,
        matching
            .iter()
            .map(|event| event.event_id.to_string())
            .collect::<Vec<_>>()
    );

    let conflicting_result = CapabilityResult::success(request_id, json!({ "answer": "two" }));
    let conflicting_receipt =
        CapabilityResultReceipt::from_result(&conflicting_result, None, None, 1, true).unwrap();
    let mut conflicting = executing_events(run_id, request_id);
    conflicting.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":dispatch_receipt.clone(),
        }),
    ));
    conflicting.push(event(
        run_id,
        request_id,
        6,
        "capability.completed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "attempt":1,
            "effect_started":true,
            "effect_known":true,
            "zero_effect":false,
            "fenced":false,
            "answer":"two",
            "result_receipt":conflicting_receipt.clone(),
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &conflicting).unwrap_err(),
        "invocation_terminal_conflict"
    );

    let mut mismatched_result_receipt = executing_events(run_id, request_id);
    mismatched_result_receipt.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":conflicting_receipt.clone(),
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &mismatched_result_receipt).unwrap_err(),
        "invocation_result_receipt_result_conflict"
    );

    let mut mismatched_execution_identity = executing_events(run_id, request_id);
    mismatched_execution_identity.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":ExecutionId::new(),
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":dispatch_receipt.clone(),
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &mismatched_execution_identity).unwrap_err(),
        "invocation_result_receipt_identity_conflict"
    );

    let foreign_request_id = RequestId::new();
    let foreign_result = CapabilityResult::success(foreign_request_id, json!({ "answer": "one" }));
    let foreign_receipt = CapabilityResultReceipt::from_result(
        &foreign_result,
        Some(execution_id),
        Some(invocation_id),
        1,
        true,
    )
    .unwrap();
    let mut mismatched_receipt_request_id = executing_events(run_id, request_id);
    mismatched_receipt_request_id.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":foreign_receipt,
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &mismatched_receipt_request_id).unwrap_err(),
        "invocation_result_receipt_request_id_conflict"
    );

    let mut mismatched_execution_effect_known = executing_events(run_id, request_id);
    mismatched_execution_effect_known.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":false,
            "outcome_state":CapabilityExecutionState::Succeeded,
            "result":result.clone(),
            "result_receipt":dispatch_receipt.clone(),
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &mismatched_execution_effect_known).unwrap_err(),
        "invocation_result_receipt_effect_known_conflict"
    );

    let mut mismatched_execution_outcome_state = executing_events(run_id, request_id);
    mismatched_execution_outcome_state.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "outcome_state":CapabilityExecutionState::Unknown,
            "result":result.clone(),
            "result_receipt":dispatch_receipt.clone(),
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &mismatched_execution_outcome_state).unwrap_err(),
        "invocation_result_receipt_outcome_state_conflict"
    );

    let mut mismatched_lifecycle = executing_events(run_id, request_id);
    mismatched_lifecycle.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result,
            "result_receipt":dispatch_receipt,
        }),
    ));
    mismatched_lifecycle.push(event(
        run_id,
        request_id,
        6,
        "capability.completed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "attempt":1,
            "effect_started":true,
            "effect_known":false,
            "zero_effect":false,
            "fenced":false,
            "answer":"one",
            "result_receipt":finalizer_receipt,
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &mismatched_lifecycle).unwrap_err(),
        "invocation_result_receipt_lifecycle_conflict"
    );
}

#[test]
fn invocation_projection_rejects_uncommitted_terminal_receipts() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let result = CapabilityResult::success(request_id, json!({ "answer": "one" }));
    let execution_id = ExecutionId::new();
    let invocation_id = InvocationId::new();
    let committed_dispatch_receipt = CapabilityResultReceipt::from_result(
        &result,
        Some(execution_id),
        Some(invocation_id),
        1,
        true,
    )
    .unwrap();
    let uncommitted_dispatch_receipt = CapabilityResultReceipt::from_result(
        &result,
        Some(execution_id),
        Some(invocation_id),
        1,
        false,
    )
    .unwrap();
    let uncommitted_finalizer_receipt =
        CapabilityResultReceipt::from_result(&result, None, None, 1, false).unwrap();

    let mut uncommitted_execution = executing_events(run_id, request_id);
    uncommitted_execution.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":uncommitted_dispatch_receipt,
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &uncommitted_execution).unwrap_err(),
        "invocation_result_receipt_uncommitted"
    );

    let mut uncommitted_finalizer = executing_events(run_id, request_id);
    uncommitted_finalizer.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "execution_id":execution_id,
            "invocation_id":invocation_id,
            "attempt":1,
            "effect_known":true,
            "result":result.clone(),
            "result_receipt":committed_dispatch_receipt,
        }),
    ));
    uncommitted_finalizer.push(event(
        run_id,
        request_id,
        6,
        "capability.completed",
        json!({
            "run_id":run_id,
            "capability_request_id":request_id,
            "attempt":1,
            "effect_started":true,
            "effect_known":true,
            "zero_effect":false,
            "fenced":false,
            "answer":"one",
            "result_receipt":uncommitted_finalizer_receipt,
        }),
    ));
    assert_eq!(
        project_invocations(run_id, &uncommitted_finalizer).unwrap_err(),
        "invocation_result_receipt_uncommitted"
    );
}

#[test]
fn capability_attempt_projection_preserves_unknown_stop_and_retry_attempts() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let mut events = executing_events(run_id, request_id);
    events.push(event(
        run_id,
        request_id,
        5,
        "execution.result_committed",
        json!({"run_id":run_id,"capability_request_id":request_id,"attempt":1,"effect_known":false,"stop_confirmed":false,"result":{"request_id":request_id,"success":false,"output":{"error":"result_unknown:disconnect"}}}),
    ));
    let records = project_capability_attempts(run_id, &events).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].effect, CapabilityEffectState::Unknown);
    assert_eq!(records[0].stop, CapabilityStopState::Unconfirmed);
    assert!(!records[0].effect_known);
    assert!(records[0].fenced);

    let retry = requested(run_id, request_id, 6, 2);
    let mut retry_events = events;
    retry_events.push(retry);
    let records = project_capability_attempts(run_id, &retry_events).unwrap();
    assert!(records.iter().any(|record| record.attempt == 1));
    assert!(records.iter().any(|record| record.attempt == 2));
}

#[test]
fn er09_projection_exposes_only_typed_attempt_identity() {
    let invocation = include_str!("../src/invocation_projection.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    for marker in [
        "invocation_terminal_conflict",
        "invocation_action_digest_conflict",
        "CapabilityExecutionState::Unknown",
        "execution.result_committed",
        "terminal_signature",
        "CapabilityResultReceipt::from_json",
        "CapabilityResultReceipt::from_result",
        "invocation_result_receipt_uncommitted",
        "receipt.execution_id",
        "receipt.invocation_id",
        "invocation_result_receipt_lifecycle_conflict",
        "invocation_result_receipt_effect_known_conflict",
        "invocation_result_receipt_outcome_state_conflict",
        "result_digest",
        "receipt_digest",
        "digest_for",
        "effect_known",
        "stop_confirmed",
        "source_event_ids",
        "fenced",
    ] {
        assert!(
            invocation.contains(marker) || attempts.contains(marker),
            "ER-09 marker missing: {marker}"
        );
    }
    assert!(attempts.contains("ForeignAttemptResult"));
    assert!(attempts.contains("multiple") || attempts.contains("terminal_digest"));
    for forbidden in [
        "CapabilityBrokerPort",
        "broker.execute",
        "RunnerCommand",
        "auto_retry_unknown",
    ] {
        assert!(
            !invocation.contains(forbidden) && !attempts.contains(forbidden),
            "projection must not {forbidden}"
        );
    }
}
