//! OA-17 fixtures: cursor/snapshot/paging boundaries for the audit query route.
//!
//! The domain guards are covered first, then the real `ControlPlane::query_audit` route is
//! exercised against a read-only fixture store so that lag, stale, revoked and oversized scans
//! are rejected before any caller sees a "complete looking" page.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{AuditProjectionError, AuditQueryInput, ControlPlane};
use kiana_domain::{
    ApprovalId, AuditActionKind, AuditDecision, AuditQueryCursor, EventLogCursorSnapshot,
    EventStoreCapabilities, ExecutionStatus, RequestContext, RequestId, RuntimeEvent,
};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::Arc;

fn filter_digest() -> String {
    kiana_domain::json_digest(&serde_json::json!({
        "action_kind": "capability",
        "decision": "denied",
        "target_kind": "capability",
    }))
}

#[test]
fn cursor_binds_epoch_projection_source_and_filter_digest() {
    let cursor = AuditQueryCursor::new("epoch:one", 1, 10, 4, filter_digest()).unwrap();
    cursor.validate().unwrap();
    let encoded = serde_json::to_string(&cursor).unwrap();
    assert_eq!(
        serde_json::from_str::<AuditQueryCursor>(&encoded).unwrap(),
        cursor
    );
    let mut forged = cursor.clone();
    forged.after_cursor = cursor.source_cursor + 1;
    assert_eq!(
        forged.validate().unwrap_err(),
        "audit_query_cursor_out_of_range"
    );
    let mut bad_digest = cursor;
    bad_digest.filter_digest =
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    assert_eq!(
        bad_digest.validate().unwrap_err(),
        "audit_query_cursor_digest_mismatch"
    );
}

#[test]
fn query_input_rejects_cursor_field_mismatch_and_unbounded_page() {
    let cursor = AuditQueryCursor::new("epoch:one", 1, 10, 4, filter_digest()).unwrap();
    let mut query = AuditQueryInput {
        source_cursor: Some(10),
        after_cursor: 0,
        limit: 10,
        action_kind: Some(AuditActionKind::Capability),
        decision: Some(AuditDecision::Denied),
        target_kind: Some("capability".to_owned()),
        cursor: Some(cursor.clone()),
    };
    query.validate().unwrap();
    query.after_cursor = 3;
    assert_eq!(
        query.validate().unwrap_err(),
        AuditProjectionError::QueryCursorInvalid
    );
    query.after_cursor = 0;
    query.source_cursor = Some(9);
    assert_eq!(
        query.validate().unwrap_err(),
        AuditProjectionError::QueryCursorInvalid
    );
    query.source_cursor = Some(10);
    query.limit = 1_001;
    assert_eq!(
        query.validate().unwrap_err(),
        AuditProjectionError::QueryLimitInvalid
    );
    let mut stale = cursor;
    stale.projection_version = 2;
    stale.cursor_digest = stale.digest();
    stale.validate().unwrap();
}

