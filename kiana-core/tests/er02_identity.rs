use kiana_domain::{CapabilityKind, CapabilityRequest, RequestId, RunId, RuntimeEvent};
use serde_json::json;

#[test]
fn cross_run_result_cannot_pair_by_sequence() {
    let selected = RunId::new();
    let foreign = RunId::new();
    let request_id = RequestId::new();
    let capability_request_id = RequestId::new();
    let events = vec![
        event(
            request_id,
            selected,
            "run.capability_requested",
            json!({
                "run_id": selected,
                "request_id": capability_request_id,
                "capability": "query",
                "operation": "search",
                "call_id": "same-sequence",
                "risk": "read_only",
                "arguments": {"query":"selected"}
            }),
        ),
        event(
            request_id,
            foreign,
            "capability.completed",
            json!({
                "run_id": foreign,
                "capability_request_id": capability_request_id,
                "result": {"success":true}
            }),
        ),
    ];
    let projected = kiana_core::project_invocations(selected, &events).unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].request_id, capability_request_id);
    assert!(projected[0].result.is_none());

    let _ = CapabilityRequest::new(
        capability_request_id,
        CapabilityKind::Query,
        "search",
        json!({"query":"selected"}),
    );
}

#[test]
fn invocation_projection_rejects_malformed_identity_links() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let mut requested = event(
        request_id,
        run_id,
        "run.capability_requested",
        json!({
            "run_id": run_id,
            "request_id": request_id,
            "capability_request_id": request_id,
            "capability": "query",
            "operation": "search",
            "risk": "read_only",
            "arguments": {"query": "identity"}
        }),
    );
    requested.parent_event_id = Some(requested.event_id);
    assert_eq!(
        kiana_core::project_invocations(run_id, &[requested]).unwrap_err(),
        "invocation_identity_links_invalid:event_parent_self"
    );
}

#[test]
fn invocation_projection_rejects_request_id_alias_drift() {
    let run_id = RunId::new();
    let request_id = RequestId::new();
    let foreign_request_id = RequestId::new();
    let requested = event(
        request_id,
        run_id,
        "run.capability_requested",
        json!({
            "run_id": run_id,
            "request_id": foreign_request_id,
            "capability_request_id": request_id,
            "capability": "query",
            "operation": "search",
            "risk": "read_only",
            "arguments": {"query": "identity"}
        }),
    );
    assert_eq!(
        kiana_core::project_invocations(run_id, &[requested]).unwrap_err(),
        "invocation_request_id_missing"
    );
}

#[test]
fn legacy_invocation_events_preserve_append_order_without_stream_metadata() {
    let run_id = RunId::new();
    let event_request_id = RequestId::new();
    let result_event_request_id = RequestId::new();
    let capability_request_id = RequestId::new();
    let execution_id = kiana_domain::ExecutionId::new();
    let invocation_id = kiana_domain::InvocationId::new();
    let turn_id = kiana_domain::TurnId::new();
    let requested = RuntimeEvent::new(
        event_request_id,
        2,
        "run.capability_requested",
        json!({
            "run_id": run_id,
            "request_id": capability_request_id,
            "capability_request_id": capability_request_id,
            "capability": "query",
            "operation": "search",
            "risk": "read_only",
            "arguments": {"query": "legacy"}
        }),
    )
    .unwrap();
    let decision = RuntimeEvent::new(
        RequestId::new(),
        4,
        "capability.decision",
        json!({
            "run_id": run_id,
            "capability_request_id": capability_request_id,
            "attempt": 1,
            "gate": {"decision": "allowed"}
        }),
    )
    .unwrap();
    let prepared = RuntimeEvent::new(
        RequestId::new(),
        3,
        "execution.prepared",
        json!({
            "run_id": run_id,
            "capability_request_id": capability_request_id,
            "execution_id": execution_id,
            "invocation_id": invocation_id,
            "turn_id": turn_id,
            "attempt": 1,
            "permit": {"request_id": capability_request_id}
        }),
    )
    .unwrap();
    let executing = RuntimeEvent::new(
        RequestId::new(),
        2,
        "invocation.executing",
        json!({
            "run_id": run_id,
            "capability_request_id": capability_request_id,
            "execution_id": execution_id,
            "invocation_id": invocation_id,
            "turn_id": turn_id,
            "attempt": 1
        }),
    )
    .unwrap();
    let completed = RuntimeEvent::new(
        result_event_request_id,
        1,
        "capability.completed",
        json!({
            "run_id": run_id,
            "capability_request_id": capability_request_id,
            "result": {"success": true}
        }),
    )
    .unwrap();

    let projected = kiana_core::project_invocations(
        run_id,
        &[requested, decision, prepared, executing, completed],
    )
    .unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].state,
        kiana_domain::CapabilityExecutionState::Succeeded
    );
}

fn event(
    request_id: RequestId,
    run_id: RunId,
    kind: &str,
    data: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, 1, kind, data)
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), 1)
}
