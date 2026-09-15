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
