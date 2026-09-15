use kiana_domain::{EventId, RequestId, RuntimeEvent};
use serde_json::json;

#[test]
fn new_events_have_request_correlation_and_explicit_links_round_trip() {
    let request_id = RequestId::new();
    let causation = EventId::new();
    let parent = EventId::new();
    let event = RuntimeEvent::new(request_id, 1, "run.prompt", json!({"run_id":"run-1"}))
        .unwrap()
        .with_identity_links(Some(request_id), None, Some(causation), Some(parent));
    assert_eq!(event.correlation_id, Some(request_id));
    assert_eq!(event.command_id, Some(request_id));
    assert_eq!(event.causation_event_id, Some(causation));
    assert_eq!(event.parent_event_id, Some(parent));
    event.validate_identity_links().unwrap();
    let encoded = serde_json::to_value(&event).unwrap();
    let decoded: RuntimeEvent = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, event);
}

#[test]
fn legacy_events_without_links_remain_readable_but_self_links_fail_closed() {
    let request_id = RequestId::new();
    let legacy = json!({
        "event_id": EventId::new(),
        "request_id": request_id,
        "sequence": 1,
        "kind": "future.opaque",
        "data": {"opaque": true}
    });
    let decoded: RuntimeEvent = serde_json::from_value(legacy).unwrap();
    assert!(decoded.correlation_id.is_none());
    decoded.validate_identity_links().unwrap();

    let self_id = decoded.event_id;
    let linked = decoded.with_identity_links(None, Some(request_id), Some(self_id), None);
    assert_eq!(
        linked.validate_identity_links().unwrap_err(),
        "event_causation_self"
    );
}
