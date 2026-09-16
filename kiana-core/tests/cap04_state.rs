use kiana_domain::{CapabilityExecutionState, RequestId, RunId, RuntimeEvent};
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

fn requested(run_id: RunId, request_id: RequestId, sequence: u64) -> RuntimeEvent {
    event(
        run_id,
        RequestId::new(),
        sequence,
        "run.capability_requested",
        json!({
            "run_id": run_id,
            "capability_request_id": request_id,
            "request_id": request_id,
            "capability": "process",
            "operation": "shell.exec",
            "risk": "local_write",
            "arguments": {"command": "false"}
        }),
    )
}

fn allowed(run_id: RunId, request_id: RequestId, sequence: u64) -> RuntimeEvent {
    event(
        run_id,
        RequestId::new(),
        sequence,
        "capability.decision",
        json!({
            "run_id": run_id,
            "capability_request_id": request_id,
            "gate": {"decision": "allowed"}
        }),
    )
}

#[test]
fn terminal_execution_cannot_transition_to_success_again() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        requested(run_id, request_id, 1),
        allowed(run_id, request_id, 2),
        event(
            run_id,
            RequestId::new(),
            3,
            "capability.failed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "error": "execution_failed:shell_exit"
            }),
        ),
        event(
            run_id,
            RequestId::new(),
            4,
            "capability.completed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "result": {"success": true}
            }),
        ),
    ];
    let error = kiana_core::project_invocations(run_id, &events).unwrap_err();
    assert_eq!(error, "invocation_state_transition_invalid");
    let error = kiana_core::project_capability_attempts(run_id, &events).unwrap_err();
    assert!(error
        .to_string()
        .contains("capability_attempt_terminal_conflict"));
}

#[test]
fn unknown_effect_cannot_be_projected_as_cancelled() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        requested(run_id, request_id, 1),
        allowed(run_id, request_id, 2),
        event(
            run_id,
            RequestId::new(),
            3,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 1,
                "effect_known": false,
                "result": {
                    "request_id": request_id,
                    "success": false,
                    "output": {"cancelled": true, "error": "result_unknown:stop_unconfirmed"},
                    "evidence_refs": []
                }
            }),
        ),
        event(
            run_id,
            RequestId::new(),
            4,
            "capability.cancelled",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "error": "cancelled:late_stop"
            }),
        ),
    ];
    let error = kiana_core::project_invocations(run_id, &events).unwrap_err();
    assert_eq!(error, "invocation_state_transition_invalid");
    let error = kiana_core::project_capability_attempts(run_id, &events).unwrap_err();
    assert!(error
        .to_string()
        .contains("capability_attempt_terminal_conflict"));
}

#[test]
fn foreign_attempt_result_is_rejected() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        requested(run_id, request_id, 1),
        allowed(run_id, request_id, 2),
        event(
            run_id,
            RequestId::new(),
            3,
            "execution.result_committed",
            json!({
                "run_id": run_id,
                "capability_request_id": request_id,
                "attempt": 2,
                "effect_known": true,
                "result": {
                    "request_id": request_id,
                    "success": true,
                    "output": {"exit_code": 0},
                    "evidence_refs": []
                }
            }),
        ),
    ];
    let error = kiana_core::project_capability_attempts(run_id, &events).unwrap_err();
    assert!(error
        .to_string()
        .starts_with("capability_attempt_foreign_result:"));
}

#[test]
fn recovery_of_dispatching_execution_is_unknown_not_success() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let events = vec![
        requested(run_id, request_id, 1),
        allowed(run_id, request_id, 2),
        event(
            run_id,
            RequestId::new(),
            3,
            "invocation.dispatching",
            json!({"run_id": run_id, "capability_request_id": request_id}),
        ),
    ];
    let projections = kiana_core::project_invocations(run_id, &events).unwrap();
    assert_eq!(projections[0].state, CapabilityExecutionState::Unknown);
}
