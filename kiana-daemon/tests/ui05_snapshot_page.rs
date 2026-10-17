//! UI-05 daemon 行为夹具：真实 `DaemonHost` 上的只读 snapshot、projector health 与 read-model rebuild。
//!
//! 这些用例把 Domain projector 接到组合根，验证的唯一路径是：
//!
//! ```text
//! 已提交事实（MemoryEventLog / JsonlEventLog）
//!     -> DaemonHost::persisted_events()            单次读取、单一 cursor
//!     -> DaemonHost::ui_snapshot_page()            原子页 + 分页 cursor + limitations
//!     -> DaemonHost::ui_snapshot_projector_health() lag/unknown 不得伪装成空列表
//!     -> DaemonHost::rebuild_ui_snapshot_projection() read-model rebuild（页 + health 同一游标）
//! ```
//!
//! 先拒绝：读源不可用、projection lag/未知、越权 session、被 retention 保护的 pending action、
//! 跨 source/epoch/generation 的拼接游标都必须失败关闭，且不得被渲染成空结果。
//!
//! 这里不启动 UI、不执行 effect、不调用模型；`MemoryEventLog` 是进程内夹具，`JsonlEventLog` 是单
//! 文件夹具，所以证明上限是 source/local 行为：新宿主实例重开同一账本不等于真实跨进程恢复。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_daemon::DaemonHost;
use kiana_domain::{
    RequestId, RuntimeEvent, UiSnapshotEntryKind, UiSnapshotProjectorHealthQuery,
    UiSnapshotProjectorStatus, UiSnapshotQuery, UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE,
    UI_SNAPSHOT_LIMITATION_PROJECTION_LAGGING, UI_SNAPSHOT_LIMITATION_PROJECTION_UNKNOWN,
    UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 这些只读用例不触发任何审批；缺失的审批端口必须明确不可用，而不是自动放行。
struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &kiana_domain::RequestContext,
        _: kiana_domain::CapabilityRequest,
        _: &str,
    ) -> Result<kiana_domain::ApprovalChallenge, PortError> {
        Err(PortError::Unavailable(
            "ui05_fixture_no_approval".to_owned(),
        ))
    }

    async fn activate(&self, _: kiana_domain::ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "ui05_fixture_no_approval".to_owned(),
        ))
    }

    async fn consume(
        &self,
        _: &kiana_domain::RequestContext,
        _: kiana_domain::ApprovalId,
    ) -> Result<kiana_domain::PendingApproval, PortError> {
        Err(PortError::Unavailable(
            "ui05_fixture_no_approval".to_owned(),
        ))
    }
}

/// snapshot 投影不得把工具调用当副作用执行；runner 被调用即测试失败。
struct NoRunner;

#[async_trait]
impl RunnerPort for NoRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("ui05_runner_unused".to_owned()))
    }

    async fn restore(
        &self,
        _run_id: kiana_domain::RunId,
        _checkpoint: Value,
    ) -> Result<(), PortError> {
        Err(PortError::Unavailable("ui05_runner_unused".to_owned()))
    }
}

/// 只声明兼容 append 的存储：`read_all` 走默认实现返回 `event_store_read_all_unsupported`，
/// 用来证明「读不到投影源」不会被当成「没有待办的空列表」。
struct UnsupportedReadAllStore;

#[async_trait]
impl EventStorePort for UnsupportedReadAllStore {
    async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "ui05_fixture_append_unsupported".to_owned(),
        ))
    }
}

/// 组装组合根；快照读路径只经过 `persisted_events()`，不打开第二条执行循环。
fn host_with(store: Arc<dyn EventStorePort>) -> DaemonHost {
    let core = Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        store,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        Arc::new(NoRunner),
    ));
    DaemonHost::new(core)
}

