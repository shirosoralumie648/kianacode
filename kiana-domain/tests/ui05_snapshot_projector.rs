//! UI-05 Domain projector 的合成事件 fixture。
//!
//! 这些用例直接构造 RuntimeEvent 列表与 UiSnapshotQuery，验证单次切片分页、排序、游标范围、projection lag、
//! retention pending action 和跨 owner session 的拒绝行为。输入不是实际 EventStore，也不会启动 DaemonHost；
//! 因此它们证明纯 projector 合同，不证明某个界面已经调用 snapshot facade 或完成身份绑定。
use kiana_domain::{
    json_digest, project_ui_snapshot, RequestId, RuntimeEvent, UiSnapshotEntryKind, UiSnapshotQuery,
};
use serde_json::json;

/// 构造带新 RequestId 的事件，并保持测试传入的 sequence 与 kind/data 原样进入 RuntimeEvent。
fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

/// 构造一个 owner/session 的事实样例：session、run、action 接受/完成状态和 artifact。
/// Action 事件还写入聚合 stream metadata，用于验证 projector 优先采用 aggregate_id。
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

/// 构造 owner=session-1 的查询；默认 projection cursor 与 source cursor 相同，表示 read model 已追平。
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
    // 一次五事件切片被稳定排序后按 2 条分页：类别顺序先 Session、Run，再 Action、Artifact。
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

    // 续页游标绑定 source cursor=5；不能拿到长度为 5 的输入却声称读取位置是 4。
    let stale = query(4, 2, first.next_page.clone());
    assert_eq!(
        project_ui_snapshot(&events, &stale).unwrap_err(),
        "ui_snapshot_source_cursor_mismatch"
    );
    // 修改已编码游标的 epoch 会让其自摘要/Query 绑定校验失败，而不会退回第一页。
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
    // 先验证 read model 落后和进度未知都明确拒绝，不返回看似空或完整的 page。
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
    // 把尚未看到终态的 accepted action 留在 retention floor 之前，必须保护这条未完成操作。
    let mut retained = fixture_events();
    retained.truncate(3);
    let mut retention = query(3, 2, None);
    retention.retention_floor = 3;
    assert_eq!(
        project_ui_snapshot(&retained, &retention).unwrap_err(),
        "ui_snapshot_retention_protected_pending"
    );

    // 指定 session 若在输入事实里属于另一个 owner，整页失败，避免把冲突当普通过滤后成功返回。
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