#[test]
fn cursor_snapshot_rejects_forged_bounds_and_digests() {
    let snapshot = EventLogCursorSnapshot::new(1, 3, 3).unwrap();
    assert_eq!(snapshot.retained_event_count(), 3);
    snapshot.validate().unwrap();
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert_eq!(
        serde_json::from_str::<EventLogCursorSnapshot>(&encoded).unwrap(),
        snapshot
    );

    // 空日志必须把保留前缀报告为 1，而不是 0。
    let empty = EventLogCursorSnapshot::new(1, 0, 0).unwrap();
    assert_eq!(empty.retained_event_count(), 0);
    assert_eq!(
        EventLogCursorSnapshot::new(0, 0, 0).unwrap_err(),
        "eventlog_cursor_snapshot_metadata_required"
    );
    // 持久化游标不能越过已提交边界。
    assert_eq!(
        EventLogCursorSnapshot::new(1, 2, 3).unwrap_err(),
        "eventlog_cursor_snapshot_durable_ahead"
    );
    // 保留前缀不能位于已提交边界之后。
    assert_eq!(
        EventLogCursorSnapshot::new(4, 3, 3).unwrap_err(),
        "eventlog_cursor_snapshot_retention_invalid"
    );

    let mut forged = serde_json::to_value(&snapshot).unwrap();
    forged["snapshot_digest"] = serde_json::json!(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    let forged: EventLogCursorSnapshot = serde_json::from_value(forged).unwrap();
    assert_eq!(
        forged.validate().unwrap_err(),
        "eventlog_cursor_snapshot_digest_mismatch"
    );
}

// ---------------------------------------------------------------------------
// ControlPlane 级夹具：受控事件日志 + 已提交游标快照
// ---------------------------------------------------------------------------

const RUN_ID: &str = "11111111-1111-4111-8111-111111111111";

fn event(sequence: u64, kind: &str, source_cursor: u64, actor: &str) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        kind,
        serde_json::json!({
            "run_id": RUN_ID,
            "actor_id": actor,
            "session_id": "session-1",
            "project_root": "/repo",
            "authority_epoch": 1,
            "data_epoch": 1,
            "source_cursor": source_cursor,
        }),
    )
    .unwrap()
}

fn run_events() -> Vec<RuntimeEvent> {
    vec![
        event(1, "run.authorized", 1, "local-user"),
        event(2, "run.completed", 2, "local-user"),
        event(3, "run.failed", 3, "local-user"),
    ]
}

fn bounded_snapshot() -> EventLogCursorSnapshot {
    EventLogCursorSnapshot::new(1, 3, 3).unwrap()
}

/// 只读夹具：事件内容与 adapter 自报的已提交快照可以分别注入。
struct SnapshotStore {
    events: Vec<RuntimeEvent>,
    snapshot: Option<EventLogCursorSnapshot>,
}

impl SnapshotStore {
    fn new(events: Vec<RuntimeEvent>, snapshot: Option<EventLogCursorSnapshot>) -> Self {
        Self { events, snapshot }
    }
}

#[async_trait]
impl EventStorePort for SnapshotStore {
    fn capabilities(&self) -> EventStoreCapabilities {
        EventStoreCapabilities::default()
    }

    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Err(PortError::Unavailable("oa17_fixture_read_only".to_owned()))
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self
            .events
            .iter()
            .filter(|event| event.request_id == *request_id)
            .cloned()
            .collect())
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.events.clone())
    }

    async fn cursor_snapshot(&self) -> Result<EventLogCursorSnapshot, PortError> {
        self.snapshot.clone().ok_or_else(|| {
            PortError::Unavailable("event_store_cursor_snapshot_unsupported".to_owned())
        })
    }
}

/// 全量读取不支持的存储：查询必须保留 unavailable，而不是返回空页。
struct UnsupportedReadStore;

#[async_trait]
impl EventStorePort for UnsupportedReadStore {
    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Err(PortError::Unavailable("oa17_fixture_read_only".to_owned()))
    }

    async fn read_request(&self, _request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(Vec::new())
    }
}

struct UnusedApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnusedApprovalStore {
    /// 查询夹具不消费审批；若被调用必须 fail closed，而不是伪造成功。
    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "oa17_fixture_approval_unused".to_owned(),
        ))
    }
}

struct UnusedRunner;

#[async_trait]
impl RunnerPort for UnusedRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable(
            "oa17_fixture_runner_unused".to_owned(),
        ))
    }
}

fn query_core(store: Arc<dyn EventStorePort>) -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        store,
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnusedApprovalStore),
        Arc::new(UnusedRunner),
    )
}

fn context(actor: &str) -> RequestContext {
    let mut context = RequestContext::local("session-1", "/repo");
    context.actor_id = Some(actor.to_owned());
    context
}

fn query(cursor: Option<AuditQueryCursor>, limit: usize) -> AuditQueryInput {
    AuditQueryInput {
        source_cursor: None,
        after_cursor: 0,
        limit,
        action_kind: None,
        decision: None,
        target_kind: None,
        cursor,
    }
}