fn event(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

/// owner/session 事实切片：session、run、接受并完成的 action、artifact、receipt。
///
/// `session.assigned` / `run.authorized` 使用已登记事件契约的字段白名单；`ui.action.*` 与 artifact/
/// receipt 目前是未登记的最小投影事实，因此只被 projector 的字段规则识别。
fn fixture_events() -> Vec<RuntimeEvent> {
    let mut action_accepted = event(
        3,
        "ui.action.accepted",
        json!({
            "owner_id": "local-user",
            "session_id": "session-1",
            "action": {"owner_id": "local-user", "session_id": "session-1"},
            "record": {"state": "accepted"},
            "state": "accepted",
        }),
    )
    .with_stream_metadata("ui_action", "action-1", 1);
    action_accepted.idempotency_key = Some("ui-action:action-1:accepted".to_owned());
    let action_applied = event(
        4,
        "ui.action.applied",
        json!({
            "owner_id": "local-user",
            "session_id": "session-1",
            "action": {"owner_id": "local-user", "session_id": "session-1"},
            "record": {"state": "applied"},
            "state": "applied",
        }),
    )
    .with_stream_metadata("ui_action", "action-1", 2);
    vec![
        event(
            1,
            "session.assigned",
            json!({"session_id": "session-1", "actor_id": "local-user"}),
        ),
        event(
            2,
            "run.authorized",
            json!({"run_id": "run-1", "session_id": "session-1", "actor_id": "local-user"}),
        ),
        action_accepted,
        action_applied,
        event(
            5,
            "ui.artifact.recorded",
            json!({"owner_id": "local-user", "session_id": "session-1", "artifact_id": "artifact-1"}),
        ),
        event(
            6,
            "ui.receipt.issued",
            json!({"owner_id": "local-user", "session_id": "session-1", "receipt_id": "receipt-1"}),
        ),
    ]
}

/// 只有「接受但未终态」的 action：用于 retention 保护拒绝。
fn pending_action_events() -> Vec<RuntimeEvent> {
    let mut accepted = event(
        3,
        "ui.action.accepted",
        json!({
            "owner_id": "local-user",
            "session_id": "session-1",
            "action": {"owner_id": "local-user", "session_id": "session-1"},
            "state": "accepted",
        }),
    )
    .with_stream_metadata("ui_action", "action-1", 1);
    accepted.idempotency_key = Some("ui-action:action-1:accepted".to_owned());
    vec![
        event(
            1,
            "session.assigned",
            json!({"session_id": "session-1", "actor_id": "local-user"}),
        ),
        event(
            2,
            "run.authorized",
            json!({"run_id": "run-1", "session_id": "session-1", "actor_id": "local-user"}),
        ),
        accepted,
    ]
}

async fn store_with(events: Vec<RuntimeEvent>) -> Arc<MemoryEventLog> {
    let store = Arc::new(MemoryEventLog::new());
    let port: Arc<dyn EventStorePort> = store.clone();
    for event in events {
        port.append(event).await.expect("append ui05 fixture fact");
    }
    store
}

/// 默认查询：owner/epoch/generation 固定，projection cursor 与 source 相同（read model 已追平）。
fn query(source_cursor: u64, page_size: usize, after: Option<String>) -> UiSnapshotQuery {
    UiSnapshotQuery::new(
        "local-user",
        Some("session-1".to_owned()),
        "epoch-1",
        source_cursor,
        Some(source_cursor),
        1,
        1,
        0,
        page_size,
        after,
        1_700_000_000_000,
    )
    .unwrap()
}

fn health_query(
    source_cursor: u64,
    projection_cursor: Option<u64>,
) -> UiSnapshotProjectorHealthQuery {
    UiSnapshotProjectorHealthQuery::new("epoch-1", source_cursor, projection_cursor, 1, 1, 0, 1)
        .unwrap()
}

fn temp_log_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("kiana-ui05-{stamp}"))
        .join("events.jsonl")
}

