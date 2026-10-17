//! UI-04 Core 行为 fixture：动作 CAS、幂等、ACK 丢失、并发和进程重启后的对账。
//!
//! 之前的用例多停在"shape guard"和单条 Domain reducer 上，这个文件把 ControlPlane facade 接到真实
//! 的 EventStore 语义上，逐个复现卡片要求的情景：
//!
//! ```text
//! 重复请求        -> 返回原记录，不写第二条 accepted 事实，不产生第二个 effect
//! 新旧 payload    -> 同 key 换内容在写事件前就被拒绝
//! ACK 丢失        -> 提交已落盘但调用方只拿到 result_unknown；只有原 key 查询能收敛
//! 并发 CAS        -> 两个不同 command ID 抢同一 key，恰好一条 accepted 事实
//! 复用旧 command  -> 换 key 复用已提交的 command ID 被结构性拒绝
//! 进程重启        -> 重开 JSONL 日志后同一条记录仍在，重放不会产生第二个 effect
//! 越权查询        -> owner/scope 不匹配时查询拒绝且不返回记录
//! ```
//!
//! 这些 fixture 使用 MemoryEventLog 与临时目录中的 JsonlEventLog，不触达任何真实 UI effect、外部目标
//! 或模型；因此证明上限是 local_behavior，不是 durable 或 live。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{ControlPlane, UiActionAuthoritySnapshot};
use kiana_domain::{
    json_digest, ApprovalChallenge, ApprovalId, CapabilityRequest, CommandReceipt, CommitOutcome,
    PendingApproval, RequestContext, RequestId, RuntimeEvent, TransitionBatch, UiActionCommand,
    UiActionResubmitPolicy, UiActionState,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const ACCEPTED_KIND: &str = "ui.action.accepted";
const APPLIED_KIND: &str = "ui.action.applied";

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

/// 判定路径不碰审批：三个方法都返回 Unavailable，也就是**万一被碰到就拒绝**。
struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn core_over(events: Arc<dyn EventStorePort>) -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

/// 模拟"提交已经落盘、但调用方没有收到 ACK"的 EventStore：只丢弃一次已提交结果，不改写存储事实。
///
/// 这不是 Corruption：底层内存日志里的命令回执和事件都真实存在，调用方拿到的只是
/// `CommitOutcome::Unknown`。测试要证明 Core 不会把这个未知当成失败吞掉，也不会自动重做。
struct LostAckStore {
    inner: Arc<MemoryEventLog>,
    drop_next_ack: AtomicBool,
}

impl LostAckStore {
    fn new() -> Self {
        Self {
            inner: Arc::new(MemoryEventLog::new()),
            drop_next_ack: AtomicBool::new(false),
        }
    }

    fn drop_next_ack(&self) {
        self.drop_next_ack.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl EventStorePort for LostAckStore {
    fn supports_atomic_transitions(&self) -> bool {
        true
    }

    async fn commit_transition(&self, batch: TransitionBatch) -> Result<CommitOutcome, PortError> {
        let command_id = batch.command_id;
        let outcome = self.inner.commit_transition(batch).await?;
        if outcome.receipt().is_some() && self.drop_next_ack.swap(false, Ordering::SeqCst) {
            return Ok(CommitOutcome::Unknown {
                command_id,
                reason: "simulated_ack_lost".to_owned(),
            });
        }
        Ok(outcome)
    }

    async fn read_command(
        &self,
        command_id: &RequestId,
    ) -> Result<Option<CommandReceipt>, PortError> {
        self.inner.read_command(command_id).await
    }

    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_stream(aggregate_type, aggregate_id).await
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }
}

fn scope_digest() -> String {
    json_digest(&json!({"scope":"run-1"}))
}

fn authority() -> UiActionAuthoritySnapshot {
    UiActionAuthoritySnapshot {
        authority_epoch: "epoch-1".to_owned(),
        cursor: 0,
        target_revision: Some(4),
        owner_id: "owner-1".to_owned(),
        scope_digest: scope_digest(),
        permit_digest: Some(json_digest(&json!({"permit":"ui-04"}))),
        cancelled: false,
    }
}

fn command(command_id: RequestId, key: &str, payload: Value) -> UiActionCommand {
    UiActionCommand::new(
        command_id,
        key,
        "run-target",
        "owner-1",
        scope_digest(),
        "epoch-1",
        0,
        Some(4),
        10_000,
        payload,
    )
    .expect("ui action command")
}

fn receipt(label: &str) -> String {
    json_digest(&json!({"receipt": label}))
}

async fn stream_events(events: &dyn EventStorePort, key: &str) -> Vec<RuntimeEvent> {
    events
        .read_stream("ui_action", key)
        .await
        .expect("ui action stream")
}

fn count_kind(events: &[RuntimeEvent], kind: &str) -> usize {
    events.iter().filter(|event| event.kind == kind).count()
}

fn unique_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-ui04-{label}-{stamp}.jsonl"))
}

#[tokio::test]
async fn duplicate_submission_returns_the_original_record_without_a_second_effect() {
    let events = Arc::new(MemoryEventLog::new());
    let core = core_over(events.clone());
    let action = command(
        RequestId::new(),
        "ui-04-duplicate",
        json!({"decision":"approve"}),
    );

    let accepted = core
        .admit_ui_action(&action, &authority(), 1_000)
        .await
        .expect("first admission");
    assert_eq!(accepted.state, UiActionState::Accepted);
    assert_eq!(accepted.effect_count, 0);

    let replayed = core
        .admit_ui_action(&action, &authority(), 2_000)
        .await
        .expect("duplicate admission returns original");
    assert_eq!(replayed, accepted);

    let applied = core
        .apply_ui_action(&action, &receipt("first"), 2_001)
        .await
        .expect("apply once");
    assert_eq!(applied.state, UiActionState::Applied);
    assert_eq!(applied.effect_count, 1);

    let applied_again = core
        .apply_ui_action(&action, &receipt("second"), 2_002)
        .await
        .expect("second apply is a replay");
    assert_eq!(applied_again.receipt_digest, applied.receipt_digest);
    assert_eq!(applied_again.effect_count, 1);

    let stream = stream_events(events.as_ref(), &action.idempotency_key).await;
    assert_eq!(count_kind(&stream, ACCEPTED_KIND), 1);
    assert_eq!(count_kind(&stream, APPLIED_KIND), 1);

    let queried = core
        .query_original_ui_action_for(&action.idempotency_key, "owner-1", &scope_digest())
        .await
        .expect("query")
        .expect("record");
    assert_eq!(queried, applied_again);
    assert_eq!(
        queried.resubmit_policy(),
        UiActionResubmitPolicy::DoNotRetry
    );
}

#[tokio::test]
async fn changed_payload_under_the_same_key_is_refused_before_any_event_is_written() {
    let events = Arc::new(MemoryEventLog::new());
    let core = core_over(events.clone());
    let action = command(
        RequestId::new(),
        "ui-04-changed",
        json!({"decision":"approve"}),
    );
    core.admit_ui_action(&action, &authority(), 1_000)
        .await
        .expect("admission");

    let changed = command(
        RequestId::new(),
        "ui-04-changed",
        json!({"decision":"deny"}),
    );
    let error = core
        .admit_ui_action(&changed, &authority(), 1_000)
        .await
        .expect_err("changed payload must be refused");
    assert!(
        error
            .to_string()
            .contains("ui_action_idempotency_digest_mismatch"),
        "unexpected error: {error}"
    );

    let stream = stream_events(events.as_ref(), &action.idempotency_key).await;
    assert_eq!(stream.len(), 1);
    assert_eq!(count_kind(&stream, ACCEPTED_KIND), 1);
    let queried = core
        .query_original_ui_action_for(&action.idempotency_key, "owner-1", &scope_digest())
        .await
        .expect("query")
        .expect("record");
    assert_eq!(queried.command_digest, action.command_digest);
    assert_eq!(queried.effect_count, 0);
}

#[tokio::test]
async fn reusing_a_committed_command_id_under_a_new_key_is_refused() {
    let events = Arc::new(MemoryEventLog::new());
    let core = core_over(events.clone());
    let command_id = RequestId::new();
    let first = command(command_id, "ui-04-reuse-a", json!({"decision":"approve"}));
    core.admit_ui_action(&first, &authority(), 1_000)
        .await
        .expect("first admission");

    let second = command(command_id, "ui-04-reuse-b", json!({"decision":"approve"}));
    let error = core
        .admit_ui_action(&second, &authority(), 1_000)
        .await
        .expect_err("reused command id must be refused");
    assert!(
        error.to_string().contains("ui_action_command_id_reused"),
        "unexpected error: {error}"
    );
    assert!(stream_events(events.as_ref(), "ui-04-reuse-b")
        .await
        .is_empty());
    assert_eq!(
        stream_events(events.as_ref(), "ui-04-reuse-a").await.len(),
        1
    );
}

#[tokio::test]
async fn lost_ack_commit_is_unknown_and_only_the_original_key_converges() {
    let store = Arc::new(LostAckStore::new());
    let core = core_over(store.clone());
    let action = command(
        RequestId::new(),
        "ui-04-lost-ack",
        json!({"decision":"approve"}),
    );
    store.drop_next_ack();

    let error = core
        .admit_ui_action(&action, &authority(), 1_000)
        .await
        .expect_err("a lost ACK is not success");
    assert!(
        error
            .to_string()
            .contains("result_unknown:ui_action_accept_unconfirmed"),
        "unexpected error: {error}"
    );

    // ACK 丢失不等于没有发生：accepted 事实已经落盘，调用方必须按原键查询收敛。
    let stream = stream_events(store.inner.as_ref(), &action.idempotency_key).await;
    assert_eq!(count_kind(&stream, ACCEPTED_KIND), 1);
    let queried = core
        .query_original_ui_action_for(&action.idempotency_key, "owner-1", &scope_digest())
        .await
        .expect("query")
        .expect("original record");
    assert_eq!(queried.state, UiActionState::Accepted);
    assert_eq!(
        queried.resubmit_policy(),
        UiActionResubmitPolicy::QueryOriginal
    );

    // 同一个命令重发会收敛到原记录，且不追加第二条 accepted 事实。
    let replayed = core
        .admit_ui_action(&action, &authority(), 1_100)
        .await
        .expect("retry converges to the original record");
    assert_eq!(replayed, queried);
    let stream = stream_events(store.inner.as_ref(), &action.idempotency_key).await;
    assert_eq!(count_kind(&stream, ACCEPTED_KIND), 1);
}

#[tokio::test]
async fn concurrent_duplicate_admissions_commit_exactly_one_accepted_fact() {
    let events = Arc::new(MemoryEventLog::new());
    let core = core_over(events.clone());
    let first = command(
        RequestId::new(),
        "ui-04-concurrent",
        json!({"decision":"approve"}),
    );
    let second = command(
        RequestId::new(),
        "ui-04-concurrent",
        json!({"decision":"approve"}),
    );

    let (left, right) = tokio::join!(
        core.admit_ui_action(&first, &authority(), 1_000),
        core.admit_ui_action(&second, &authority(), 1_000),
    );

    let mut succeeded = 0usize;
    for result in [&left, &right] {
        match result {
            Ok(record) => {
                succeeded += 1;
                assert_eq!(record.state, UiActionState::Accepted);
            }
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("ui_action_cas_conflict")
                        || text.contains("ui_action_idempotency_digest_mismatch"),
                    "a losing concurrent admission must be refused structurally, got: {text}"
                );
            }
        }
    }
    assert_eq!(succeeded, 1, "exactly one concurrent admission may succeed");

    let stream = stream_events(events.as_ref(), "ui-04-concurrent").await;
    assert_eq!(count_kind(&stream, ACCEPTED_KIND), 1);
    let queried = core
        .query_original_ui_action_for("ui-04-concurrent", "owner-1", &scope_digest())
        .await
        .expect("query")
        .expect("record");
    assert_eq!(queried.effect_count, 0);
    assert!(queried.command_id == first.command_id || queried.command_id == second.command_id);
}

