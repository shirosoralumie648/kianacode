use kiana_entrypoints::notification_cli::{
    present_notification_page, present_run_status, CliRunStatus,
};
use kiana_protocol::{
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UI_FEED_FRAME_SCHEMA,
};
use serde_json::json;

fn page() -> serde_json::Value {
    json!({
        "schema":"kiana.notification-page.v1",
        "source_cursor":12,
        "items":[{
            "item":{
                "item_id":"notification:14",
                "kind":"approval",
                "title":"Approval required",
                "source_ref":"event:14",
                "detail":{"secret":"must not cross presenter","due_at_unix_ms":30,"source_cursor":12},
                "actions":[{"id":"approve","command":"notification.approve","arguments":{"token":"raw"}}]
            },
            "urgency":"high",
            "due_at_unix_ms":30,
            "read":false,
            "acknowledged":false,
            "snoozed_until_unix_ms":null
        }],
        "next_after_item_id":null,
        "page_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"
    })
}

fn frame(kind: UiFeedFrameKind, terminal: bool) -> UiFeedFrameV1 {
    UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind,
        cursor: UiFeedCursorV1::new(
            "instance-14",
            "epoch-14",
            1,
            UiCursor {
                epoch: "epoch-14".to_owned(),
                sequence: 12,
            },
        )
        .unwrap(),
        event_id: "event-14".to_owned(),
        replay: false,
        terminal,
        event: None,
        gap: None,
    }
}

#[test]
fn presenter_redacts_payload_and_keeps_actions_display_only() {
    let view = present_notification_page(&page()).unwrap();
    assert_eq!(view.rows.len(), 1);
    assert_eq!(view.rows[0].actions[0].command, "notification.approve");
    assert!(!serde_json::to_string(&view).unwrap().contains("raw"));
    assert!(view.unknown_visible);
}

#[test]
fn run_status_preserves_unknown_and_terminal_retry_semantics() {
    let running = present_run_status(&frame(UiFeedFrameKind::Delta, false)).unwrap();
    assert_eq!(running.status, CliRunStatus::Running);
    assert_eq!(running.retry, "do_not_retry");
    let unknown = present_run_status(&frame(UiFeedFrameKind::Unknown, false)).unwrap();
    assert_eq!(unknown.status, CliRunStatus::Unknown);
    assert!(unknown.snapshot_required);
    assert_eq!(unknown.retry, "query_original");
    let terminal = present_run_status(&frame(UiFeedFrameKind::Terminal, true)).unwrap();
    assert_eq!(terminal.status, CliRunStatus::Terminal);
}

#[test]
fn malformed_and_oversized_pages_fail_closed() {
    let mut invalid = page();
    invalid["items"][0]["item"]["title"] = serde_json::Value::String("".to_owned());
    assert!(present_notification_page(&invalid).is_err());
    let mut oversized = page();
    oversized["items"] = serde_json::Value::Array(vec![page()["items"][0].clone(); 31]);
    assert!(present_notification_page(&oversized).is_err());
}
