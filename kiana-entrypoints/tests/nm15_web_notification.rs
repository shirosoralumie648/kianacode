use kiana_entrypoints::web_notifications::{
    decode_notification_cursor, encode_notification_cursor, notification_gap_frame,
    present_notification_frame, present_notification_page,
};
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UI_FEED_FRAME_SCHEMA,
};
use serde_json::{json, Value};

fn page() -> Value {
    json!({
        "schema":"kiana.notification-page.v1",
        "source_cursor":12,
        "items":[{
            "item":{
                "item_id":"notification:15",
                "kind":"approval",
                "title":"Approval required",
                "source_ref":"event:15",
                "detail":{"secret":"must not cross web presenter"},
                "actions":[{"id":"approve","label":"Approve","command":"notification.approve","arguments":{"token":"raw-secret"},"required_fields":[]}]
            },
            "urgency":"critical",
            "due_at_unix_ms":30,
            "read":false,
            "acknowledged":false,
            "snoozed_until_unix_ms":null
        }],
        "next_after_item_id":null,
        "page_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"
    })
}

fn cursor() -> UiFeedCursorV1 {
    UiFeedCursorV1::new(
        "instance-15",
        "epoch-15",
        1,
        UiCursor {
            epoch: "epoch-15".to_owned(),
            sequence: 12,
        },
    )
    .unwrap()
}

fn frame(kind: UiFeedFrameKind, terminal: bool) -> UiFeedFrameV1 {
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind,
        cursor: cursor(),
        event_id: "event-15".to_owned(),
        replay: false,
        terminal,
        event: Some(json!({"secret":"must not cross SSE presenter"})),
        gap: None,
    }
}

#[test]
fn web_page_presenter_is_bounded_and_redacts_detail_and_action_arguments() {
    let view =
        present_notification_page(&page(), "session-15", "tab-15", "instance-15", "epoch-15")
            .unwrap();
    assert_eq!(view["schema"], "kiana.web-notification-page.v1");
    assert_eq!(
        view["items"][0]["actions"][0]["command"],
        "notification.approve"
    );
    assert!(view["unknown_visible"].as_bool().unwrap());
    let encoded = serde_json::to_string(&view).unwrap();
    assert!(!encoded.contains("must not cross web presenter"));
    assert!(!encoded.contains("raw-secret"));
    assert!(!encoded.contains("source_ref"));
}

#[test]
fn web_sse_redacts_feed_payload_and_round_trips_server_cursor() {
    let frame = frame(UiFeedFrameKind::Delta, false);
    let view = present_notification_frame(&frame, "session-15", "tab-15").unwrap();
    assert_eq!(view["kind"], "delta");
    assert_eq!(view["retry"], "do_not_retry");
    assert!(!serde_json::to_string(&view)
        .unwrap()
        .contains("must not cross SSE presenter"));
    let encoded = encode_notification_cursor(&cursor()).unwrap();
    assert_eq!(decode_notification_cursor(&encoded).unwrap(), cursor());
}

#[test]
fn web_gap_is_snapshot_required_and_terminal_is_not_retryable() {
    let gap = kiana_protocol::UiFeedGapV1 {
        schema: kiana_protocol::UI_FEED_GAP_SCHEMA.to_owned(),
        reason: UiFeedGapReason::SequenceGap,
        from: None,
        to: cursor(),
        snapshot_required: true,
    };
    let gap_view = present_notification_frame(
        &notification_gap_frame(gap).unwrap(),
        "session-15",
        "tab-15",
    )
    .unwrap();
    assert_eq!(gap_view["kind"], "gap");
    assert!(gap_view["snapshot_required"].as_bool().unwrap());
    let terminal = present_notification_frame(
        &frame(UiFeedFrameKind::Terminal, true),
        "session-15",
        "tab-15",
    )
    .unwrap();
    assert_eq!(terminal["kind"], "terminal");
    assert_eq!(terminal["retry"], "do_not_retry");
}

#[test]
fn malformed_or_oversized_web_pages_fail_closed() {
    let mut invalid = page();
    invalid["items"][0]["item"]["title"] = Value::String(String::new());
    assert!(
        present_notification_page(&invalid, "session-15", "tab-15", "instance-15", "epoch-15")
            .is_err()
    );
    let mut oversized = page();
    oversized["items"] = Value::Array(vec![page()["items"][0].clone(); 31]);
    assert!(present_notification_page(
        &oversized,
        "session-15",
        "tab-15",
        "instance-15",
        "epoch-15"
    )
    .is_err());
    assert!(decode_notification_cursor("not-json").is_err());
}

#[test]
fn fixture_captures_web_notification_contract() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/nm15-web-notification.json"))
        .expect("valid NM-15 fixture");
    assert_eq!(fixture["schema"], "kiana.web-notification-page.v1");
    assert_eq!(fixture["transport"], "sse");
    assert_eq!(fixture["limits"]["page_items"], 30);
    assert_eq!(fixture["limits"]["actions_per_item"], 16);
    assert_eq!(fixture["cursor"]["header"], "Last-Event-ID");
    for event in [
        "snapshot_boundary",
        "delta",
        "heartbeat",
        "stream_gap",
        "stream_error",
        "terminal",
    ] {
        assert!(fixture["sse_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == event));
    }
    for denial in [
        "wrong_host",
        "wrong_origin",
        "wrong_bearer_or_web_token",
        "cross_session_tab",
        "foreign_or_stale_cursor",
        "payload_or_action_argument_leak",
        "gap_as_completion",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
}
