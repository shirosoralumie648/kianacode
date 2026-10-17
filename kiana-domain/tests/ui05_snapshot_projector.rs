//! UI-05 Domain projector 的合成事件 fixture。
//!
//! 这些用例直接构造 RuntimeEvent 列表与 UiSnapshotQuery，验证单次切片分页、排序、游标范围、projection lag、
//! retention pending action 和跨 owner session 的拒绝行为。输入不是实际 EventStore，也不会启动 DaemonHost；
//! 因此它们证明纯 projector 合同，不证明某个界面已经调用 snapshot facade 或完成身份绑定。
use kiana_domain::{
    json_digest, project_ui_snapshot, project_ui_snapshot_health, rebuild_ui_snapshot_projection,
    RequestId, RuntimeEvent, UiSnapshotEntryKind, UiSnapshotPageCursor,
    UiSnapshotProjectorHealthQuery, UiSnapshotProjectorStatus, UiSnapshotQuery,
    UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE, UI_SNAPSHOT_LIMITATION_PROJECTION_LAGGING,
    UI_SNAPSHOT_LIMITATION_PROJECTION_UNKNOWN, UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR,
    UI_SNAPSHOT_MAX_PAGE_SIZE,
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

/// 构造与 [`query`] 同 owner/epoch 的 projector health 查询，只有 projection cursor 可变。
fn health_query(
    source_cursor: u64,
    projection_cursor: Option<u64>,
) -> UiSnapshotProjectorHealthQuery {
    UiSnapshotProjectorHealthQuery::new("epoch-1", source_cursor, projection_cursor, 1, 1, 0, 1)
        .unwrap()
}

#[test]
fn empty_slice_projects_an_empty_page_without_a_next_cursor() {
    // 空集合必须来自真实的空切片：合法的 source cursor 0 返回可渲染空页与自洽摘要。
    let page = project_ui_snapshot(&[], &query(0, 4, None)).unwrap();
    assert_eq!(page.snapshot_cursor, 0);
    assert!(page.entries.is_empty());
    assert!(page.next_page.is_none());
    assert!(page.limitations.is_empty());
    page.validate().expect("empty page is self-consistent");
    // 声明 source cursor 1 却没有事件是 cursor 拼接，不是「空一点的历史」。
    assert_eq!(
        project_ui_snapshot(&[], &query(1, 4, None)).unwrap_err(),
        "ui_snapshot_source_cursor_mismatch"
    );
}

#[test]
fn repeat_reads_and_cursor_replay_return_the_same_page_without_changing_facts() {
    let events = fixture_events();
    let facts_before = events.clone();
    let first = project_ui_snapshot(&events, &query(5, 3, None)).unwrap();
    let repeated = project_ui_snapshot(&events, &query(5, 3, None)).unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.snapshot_digest, repeated.snapshot_digest);

    // 游标回放：同一 next cursor 必须回到同一分页位置，事实切片逐字节不变。
    let next = first.next_page.clone().expect("second page");
    let second = project_ui_snapshot(&events, &query(5, 3, Some(next.clone()))).unwrap();
    assert_eq!(second.entries.len(), 1);
    assert_eq!(second.entries[0].id, "artifact-1");
    let replayed = project_ui_snapshot(&events, &query(5, 3, Some(next))).unwrap();
    assert_eq!(second.snapshot_digest, replayed.snapshot_digest);
    assert_eq!(events, facts_before);
}

#[test]
fn page_limitations_expose_partial_pages_and_the_retention_floor() {
    let events = fixture_events();
    // 首页只是完整结果的一部分：partial limitation 必须出现；末页不再有 partial。
    let first = project_ui_snapshot(&events, &query(5, 2, None)).unwrap();
    assert_eq!(
        first.limitations,
        vec![UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE.to_owned()]
    );
    let last = project_ui_snapshot(&events, &query(5, 2, first.next_page.clone())).unwrap();
    assert!(last.limitations.is_empty());

    // retention floor 改变了历史的可解释范围，必须作为稳定 limitation 暴露给调用方。
    let mut retained = query(5, 2, None);
    retained.retention_floor = 2;
    let retained_page = project_ui_snapshot(&events, &retained).unwrap();
    assert_eq!(
        retained_page.limitations,
        vec![
            UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE.to_owned(),
            UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR.to_owned(),
        ]
    );
    retained_page.validate().expect("limitations stay bounded");
}

