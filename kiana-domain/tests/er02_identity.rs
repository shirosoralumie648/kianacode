use kiana_domain::{EventId, JournalFrame, JournalFramePayload, RequestId, RuntimeEvent};
use serde_json::json;
use uuid::Uuid;

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

#[test]
fn journal_frames_reject_identity_link_drift() {
    let mut event = RuntimeEvent::new(RequestId::new(), 1, "run.accepted", json!({})).unwrap();
    event.parent_event_id = Some(event.event_id);
    let frame = JournalFrame::new(JournalFramePayload::Event { event }).unwrap();
    assert_eq!(
        frame.validate().unwrap_err(),
        "journal_frame_event_identity_invalid:event_parent_self"
    );
}

#[test]
fn nil_identity_links_fail_closed_but_legacy_links_remain_optional() {
    let request_id = RequestId::new();
    let nil_request = RequestId::from_uuid(Uuid::nil());
    let nil_event = EventId::from_uuid(Uuid::nil());

    let mut nil_command = RuntimeEvent::new(request_id, 1, "run.accepted", json!({})).unwrap();
    nil_command.command_id = Some(nil_request);
    assert_eq!(
        nil_command.validate_identity_links().unwrap_err(),
        "event_command_id_invalid"
    );

    let mut nil_correlation = RuntimeEvent::new(request_id, 1, "run.accepted", json!({})).unwrap();
    nil_correlation.correlation_id = Some(nil_request);
    assert_eq!(
        nil_correlation.validate_identity_links().unwrap_err(),
        "event_correlation_id_invalid"
    );

    let mut nil_causation = RuntimeEvent::new(request_id, 1, "run.accepted", json!({})).unwrap();
    nil_causation.causation_event_id = Some(nil_event);
    assert_eq!(
        nil_causation.validate_identity_links().unwrap_err(),
        "event_causation_id_invalid"
    );

    let mut nil_parent = RuntimeEvent::new(request_id, 1, "run.accepted", json!({})).unwrap();
    nil_parent.parent_event_id = Some(nil_event);
    assert_eq!(
        nil_parent.validate_identity_links().unwrap_err(),
        "event_parent_id_invalid"
    );
}
