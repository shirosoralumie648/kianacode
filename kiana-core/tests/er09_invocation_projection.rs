use kiana_core::{project_capability_attempts, project_invocations};
use kiana_domain::{
    CapabilityEffectState, CapabilityExecutionState, CapabilityStopState, RequestId, RunId,
    RuntimeEvent, TurnId,
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
