use kiana_domain::{
    json_digest, project_ui_snapshot, RequestId, RuntimeEvent, UiSnapshotEntryKind,
    UiSnapshotQuery,
};
use serde_json::json;

fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

fn fixture_events() -> Vec<RuntimeEvent> {
    let mut action_accepted = event(
        3,
        "ui.action.accepted",
        json!({
            "session_id":"session-1",
            "action": {"owner_id":"owner-1","session_id":"session-1"},
            "record": {"state":"accepted"},
            "state":"accepted"
        }),
    )
    .with_stream_metadata("ui_action", "action-1", 1);
    action_accepted.idempotency_key = Some("ui-action:action-1:accepted".to_owned());
    let action_applied = event(
        4,
        "ui.action.applied",
        json!({
            "session_id":"session-1",
            "action": {"owner_id":"owner-1","session_id":"session-1"},
            "record": {"state":"applied"},
            "state":"applied"
        }),
    )
    .with_stream_metadata("ui_action", "action-1", 2);
    vec![
        event(
            1,
            "session.created",
            json!({"owner_id":"owner-1","session_id":"session-1"}),
        ),
        event(
            2,
            "run.authorized",
            json!({"owner_id":"owner-1","session_id":"session-1","run_id":"run-1"}),
        ),
        action_accepted,
        action_applied,
        event(
            5,
            "artifact.created",
            json!({"owner_id":"owner-1","session_id":"session-1","artifact_id":"artifact-1"}),
        ),
    ]
}

fn query(source_cursor: u64, page_size: usize, after: Option<String>) -> UiSnapshotQuery {
    UiSnapshotQuery::new(
        "owner-1",
        Some("session-1".to_owned()),
        "epoch-1",
        source_cursor,
        Some(source_cursor),
        1,
        1,
        0,
        page_size,
        after,
        1,
    )
    .unwrap()
}

#[test]
fn snapshot_is_atomic_stable_and_page_cursor_bound() {
    let events = fixture_events();
    let first = project_ui_snapshot(&events, &query(5, 2, None)).unwrap();
    assert_eq!(first.snapshot_cursor, 5);
    assert_eq!(first.entries.len(), 2);
    assert_eq!(first.entries[0].kind, UiSnapshotEntryKind::Session);
    assert_eq!(first.entries[1].kind, UiSnapshotEntryKind::Run);
    let next = first.next_page.clone().expect("second page");
    let second = project_ui_snapshot(&events, &query(5, 2, Some(next))).unwrap();
    assert_eq!(
        second
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["action-1", "artifact-1"]
    );
    assert!(second.next_page.is_none());

    let stale = query(4, 2, first.next_page.clone());
    assert_eq!(
        project_ui_snapshot(&events, &stale).unwrap_err(),
        "ui_snapshot_source_cursor_mismatch"
    );
    let mut tampered = query(5, 2, None);
    tampered.after = Some(
        first
            .next_page
            .clone()
            .unwrap()
            .replace("epoch-1", "epoch-old"),
    );
    assert!(project_ui_snapshot(&events, &tampered).is_err());
}

#[test]
fn lag_retention_and_owner_boundaries_fail_closed() {
    let events = fixture_events();
    let mut lagging = query(5, 2, None);
    lagging.projection_cursor = Some(4);
    assert_eq!(
        project_ui_snapshot(&events, &lagging).unwrap_err(),
        "ui_snapshot_projection_lag"
    );
    let mut unknown = query(5, 2, None);
    unknown.projection_cursor = None;
    assert_eq!(
        project_ui_snapshot(&events, &unknown).unwrap_err(),
        "ui_snapshot_projection_lag_unknown"
    );
    let mut retained = fixture_events();
    retained.truncate(3);
    let mut retention = query(3, 2, None);
    retention.retention_floor = 3;
    assert_eq!(
        project_ui_snapshot(&retained, &retention).unwrap_err(),
        "ui_snapshot_retention_protected_pending"
    );

    let foreign = vec![event(
        1,
        "session.created",
        json!({"owner_id":"owner-2","session_id":"session-1"}),
    )];
    let mut foreign_query = query(1, 1, None);
    foreign_query.projection_cursor = Some(1);
    assert_eq!(
        project_ui_snapshot(&foreign, &foreign_query).unwrap_err(),
        "ui_snapshot_session_owner_mismatch"
    );
    assert!(json_digest(&json!({"fixture":"ui05"})).starts_with("sha256:"));
}