// ---------------------------------------------------------------------------
// 先拒绝：滞后、超前、越界、撤销、伪造快照、超限扫描
// ---------------------------------------------------------------------------

#[tokio::test]
async fn query_rejects_projection_lag_and_snapshot_ahead() {
    // 投影滞后：adapter 已提交到 3，但本次读取只有 2 条事件。
    let lag = query_core(Arc::new(SnapshotStore::new(
        run_events()[..2].to_vec(),
        Some(bounded_snapshot()),
    )));
    let error = lag
        .query_audit(&context("local-user"), query(None, 10))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "port_unavailable:audit_query_projection_lag"
    );

    // 读取越过了快照边界（查询期间的新提交/重放）：必须要求重试，而不是返回过期页。
    let ahead = query_core(Arc::new(SnapshotStore::new(
        run_events(),
        Some(EventLogCursorSnapshot::new(1, 2, 2).unwrap()),
    )));
    let error = ahead
        .query_audit(&context("local-user"), query(None, 10))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "port_conflict:audit_query_snapshot_stale"
    );
}

#[tokio::test]
async fn query_rejects_out_of_range_and_revoked_page_cursors() {
    let core = query_core(Arc::new(SnapshotStore::new(
        run_events(),
        Some(bounded_snapshot()),
    )));
    let mut out_of_range = query(None, 10);
    out_of_range.after_cursor = 4;
    let error = core
        .query_audit(&context("local-user"), out_of_range)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "port_conflict:audit_query_cursor_invalid"
    );

    // 保留策略撤销：游标所在的记录已不在保留前缀内，继续翻页会跨过缺口。
    let revoked = query_core(Arc::new(SnapshotStore::new(
        run_events(),
        Some(EventLogCursorSnapshot::new(5, 8, 8).unwrap()),
    )));
    let mut revoked_query = query(None, 10);
    revoked_query.after_cursor = 2;
    let error = revoked
        .query_audit(&context("local-user"), revoked_query)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "port_conflict:audit_query_cursor_revoked"
    );
}

#[tokio::test]
async fn query_rejects_a_forged_eventlog_cursor_snapshot() {
    let mut forged = serde_json::to_value(bounded_snapshot()).unwrap();
    forged["snapshot_digest"] = serde_json::json!(
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    let snapshot: EventLogCursorSnapshot = serde_json::from_value(forged).unwrap();
    let core = query_core(Arc::new(SnapshotStore::new(run_events(), Some(snapshot))));

    let error = core
        .query_audit(&context("local-user"), query(None, 10))
        .await
        .unwrap_err();
    let reason = error.to_string();
    assert!(
        reason.contains("eventlog_cursor_snapshot_invalid"),
        "unexpected reason: {reason}"
    );
    assert!(
        reason.contains("eventlog_cursor_snapshot_digest_mismatch"),
        "unexpected reason: {reason}"
    );
}

#[tokio::test]
async fn oversized_scan_is_rejected_without_truncating_the_page() {
    let events = (1..=257)
        .map(|sequence| event(sequence, "run.authorized", sequence, "local-user"))
        .collect::<Vec<_>>();
    let core = query_core(Arc::new(SnapshotStore::new(events, None)));

    let error = core
        .query_audit(&context("local-user"), query(None, 10))
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "port_unavailable:audit_query_scan_limit");
}

// ---------------------------------------------------------------------------
// 成功与回归：显式降级、空页 vs unavailable、重连与多客户端一致
// ---------------------------------------------------------------------------

#[tokio::test]
async fn query_without_cursor_snapshot_degrades_explicitly_and_reports_scan() {
    let core = query_core(Arc::new(SnapshotStore::new(run_events(), None)));
    let response = core
        .query_audit(&context("local-user"), query(None, 10))
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["records"].as_array().unwrap().len(), 3);
    assert!(response.output["snapshot"].is_null());
    let scan = &response.output["scan"];
    assert_eq!(scan["scanned_events"], 3);
    assert_eq!(scan["projected_records"], 3);
    assert_eq!(scan["scoped_records"], 3);
    assert_eq!(scan["matched_records"], 3);
    assert_eq!(scan["page_records"], 3);
    assert_eq!(scan["scan_class"], "bounded");
    let limitations: Vec<String> =
        serde_json::from_value(response.output["limitations"].clone()).unwrap();
    assert!(
        limitations.contains(&"eventlog_cursor_snapshot_unsupported".to_owned()),
        "unexpected limitations: {limitations:?}"
    );
}