#[test]
fn pagination_boundaries_and_illegal_cursors_fail_closed() {
    let events = fixture_events();

    // page size 边界：0 与 257 都不构成分页请求；上界值本身合法。
    let mut zero_page = query(5, 2, None);
    zero_page.page_size = 0;
    assert_eq!(
        zero_page.validate().unwrap_err(),
        "ui_snapshot_query_header_invalid"
    );
    let mut oversize_page = query(5, 2, None);
    oversize_page.page_size = UI_SNAPSHOT_MAX_PAGE_SIZE + 1;
    assert_eq!(
        oversize_page.validate().unwrap_err(),
        "ui_snapshot_query_header_invalid"
    );

    // 空 after 不是「首页」的别名。
    let mut empty_after = query(5, 2, None);
    empty_after.after = Some(String::new());
    assert_eq!(
        project_ui_snapshot(&events, &empty_after).unwrap_err(),
        "ui_snapshot_after_cursor_invalid"
    );
    // 非 JSON 与超大游标不能静默退回第一页。
    let mut garbage = query(5, 2, None);
    garbage.after = Some("not-a-cursor".to_owned());
    assert_eq!(
        project_ui_snapshot(&events, &garbage).unwrap_err(),
        "ui_snapshot_cursor_decode_failed"
    );
    let mut oversized = query(5, 2, None);
    oversized.after = Some("x".repeat(2_049));
    assert_eq!(
        project_ui_snapshot(&events, &oversized).unwrap_err(),
        "ui_snapshot_cursor_too_large"
    );

    // offset 超出本次 owner/session 结果长度：拒绝，不返回空页。
    let beyond = UiSnapshotPageCursor::new("epoch-1", 5, 5, 1)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        project_ui_snapshot(&events, &query(5, 2, Some(beyond))).unwrap_err(),
        "ui_snapshot_cursor_offset_invalid"
    );
    // 跨 source/generation/epoch 的游标会把两个切点拼成一个 snapshot，全部拒绝。
    let stale_generation = UiSnapshotPageCursor::new("epoch-1", 5, 1, 2)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        project_ui_snapshot(&events, &query(5, 2, Some(stale_generation))).unwrap_err(),
        "ui_snapshot_cursor_generation_mismatch"
    );
    let other_source = UiSnapshotPageCursor::new("epoch-1", 4, 1, 1)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        project_ui_snapshot(&events, &query(5, 2, Some(other_source))).unwrap_err(),
        "ui_snapshot_cursor_source_mismatch"
    );
    let other_epoch = UiSnapshotPageCursor::new("epoch-old", 5, 1, 1)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        project_ui_snapshot(&events, &query(5, 2, Some(other_epoch))).unwrap_err(),
        "ui_snapshot_cursor_source_mismatch"
    );

    // offset == 结果长度是合法的空尾页边界，不允许伪造 next_page 或 limitation。
    let tail = UiSnapshotPageCursor::new("epoch-1", 5, 4, 1)
        .unwrap()
        .encode()
        .unwrap();
    let tail_page = project_ui_snapshot(&events, &query(5, 2, Some(tail))).unwrap();
    assert!(tail_page.entries.is_empty());
    assert!(tail_page.next_page.is_none());
    assert!(tail_page.limitations.is_empty());
    assert_eq!(tail_page.snapshot_cursor, 5);
}