#[tokio::test]
async fn ui_snapshot_page_projects_one_owner_scoped_slice_and_paginates_with_limitations() {
    let host = host_with(store_with(fixture_events()).await);
    let events = host.persisted_events().await.unwrap().unwrap();
    assert_eq!(events.len(), 6);

    let first = host.ui_snapshot_page(&query(6, 3, None)).await.unwrap();
    assert_eq!(first.snapshot_cursor, 6);
    assert_eq!(first.entries.len(), 3);
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| entry.kind)
            .collect::<Vec<_>>(),
        vec![
            UiSnapshotEntryKind::Session,
            UiSnapshotEntryKind::Run,
            UiSnapshotEntryKind::Action
        ]
    );
    // 首屏只是完整结果的一页：partial limitation 与 next cursor 必须同时出现。
    assert_eq!(
        first.limitations,
        vec![UI_SNAPSHOT_LIMITATION_PARTIAL_PAGE.to_owned()]
    );
    let next = first.next_page.clone().expect("second page");

    let second = host
        .ui_snapshot_page(&query(6, 3, Some(next.clone())))
        .await
        .unwrap();
    assert_eq!(
        second
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec!["artifact-1", "receipt-1"]
    );
    assert!(second.next_page.is_none());
    assert!(second.limitations.is_empty());

    // 重复读取与游标回放：同一 query 与同一事实必须得到同一页和同一摘要。
    let repeated = host.ui_snapshot_page(&query(6, 3, None)).await.unwrap();
    assert_eq!(repeated, first);
    let replayed = host
        .ui_snapshot_page(&query(6, 3, Some(next)))
        .await
        .unwrap();
    assert_eq!(replayed.snapshot_digest, second.snapshot_digest);

    // 投影只读：事实数量与内容都不因 snapshot 读取而改变。
    let after = host.persisted_events().await.unwrap().unwrap();
    assert_eq!(after, events);
}

#[tokio::test]
async fn ui_snapshot_page_refuses_lag_foreign_session_retention_and_stitched_cursors() {
    let host = host_with(store_with(fixture_events()).await);

    // projection lag / 未知进度：拒绝，而不是返回看似完整的空页。
    let mut lagging = query(6, 3, None);
    lagging.projection_cursor = Some(5);
    let error = host.ui_snapshot_page(&lagging).await.unwrap_err();
    assert!(
        error.to_string().contains("ui_snapshot_projection_lag"),
        "unexpected error: {error}"
    );
    let mut unknown = query(6, 3, None);
    unknown.projection_cursor = None;
    let error = host.ui_snapshot_page(&unknown).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ui_snapshot_projection_lag_unknown"),
        "unexpected error: {error}"
    );

    // 跨 cursor 拼接：cursor 绑定的 source/generation 与本次读取不一致时整页拒绝。
    let stitched = stitched_cursor("epoch-1", 6, 3, 2);
    let error = host
        .ui_snapshot_page(&query(6, 3, Some(stitched)))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ui_snapshot_cursor_generation_mismatch"),
        "unexpected error: {error}"
    );
    let other_source = stitched_cursor("epoch-1", 5, 3, 1);
    let error = host
        .ui_snapshot_page(&query(6, 3, Some(other_source)))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ui_snapshot_cursor_source_mismatch"),
        "unexpected error: {error}"
    );
    let other_epoch = stitched_cursor("epoch-old", 6, 3, 1);
    let error = host
        .ui_snapshot_page(&query(6, 3, Some(other_epoch)))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ui_snapshot_cursor_source_mismatch"),
        "unexpected error: {error}"
    );

    // 指定 session 在事实里属于另一个 owner：整页拒绝，不能只过滤后当作成功。
    let mut foreign = fixture_events();
    foreign.push(event(
        7,
        "ui.session.observed",
        json!({"owner_id": "owner-2", "session_id": "session-1"}),
    ));
    let host = host_with(store_with(foreign).await);
    let error = host.ui_snapshot_page(&query(7, 3, None)).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ui_snapshot_session_owner_mismatch"),
        "unexpected error: {error}"
    );

    // retention floor 之前仍有未终态 action：拒绝删掉这条待办，也不返回空待办列表。
    let pending_host = host_with(store_with(pending_action_events()).await);
    let mut retention = query(3, 3, None);
    retention.retention_floor = 3;
    let error = pending_host.ui_snapshot_page(&retention).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("ui_snapshot_retention_protected_pending"),
        "unexpected error: {error}"
    );

    // 终态出现后同一游标可以恢复：retention limitation 仍可见，action 已是终态。
    let mut resolved = pending_action_events();
    resolved.push(
        event(
            4,
            "ui.action.applied",
            json!({
                "owner_id": "local-user",
                "session_id": "session-1",
                "action": {"owner_id": "local-user", "session_id": "session-1"},
                "state": "applied",
            }),
        )
        .with_stream_metadata("ui_action", "action-1", 2),
    );
    let resolved_host = host_with(store_with(resolved).await);
    let mut recovered = query(4, 3, None);
    recovered.retention_floor = 4;
    let page = resolved_host
        .ui_snapshot_page(&recovered)
        .await
        .expect("terminal action unblocks retention");
    assert!(page
        .limitations
        .contains(&UI_SNAPSHOT_LIMITATION_RETENTION_FLOOR.to_owned()));
    assert!(page
        .entries
        .iter()
        .any(|entry| entry.kind == UiSnapshotEntryKind::Action));
}

