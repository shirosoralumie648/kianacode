use kiana_domain::{RequestId, RuntimeEvent};
use kiana_quality::{canonical_json, ArrayPolicy, DurableEvent, TraceNormalizer};
use serde_json::json;

fn event(
    request_id: RequestId,
    sequence: u64,
    kind: &str,
    run_id: &str,
    data: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(request_id, sequence, kind, data)
        .expect("valid runtime event")
        .with_stream_metadata("run", run_id, sequence)
        .with_identity_links(None, Some(request_id), None, None)
}

#[test]
fn canonical_json_sorts_objects_and_respects_explicit_array_policy() {
    let ordered = canonical_json(
        json!({"z": {"b": 2, "a": 1}, "items": [2, 1]}),
        ArrayPolicy::Ordered,
    )
    .expect("ordered canonical json");
    let multiset = canonical_json(
        json!({"items": [2, 1], "z": {"a": 1, "b": 2}}),
        ArrayPolicy::Multiset,
    )
    .expect("multiset canonical json");
    assert_eq!(ordered["z"]["a"], 1);
    assert_eq!(ordered["items"], json!([2, 1]));
    assert_eq!(multiset["items"], json!([1, 2]));
    assert_ne!(ordered["items"], multiset["items"]);
}

#[test]
fn canonical_event_uses_whitelist_and_shared_redaction_boundary() {
    let request_id = RequestId::new();
    let data = json!({
        "run_id": "run-1",
        "text": "Authorization: Bearer should-not-escape",
        "arguments": {"api_key": "secret-value", "safe": "visible"}
    });
    let trace = TraceNormalizer::new()
        .canonicalize(
            &[DurableEvent::new(
                4,
                event(request_id, 1, "run.prompt", "run-1", data),
            )],
            ArrayPolicy::Ordered,
        )
        .expect("canonical trace");
    let value = &trace.events[0].value;
    assert_eq!(value["data"]["text"], "Authorization: Bearer [REDACTED]");
    assert_eq!(value["data"]["arguments"]["api_key"], "[REDACTED]");
    assert_eq!(value["data"]["arguments"]["safe"], "visible");
    assert!(trace.canonical_bytes().is_ok());
}

#[test]
fn unknown_payload_field_is_rejected_before_canonical_output() {
    let request_id = RequestId::new();
    let result = TraceNormalizer::new().canonicalize(
        &[DurableEvent::new(
            1,
            event(
                request_id,
                1,
                "run.started",
                "run-2",
                json!({"run_id": "run-2", "unexpected": true}),
            ),
        )],
        ArrayPolicy::Ordered,
    );
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("event_payload_unknown_field"));
}
