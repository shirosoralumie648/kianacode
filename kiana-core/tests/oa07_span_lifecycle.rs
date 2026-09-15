use kiana_core::{project_span_lifecycle, SpanProjectionError};
use kiana_domain::{
    ExecutionId, InvocationId, RequestId, RunId, RuntimeEvent, SpanEntityKind, SpanLifecyclePhase,
    TraceStatus, TurnId,
};
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

fn fixture() -> (RunId, TurnId, InvocationId, ExecutionId, Vec<RuntimeEvent>) {
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let invocation_id = InvocationId::new();
    let execution_id = ExecutionId::new();
    let request_id = RequestId::new();
    let capability_request_id = RequestId::new();
    let events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.authorized",
            json!({"run_id":run_id,"turn_id":turn_id}),
        ),
        event(
            run_id,
            request_id,
            2,
            "run.prompt",
            json!({"run_id":run_id,"turn_id":turn_id}),
        ),
        event(
            run_id,
            request_id,
            3,
            "run.started",
            json!({"run_id":run_id}),
        ),
        event(
            run_id,
            request_id,
            4,
            "run.capability_requested",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "capability_request_id":capability_request_id,"attempt":1}),
        ),
        event(
            run_id,
            request_id,
            5,
            "approval.requested",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "capability_request_id":capability_request_id}),
        ),
        event(
            run_id,
            request_id,
            6,
            "approval.approved",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "capability_request_id":capability_request_id}),
        ),
        event(
            run_id,
            request_id,
            7,
            "invocation.dispatching",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "execution_id":execution_id,"capability_request_id":capability_request_id,"attempt":1}),
        ),
        event(
            run_id,
            request_id,
            8,
            "invocation.executing",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "execution_id":execution_id,"capability_request_id":capability_request_id,"attempt":1}),
        ),
        event(
            run_id,
            request_id,
            9,
            "run.compacted",
            json!({"run_id":run_id,"tokens_before":100,"tokens_after":40,"summary_present":true}),
        ),
        event(
            run_id,
            request_id,
            10,
            "execution.result_committed",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "execution_id":execution_id,"capability_request_id":capability_request_id,
                "attempt":1,"effect_known":true,"stop_confirmed":true,
                "result":{"success":true,"output":{"value":"ok"}}}),
        ),
        event(
            run_id,
            request_id,
            11,
            "run.completed",
            json!({"run_id":run_id,"result":"done"}),
        ),
    ];
    (run_id, turn_id, invocation_id, execution_id, events)
}

#[test]
fn lifecycle_projection_is_deterministic_and_covers_run_turn_invocation() {
    let (run_id, _turn_id, _invocation_id, _execution_id, events) = fixture();
    let first = project_span_lifecycle(run_id, &events).unwrap();
    let second = project_span_lifecycle(run_id, &events).unwrap();
    assert_eq!(first, second);
    assert!(first.iter().any(|record| {
        record.entity == SpanEntityKind::Run && record.phase == SpanLifecyclePhase::Started
    }));
    assert!(first.iter().any(|record| {
        record.entity == SpanEntityKind::Turn && record.phase == SpanLifecyclePhase::Checkpointed
    }));
    assert!(first.iter().any(|record| {
        record.entity == SpanEntityKind::Invocation && record.phase == SpanLifecyclePhase::Paused
    }));
    assert!(first.iter().any(|record| {
        record.entity == SpanEntityKind::Invocation
            && record.phase == SpanLifecyclePhase::Ended
            && record.status == TraceStatus::Ok
    }));
}

#[test]
fn late_delta_and_duplicate_terminal_do_not_mutate_ended_spans() {
    let (run_id, _turn_id, _invocation_id, _execution_id, mut events) = fixture();
    let completed = events.last().cloned().unwrap();
    events.push(event(
        run_id,
        RequestId::new(),
        12,
        "run.delta",
        json!({"run_id":run_id,"text":"late"}),
    ));
    events.push(event(
        run_id,
        RequestId::new(),
        13,
        "run.completed",
        json!({"run_id":run_id,"result":"replayed"}),
    ));
    let projected = project_span_lifecycle(run_id, &events).unwrap();
    assert_eq!(completed.kind, "run.completed");
    assert_eq!(
        projected
            .iter()
            .filter(|record| record.phase == SpanLifecyclePhase::Ended)
            .count(),
        3,
        "run, turn and invocation ends must remain single rows per span"
    );
    assert!(projected
        .iter()
        .all(|record| record.event_kind != "run.delta"));
}

#[test]
fn conflicting_terminal_and_stale_attempt_fail_closed_or_remain_unknown() {
    let (run_id, turn_id, invocation_id, execution_id, mut events) = fixture();
    events.push(event(
        run_id,
        RequestId::new(),
        12,
        "run.failed",
        json!({"run_id":run_id,"error":"contradiction"}),
    ));
    let error = project_span_lifecycle(run_id, &events).unwrap_err();
    assert!(matches!(
        error,
        SpanProjectionError::TerminalConflict { .. }
    ));

    let retry_events = vec![
        event(
            run_id,
            RequestId::new(),
            1,
            "run.prompt",
            json!({"run_id":run_id,"turn_id":turn_id}),
        ),
        event(
            run_id,
            RequestId::new(),
            2,
            "run.capability_requested",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,"attempt":2}),
        ),
        event(
            run_id,
            RequestId::new(),
            3,
            "execution.result_committed",
            json!({"run_id":run_id,"turn_id":turn_id,"invocation_id":invocation_id,
                "execution_id":execution_id,"attempt":1,"effect_known":true,
                "result":{"success":true,"output":{}}}),
        ),
    ];
    let projected = project_span_lifecycle(run_id, &retry_events).unwrap();
    assert!(projected
        .iter()
        .all(|record| record.event_kind != "execution.result_committed"));
}