#[tokio::test]
async fn ui_snapshot_page_fails_closed_when_the_projection_source_cannot_be_read() {
    let store: Arc<dyn EventStorePort> = Arc::new(UnsupportedReadAllStore);
    let host = host_with(store);

    // 「不支持全量读取」必须报结构化失败，而不是空列表；health 走同一规则。
    let error = host.ui_snapshot_page(&query(0, 3, None)).await.unwrap_err();
    assert!(
        error.to_string().contains("ui_snapshot_projection_unknown"),
        "unexpected error: {error}"
    );
    let error = host
        .ui_snapshot_projector_health(&health_query(0, Some(0)))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("ui_snapshot_projection_unknown"),
        "unexpected error: {error}"
    );
    let error = host
        .rebuild_ui_snapshot_projection(&query(0, 3, None))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("ui_snapshot_projection_unknown"),
        "unexpected error: {error}"
    );

    // 真正空的账本与「读不到」不同：source cursor 0 时返回合法空页。
    let empty_host = host_with(store_with(Vec::new()).await);
    let page = empty_host
        .ui_snapshot_page(&query(0, 3, None))
        .await
        .unwrap();
    assert!(page.entries.is_empty());
    assert!(page.next_page.is_none());
    let health = empty_host
        .ui_snapshot_projector_health(&health_query(0, Some(0)))
        .await
        .unwrap();
    assert_eq!(health.status, UiSnapshotProjectorStatus::UpToDate);
    assert_eq!(health.source_cursor, 0);
}