#[test]
fn projector_health_reports_lag_and_unknown_instead_of_an_empty_list() {
    let events = fixture_events();

    // 进度未知：必须是独立状态，不能被当作已追平或空结果。
    let unknown = project_ui_snapshot_health(&events, &health_query(5, None)).unwrap();
    assert_eq!(unknown.status, UiSnapshotProjectorStatus::Unknown);
    assert_eq!(unknown.projection_cursor, None);
    assert_eq!(unknown.lag, None);
    assert!(unknown
        .limitations
        .contains(&UI_SNAPSHOT_LIMITATION_PROJECTION_UNKNOWN.to_owned()));

    // 落后：lag 是显式数值，不能靠调用方自己数。
    let lagging = project_ui_snapshot_health(&events, &health_query(5, Some(2))).unwrap();
    assert_eq!(lagging.status, UiSnapshotProjectorStatus::Lagging);
    assert_eq!(lagging.lag, Some(3));
    assert!(lagging
        .limitations
        .contains(&UI_SNAPSHOT_LIMITATION_PROJECTION_LAGGING.to_owned()));

    // 追平：limitations 为空且摘要可自检。
    let current = project_ui_snapshot_health(&events, &health_query(5, Some(5))).unwrap();
    assert_eq!(current.status, UiSnapshotProjectorStatus::UpToDate);
    assert_eq!(current.lag, Some(0));
    assert!(current.limitations.is_empty());
    current.validate().expect("health is self-consistent");

    // 谎报处理位置超过 source：拒绝，不降级成「未知」。
    assert_eq!(
        UiSnapshotProjectorHealthQuery::new("epoch-1", 5, Some(6), 1, 1, 0, 1).unwrap_err(),
        "ui_snapshot_projection_cursor_ahead"
    );
    // source cursor 与切片长度不符：拒绝。
    let mismatch = UiSnapshotProjectorHealthQuery::new("epoch-1", 4, Some(4), 1, 1, 0, 1).unwrap();
    assert_eq!(
        project_ui_snapshot_health(&events, &mismatch).unwrap_err(),
        "ui_snapshot_source_cursor_mismatch"
    );

    // retention floor 也要进入 health limitation，避免把裁剪过的历史当完整。
    let retained = UiSnapshotProjectorHealthQuery::new("epoch-1", 5, Some(5), 1, 1, 2, 1).unwrap();
    let retained_health = project_ui_snapshot_health(&events, &retained).unwrap();
    assert!(retained_health
        .limitations
        .contains(&UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR.to_owned()));

    // 篡改 status 让其与 cursor/lag 不自洽时 validator 必须拒绝。
    let mut forged = current.clone();
    forged.status = UiSnapshotProjectorStatus::Unknown;
    assert_eq!(
        forged.validate().unwrap_err(),
        "ui_snapshot_health_status_inconsistent"
    );
}

#[test]
fn rebuild_matches_the_page_and_recovers_after_projection_faults() {
    let events = fixture_events();
    let facts_before = events.clone();
    let healthy = rebuild_ui_snapshot_projection(&events, &query(5, 2, None)).unwrap();
    let baseline_digest = healthy.page.snapshot_digest.clone();
    assert_eq!(healthy.health.status, UiSnapshotProjectorStatus::UpToDate);
    assert_eq!(healthy.health.source_cursor, healthy.page.source_cursor);
    healthy.validate().expect("rebuild is self-consistent");

    // 故障：lag 与未知进度都必须让重建失败，不返回半新半旧的组合。
    let mut lagging = query(5, 2, None);
    lagging.projection_cursor = Some(4);
    assert_eq!(
        rebuild_ui_snapshot_projection(&events, &lagging).unwrap_err(),
        "ui_snapshot_projection_lag"
    );
    let mut unknown = query(5, 2, None);
    unknown.projection_cursor = None;
    assert_eq!(
        rebuild_ui_snapshot_projection(&events, &unknown).unwrap_err(),
        "ui_snapshot_projection_lag_unknown"
    );

    // 恢复：同一事实与同一游标重建出与故障前完全一致的页面摘要。
    let recovered = rebuild_ui_snapshot_projection(&events, &query(5, 2, None)).unwrap();
    assert_eq!(recovered.page.snapshot_digest, baseline_digest);
    assert_eq!(recovered.rebuild_digest, healthy.rebuild_digest);
    assert_eq!(events, facts_before);

    // rebuild 摘要绑定 page 与 health：篡改任一子摘要都必须被拒绝。
    let mut forged = recovered.clone();
    forged.page.snapshot_digest = json_digest(&json!({"forged": true}));
    assert_eq!(
        forged.validate().unwrap_err(),
        "ui_snapshot_digest_mismatch"
    );
    let mut forged = recovered.clone();
    forged.health.health_digest = json_digest(&json!({"forged": true}));
    assert_eq!(
        forged.validate().unwrap_err(),
        "ui_snapshot_health_digest_mismatch"
    );
}