#[tokio::test]
async fn missing_permit_and_mismatched_authority_are_refused_without_events() {
    let events = Arc::new(MemoryEventLog::new());
    let core = core_over(events.clone());
    let action = command(
        RequestId::new(),
        "ui-04-denied",
        json!({"decision":"approve"}),
    );

    let mut no_permit = authority();
    no_permit.permit_digest = None;
    let error = core
        .admit_ui_action(&action, &no_permit, 1_000)
        .await
        .expect_err("permit required");
    assert!(
        error.to_string().contains("ui_action_permit_required"),
        "unexpected error: {error}"
    );

    let mut cancelling = authority();
    cancelling.cancelled = true;
    let error = core
        .admit_ui_action(&action, &cancelling, 1_000)
        .await
        .expect_err("cancelled target refused");
    assert!(
        error.to_string().contains("ui_action_cancelled"),
        "unexpected error: {error}"
    );

    let mut foreign_owner = authority();
    foreign_owner.owner_id = "other-owner".to_owned();
    let error = core
        .admit_ui_action(&action, &foreign_owner, 1_000)
        .await
        .expect_err("foreign owner refused");
    assert!(
        error.to_string().contains("ui_action_owner_mismatch"),
        "unexpected error: {error}"
    );

    let mut foreign_scope = authority();
    foreign_scope.scope_digest = json_digest(&json!({"scope":"run-2"}));
    let error = core
        .admit_ui_action(&action, &foreign_scope, 1_000)
        .await
        .expect_err("foreign scope refused");
    assert!(
        error.to_string().contains("ui_action_scope_mismatch"),
        "unexpected error: {error}"
    );

    assert!(stream_events(events.as_ref(), "ui-04-denied")
        .await
        .is_empty());
}