#[tokio::test]
async fn projector_health_reports_lag_and_rebuild_matches_the_page_for_the_same_cursor() {
    let host = host_with(store_with(fixture_events()).await);

    let current = host
        .ui_snapshot_projector_health(&health_query(6, Some(6)))
        .await
        .unwrap();
    assert_eq!(current.status, UiSnapshotProjectorStatus::UpToDate);
    assert_eq!(current.lag, Some(0));
    assert!(current.limitations.is_empty());

    let lagging = host
        .ui_snapshot_projector_health(&health_query(6, Some(4)))
        .await
        .unwrap();
    assert_eq!(lagging.status, UiSnapshotProjectorStatus::Lagging);
    assert_eq!(lagging.lag, Some(2));
    assert!(lagging
        .limitations
        .contains(&UI_SNAPSHOT_LIMITATION_PROJECTION_LAGGING.to_owned()));

    let unknown = host
        .ui_snapshot_projector_health(&health_query(6, None))
        .await
        .unwrap();
    assert_eq!(unknown.status, UiSnapshotProjectorStatus::Unknown);
    assert_eq!(unknown.lag, None);
    assert!(unknown
        .limitations
        .contains(&UI_SNAPSHOT_LIMITATION_PROJECTION_UNKNOWN.to_owned()));

    // rebuild 是显式的 read-model 重建：同一游标下页面与页面投影一致，health 已追平。
    let page = host.ui_snapshot_page(&query(6, 3, None)).await.unwrap();
    let rebuild = host
        .rebuild_ui_snapshot_projection(&query(6, 3, None))
        .await
        .unwrap();
    assert_eq!(rebuild.page, page);
    assert_eq!(rebuild.health.status, UiSnapshotProjectorStatus::UpToDate);
    assert_eq!(rebuild.health.source_cursor, page.source_cursor);
    rebuild.validate().expect("rebuild stays self-consistent");

    // 有故障时 rebuild 与页面投影一样失败，不返回半新半旧的组合。
    let mut lagging_query = query(6, 3, None);
    lagging_query.projection_cursor = Some(5);
    let error = host
        .rebuild_ui_snapshot_projection(&lagging_query)
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("ui_snapshot_projection_lag"),
        "unexpected error: {error}"
    );

    // 故障之后同一事实重建必须得到同一摘要（恢复读路径不产生副作用）。
    let recovered = host
        .rebuild_ui_snapshot_projection(&query(6, 3, None))
        .await
        .unwrap();
    assert_eq!(recovered.rebuild_digest, rebuild.rebuild_digest);
}

#[tokio::test]
async fn rebuild_from_a_restarted_host_over_the_same_ledger_returns_the_same_page() {
    let path: PathBuf = temp_log_path();
    let store = Arc::new(JsonlEventLog::open(&path).expect("open ui05 fixture ledger"));
    {
        // 追加句柄先于宿主释放，保证「重开同一账本」时没有旧句柄并存。
        let port: Arc<dyn EventStorePort> = store.clone();
        for event in fixture_events() {
            port.append(event).await.expect("append ui05 fact");
        }
        port.flush().await.expect("flush ui05 facts");
    }

    let (page_digest, rebuild_digest) = {
        let host = host_with(store.clone());
        let page = host.ui_snapshot_page(&query(6, 3, None)).await.unwrap();
        let rebuild = host
            .rebuild_ui_snapshot_projection(&query(6, 3, None))
            .await
            .unwrap();
        host.flush_event_store()
            .await
            .expect("flush before restart");
        (page.snapshot_digest.clone(), rebuild.rebuild_digest.clone())
    };
    // 结束第一宿主的账本句柄，模拟进程退出后由新宿主实例重新打开同一文件。
    drop(store);

    let reopened = Arc::new(JsonlEventLog::open(&path).expect("reopen ui05 ledger"));
    let restarted = host_with(reopened.clone());
    let page = restarted
        .ui_snapshot_page(&query(6, 3, None))
        .await
        .expect("restarted host projects the same facts");
    assert_eq!(page.snapshot_digest, page_digest);
    let rebuild = restarted
        .rebuild_ui_snapshot_projection(&query(6, 3, None))
        .await
        .expect("restarted host rebuilds the same read model");
    assert_eq!(rebuild.rebuild_digest, rebuild_digest);
    assert!(path.exists());
}

/// 通过公开 Domain 游标构造函数生成「另一个位置」的拼接游标，避免在测试里手写 JSON。
fn stitched_cursor(epoch: &str, source_cursor: u64, offset: usize, revision: u64) -> String {
    kiana_domain::UiSnapshotPageCursor::new(epoch, source_cursor, offset, revision)
        .unwrap()
        .encode()
        .unwrap()
}
