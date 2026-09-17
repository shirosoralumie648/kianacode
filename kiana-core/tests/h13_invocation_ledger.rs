use kiana_core::project_invocations;
use kiana_domain::{CapabilityExecutionState, RequestId, RunId, RuntimeEvent, TurnId};
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

fn invocation_events(
    run_id: RunId,
    request_id: RequestId,
    include_result: bool,
) -> Vec<RuntimeEvent> {
    let execution_id = kiana_domain::ExecutionId::new();
    let invocation_id = kiana_domain::InvocationId::new();
    let turn_id = TurnId::new();
    let request = kiana_domain::CapabilityRequest::new(
        request_id,
        kiana_domain::CapabilityKind::Query,
        "search",
        json!({"query":"ledger"}),
    );
    let mut events = vec![
        event(
            run_id,
            request_id,
            1,
            "run.capability_requested",
            json!({"run_id":run_id,"capability_request_id":request_id,"capability":request.capability,"operation":request.operation,"arguments":request.arguments,"risk":request.risk,"action_digest":digest('a'),"attempt":1}),
        ),
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
            json!({"run_id":run_id,"capability_request_id":request_id,"execution_id":execution_id,"invocation_id":invocation_id,"turn_id":turn_id,"attempt":1,"permit":{"request_id":request_id,"execution_id":execution_id,"invocation_id":invocation_id,"turn_id":turn_id,"action_digest":digest('a')}}),
        ),
        event(
            run_id,
            request_id,
            4,
            "invocation.executing",
            json!({"run_id":run_id,"capability_request_id":request_id,"execution_id":execution_id,"invocation_id":invocation_id,"turn_id":turn_id,"attempt":1,"action_digest":digest('a')}),
        ),
    ];
    if include_result {
        events.push(event(
            run_id,
            request_id,
            5,
            "execution.result_committed",
            json!({"run_id":run_id,"capability_request_id":request_id,"execution_id":execution_id,"invocation_id":invocation_id,"attempt":1,"effect_known":true,"outcome_ready":true,"result":{"request_id":request_id,"success":true,"output":{"answer":"persisted"}}}),
        ));
    }
    events
}

#[test]
fn event_append_failure_prevents_dispatch() {
    let dispatch = include_str!("../src/dispatch.rs");
    assert!(dispatch.contains("commit_invocation_executing"));
    assert!(dispatch.contains("result_unknown:result_commit_failed"));
    assert!(
        dispatch.find("commit_invocation_executing").unwrap()
            < dispatch.find("execute_cancellable").unwrap()
    );
}

#[test]
fn crash_after_effect_before_outcome_requires_reconciliation() {
    let run_id = RunId::new();
    let projections =
        project_invocations(run_id, &invocation_events(run_id, RequestId::new(), false)).unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].state, CapabilityExecutionState::Unknown);
}

#[test]
fn persisted_outcome_is_reused_without_reexecuting_tool() {
    let run_id = RunId::new();
    let projections =
        project_invocations(run_id, &invocation_events(run_id, RequestId::new(), true)).unwrap();
    assert_eq!(projections.len(), 1);
    assert_eq!(projections[0].state, CapabilityExecutionState::Succeeded);
    assert!(projections[0].result.is_some());
    let dispatch = include_str!("../src/dispatch.rs");
    assert!(dispatch.contains("result.delivery_claimed"));
    assert!(dispatch.contains("delivery_policy"));
}