#[tokio::test]
async fn original_query_hides_records_from_other_owners_and_scopes() {
    let events = Arc::new(MemoryEventLog::new());
    let core = core_over(events.clone());
    let action = command(
        RequestId::new(),
        "ui-04-query",
        json!({"decision":"approve"}),
    );
    core.admit_ui_action(&action, &authority(), 1_000)
        .await
        .expect("admission");

    assert!(core
        .query_original_ui_action_for("ui-04-query", "owner-1", &scope_digest())
        .await
        .expect("owner query")
        .is_some());
    let error = core
        .query_original_ui_action_for("ui-04-query", "other-owner", &scope_digest())
        .await
        .expect_err("foreign owner query refused");
    assert!(
        error.to_string().contains("ui_action_query_scope_mismatch"),
        "unexpected error: {error}"
    );
    let error = core
        .query_original_ui_action_for(
            "ui-04-query",
            "owner-1",
            &json_digest(&json!({"scope":"run-2"})),
        )
        .await
        .expect_err("foreign scope query refused");
    assert!(
        error.to_string().contains("ui_action_query_scope_mismatch"),
        "unexpected error: {error}"
    );
    assert!(core
        .query_original_ui_action_for("ui-04-absent", "owner-1", &scope_digest())
        .await
        .expect("absent key query")
        .is_none());
    let error = core
        .query_original_ui_action_for("ui-04-query", "", &scope_digest())
        .await
        .expect_err("empty owner refused");
    assert!(
        error.to_string().contains("ui_action_query_owner_invalid"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn restart_reloads_the_applied_record_and_refuses_a_second_effect() {
    let path = unique_path("restart");
    let action = command(
        RequestId::new(),
        "ui-04-restart",
        json!({"decision":"approve"}),
    );
    let applied = {
        let events: Arc<dyn EventStorePort> =
            Arc::new(JsonlEventLog::open(&path).expect("open durable journal"));
        let core = core_over(events);
        core.admit_ui_action(&action, &authority(), 1_000)
            .await
            .expect("admission");
        core.apply_ui_action(&action, &receipt("restart"), 1_001)
            .await
            .expect("apply")
    };
    assert_eq!(applied.state, UiActionState::Applied);

    // 重新打开同一份日志模拟进程重启：事实不变，重建后的 ControlPlane 必须给出同一条记录。
    let events: Arc<dyn EventStorePort> =
        Arc::new(JsonlEventLog::open(&path).expect("reopen durable journal"));
    let core = core_over(events.clone());
    let queried = core
        .query_original_ui_action_for("ui-04-restart", "owner-1", &scope_digest())
        .await
        .expect("query after restart")
        .expect("record survives restart");
    assert_eq!(queried.state, UiActionState::Applied);
    assert_eq!(queried.record_digest, applied.record_digest);

    let replayed = core
        .admit_ui_action(&action, &authority(), 2_000)
        .await
        .expect("replay after restart");
    assert_eq!(replayed, applied);
    let applied_again = core
        .apply_ui_action(&action, &receipt("second"), 2_001)
        .await
        .expect("second apply is a replay");
    assert_eq!(applied_again.receipt_digest, applied.receipt_digest);
    assert_eq!(applied_again.effect_count, 1);

    let stream = stream_events(events.as_ref(), "ui-04-restart").await;
    assert_eq!(count_kind(&stream, ACCEPTED_KIND), 1);
    assert_eq!(count_kind(&stream, APPLIED_KIND), 1);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}