#[tokio::test]
async fn empty_page_and_unavailable_projection_stay_distinct() {
    // 无匹配记录：Completed + 空页，next_cursor 为空。
    let core = query_core(Arc::new(SnapshotStore::new(
        run_events(),
        Some(bounded_snapshot()),
    )));
    let empty = core
        .query_audit(&context("foreign-user"), query(None, 10))
        .await
        .unwrap();
    assert_eq!(empty.status, ExecutionStatus::Completed);
    assert_eq!(empty.output["records"].as_array().unwrap().len(), 0);
    assert!(empty.output["next_cursor"].is_null());
    assert_eq!(empty.output["scan"]["scoped_records"], 0);

    // 投影不可用：读取失败必须保留错误，不能伪装成空页。
    let unavailable = query_core(Arc::new(UnsupportedReadStore));
    let error = unavailable
        .query_audit(&context("local-user"), query(None, 10))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "port_unavailable:audit_query_read_all_unsupported"
    );
}

#[tokio::test]
async fn page_binding_is_stable_across_reconnect_and_clients() {
    let core = query_core(Arc::new(SnapshotStore::new(
        run_events(),
        Some(bounded_snapshot()),
    )));

    let first = core
        .query_audit(&context("local-user"), query(None, 1))
        .await
        .unwrap();
    let first_records = first.output["records"].as_array().unwrap();
    assert_eq!(first_records.len(), 1);
    assert_eq!(first_records[0]["source_cursor"], 1);
    let next: AuditQueryCursor =
        serde_json::from_value(first.output["next_cursor"].clone()).unwrap();

    // 重连或另一个 tab 用同一个 next cursor：结果与下一页游标必须一致。
    let second = core
        .query_audit(&context("local-user"), query(Some(next.clone()), 1))
        .await
        .unwrap();
    let repeated = core
        .query_audit(&context("local-user"), query(Some(next), 1))
        .await
        .unwrap();
    assert_eq!(second.output["records"], repeated.output["records"]);
    assert_eq!(second.output["next_cursor"], repeated.output["next_cursor"]);
    let second_records = second.output["records"].as_array().unwrap();
    assert_eq!(second_records.len(), 1);
    assert_eq!(second_records[0]["source_cursor"], 2);
    let second_next: AuditQueryCursor =
        serde_json::from_value(second.output["next_cursor"].clone()).unwrap();

    // 游标不携带 owner/scope：另一个主体复用它只会看到自己的（空）范围。
    let other = core
        .query_audit(&context("foreign-user"), query(Some(second_next), 1))
        .await
        .unwrap();
    assert_eq!(other.output["records"].as_array().unwrap().len(), 0);
    assert!(other.output["next_cursor"].is_null());
}

#[tokio::test]
async fn cursor_from_an_older_projection_is_rejected_as_stale() {
    let first_core = query_core(Arc::new(SnapshotStore::new(
        run_events()[..2].to_vec(),
        Some(EventLogCursorSnapshot::new(1, 2, 2).unwrap()),
    )));
    let page = first_core
        .query_audit(&context("local-user"), query(None, 1))
        .await
        .unwrap();
    let next: AuditQueryCursor =
        serde_json::from_value(page.output["next_cursor"].clone()).unwrap();

    // 重连时日志已经增长：旧游标必须 fail closed，而不是静默跳到新记录之后。
    let grown = query_core(Arc::new(SnapshotStore::new(
        run_events(),
        Some(bounded_snapshot()),
    )));
    let error = grown
        .query_audit(&context("local-user"), query(Some(next), 1))
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "port_conflict:audit_query_cursor_stale");
}
