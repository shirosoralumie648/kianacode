//! UI-06 daemon 行为夹具：真实 `DaemonHost` 上的 feed boundary、replay window、gap、背压与重启。
//!
//! 这些用例验证的唯一路径是组合根自带的单实例有界投影：
//!
//! ```text
//! 真实 run（KianaHarness + EventLog 事实）
//!     -> RunStreamBus                                  每个 run 一个有界 window + 有界广播队列
//!     -> DaemonHost::subscribe_run_feed_after()        boundary / delta / heartbeat / gap / terminal
//!     -> DaemonHost::feed_heartbeat()                  显式心跳，带服务端当前 cursor
//!     -> DaemonHost::feed_backpressure_metrics()       队列/窗口/滞后/终态拒绝计数
//!     -> DaemonHost::ui_snapshot_page()                gap 后按服务端 cursor 重新 hydrate（UI-05 只读页）
//! ```
//!
//! 先拒绝：foreign instance、old epoch、sequence ahead、replay window 过期、terminal 之后的 delta、
//! 无界队列与「慢消费者阻塞事实提交」都必须失败关闭或被显式降级成 gap，绝不能被渲染成连续 feed。
//!
//! 证明上限是 source/local 行为：`MemoryEventLog` 是进程内夹具，`JsonlEventLog` 是单文件夹具，
//! feed window 本身只活在进程内。`MemoryEventLog` -> 新宿主实例重开同一账本不等于真实跨进程
//! 恢复，也不产生 durable/live/physical 证据。

use async_trait::async_trait;
use kiana_daemon::{DaemonHost, ProjectTrustAuthority, RunStreamFeedError, UI_FEED_QUEUE_CAPACITY};
use kiana_domain::{RunId, UiSnapshotEntryKind, UiSnapshotQuery};
use kiana_protocol::{
    ExecutionStatus, RequestEnvelope, RequestMetadata, ResponseEnvelope, SessionId, UiCursor,
    UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UiFeedGapReason, UiFeedGapV1,
};
use kiana_runner::{KianaHarness, ModelClient, ModelDelta, ModelOutput, ModelRequest};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 与 `kiana-daemon/src/run_stream.rs` 的 `FEED_REPLAY_WINDOW` 一致；core 守卫把源码常量、本常量与
/// fixture 里声明的窗口绑在一起，任一处漂移都会失败。
const REPLAY_WINDOW: usize = 128;

/// 每个用例独立 `KIANA_HOME`；同 crate 的其它测试二进制在各自进程里，不需要跨文件串行。
static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn environment_lock() -> MutexGuard<'static, ()> {
    ENVIRONMENT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct TrustedProjects;

impl ProjectTrustAuthority for TrustedProjects {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

/// 流式模型：每个 chunk 产生一条 Delta，用来驱动真实 run 的 feed 投影。
///
/// `complete` 明确失败，保证用例只走 harness 的流式路径；`calls` 记录模型调用次数，用来证明
/// reconnect/replay 不会重新执行模型。
struct StreamingModel {
    chunks: usize,
    calls: AtomicUsize,
}

impl StreamingModel {
    fn new(chunks: usize) -> Arc<Self> {
        Arc::new(Self {
            chunks,
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl ModelClient for StreamingModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("ui06_model_complete_must_not_be_called".to_owned())
    }

    async fn complete_streaming(
        &self,
        _request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut text = String::new();
        for index in 0..self.chunks {
            let chunk = format!("chunk-{index:03}");
            text.push_str(&chunk);
            on_delta(ModelDelta::Text { text: chunk })?;
        }
        Ok(ModelOutput::text(text))
    }
}

/// 组装组合根；`MemoryEventLog` 是进程内夹具，feed window 只活在这次宿主实例里。
fn host_with_model(model: Arc<StreamingModel>) -> Arc<DaemonHost> {
    let model: Arc<dyn ModelClient> = model;
    let harness = KianaHarness::new(model);
    Arc::new(
        DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProjects))
            .expect("ui06 feed daemon"),
    )
}

fn disk_host(model: Arc<StreamingModel>, events_path: impl AsRef<Path>) -> Arc<DaemonHost> {
    let model: Arc<dyn ModelClient> = model;
    let harness = KianaHarness::new(model);
    Arc::new(
        DaemonHost::with_harness_on_disk_and_project_authority(
            harness,
            events_path,
            Arc::new(TrustedProjects),
        )
        .expect("ui06 disk daemon"),
    )
}

fn temp_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("kiana-ui06-{label}-{stamp}"));
    fs::create_dir_all(&path).expect("ui06 temp dir");
    path
}

/// run 身份来自 session：`RequestMetadata::local` + `SessionId == run_id` 是仓内既有约定。
fn run_envelope(project_root: &Path, run_id: RunId) -> RequestEnvelope {
    let mut metadata = RequestMetadata::local(run_id.to_string(), project_root.to_string_lossy());
    metadata.session_id = SessionId::new(run_id.to_string());
    metadata.project_trusted = true;
    RequestEnvelope::run(metadata, "stream it", None)
}

fn response_run_id(response: &ResponseEnvelope) -> RunId {
    response
        .output
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .and_then(RunId::parse_str)
        .expect("a run response carries its run id")
}

/// 收下所有 live 帧直到终态；返回终态帧与中间的 feed sequence。
async fn drain_until_terminal(
    subscription: &mut kiana_daemon::RunStreamFeedSubscription,
) -> (UiFeedFrameV1, Vec<u64>) {
    let mut sequences = Vec::new();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), subscription.recv_frame())
            .await
            .expect("live feed frame timeout")
            .expect("live feed frame");
        assert_ne!(
            frame.kind,
            UiFeedFrameKind::Gap,
            "a live subscription must not be served a gap"
        );
        assert!(!frame.replay, "live frames are never replays");
        sequences.push(frame.cursor.feed_sequence);
        if frame.terminal {
            return (frame, sequences);
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedFaultPlan {
    schema: String,
    step: String,
    queue_capacity: usize,
    replay_window: usize,
    gap_reasons: Vec<String>,
    required: Vec<String>,
    denied: Vec<String>,
}

fn fault_plan() -> FeedFaultPlan {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui06-feed-replay.json");
    let raw = fs::read_to_string(&path).expect("ui06 fault fixture is readable");
    serde_json::from_str(&raw).expect("ui06 fault fixture matches its declared schema")
}

fn sample_cursor(instance: &str, epoch: &str, feed_sequence: u64) -> UiFeedCursorV1 {
    UiFeedCursorV1::new(
        instance,
        epoch,
        feed_sequence,
        UiCursor {
            epoch: epoch.to_owned(),
            sequence: 1,
        },
    )
    .expect("ui06 fixture cursor")
}

fn assert_gap(frame: &UiFeedFrameV1, reason: UiFeedGapReason) -> &UiFeedGapV1 {
    assert_eq!(frame.kind, UiFeedFrameKind::Gap);
    let gap = frame.gap.as_ref().expect("gap frame carries its range");
    assert_eq!(gap.reason, reason);
    assert!(gap.snapshot_required, "every gap requires a snapshot");
    gap
}

#[test]
fn ui06_feed_fault_plan_binds_the_bounded_budget_and_the_deny_matrix() {
    let plan = fault_plan();
    assert_eq!(plan.schema, "kiana.ui06-feed-replay-fixture.v1");
    assert_eq!(plan.step, "UI-06");
    assert_eq!(plan.queue_capacity, UI_FEED_QUEUE_CAPACITY);
    assert_eq!(plan.replay_window, REPLAY_WINDOW);

    let mut declared = plan.gap_reasons.clone();
    declared.sort();
    declared.dedup();
    assert_eq!(
        declared.len(),
        plan.gap_reasons.len(),
        "the fault plan must not repeat a gap reason"
    );
    for reason in [
        UiFeedGapReason::SequenceGap,
        UiFeedGapReason::ReplayExpired,
        UiFeedGapReason::OldEpoch,
        UiFeedGapReason::InstanceChanged,
        UiFeedGapReason::SequenceAhead,
        UiFeedGapReason::Backpressure,
        UiFeedGapReason::TerminalRetention,
    ] {
        let encoded = serde_json::to_value(&reason)
            .expect("gap reason encodes")
            .as_str()
            .expect("gap reason is a string")
            .to_owned();
        assert!(
            declared.binary_search(&encoded).is_ok(),
            "the fault plan must keep the {encoded} denial"
        );
    }
    for needle in [
        "snapshot boundary",
        "bounded replay window",
        "terminal retention",
        "backpressure metrics",
    ] {
        assert!(
            plan.required.iter().any(|line| line.contains(needle)),
            "the fault plan must require: {needle}"
        );
    }
    for needle in [
        "unbounded queue",
        "delta after terminal",
        "second execution loop",
    ] {
        assert!(
            plan.denied.iter().any(|line| line.contains(needle)),
            "the fault plan must deny: {needle}"
        );
    }
}

#[tokio::test]
async fn live_feed_keeps_the_boundary_the_sequence_and_the_retained_terminal() {
    let _environment_lock = environment_lock();
    let home = temp_dir("live-home");
    let root = temp_dir("live-root");
    let _home = EnvGuard::set("KIANA_HOME", &home);
    let _distill = EnvGuard::remove("KIANA_MEMORY_DISTILL_AUTO");
    let model = StreamingModel::new(6);
    let host = host_with_model(model.clone());
    let run_id = RunId::new();

    // 订阅必须先于 run 建立，否则 feed 不会把 delta 投影给任何消费者。
    let mut live = host.subscribe_run_feed_after(run_id, None);
    let boundary = live.recv_frame().await.expect("boundary frame");
    assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
    assert_eq!(boundary.cursor.feed_sequence, 0);
    assert_eq!(boundary.cursor.instance_id, host.feed_instance_id());
    assert_eq!(boundary.gap, None);

    let response = host.handle(run_envelope(&root, run_id)).await;
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response_run_id(&response), run_id);

    let (terminal, sequences) = drain_until_terminal(&mut live).await;
    assert_eq!(terminal.kind, UiFeedFrameKind::Terminal);
    assert!(!terminal.replay);
    assert!(
        sequences.len() > 1,
        "the run must publish deltas before the terminal"
    );
    for window in sequences.windows(2) {
        assert_eq!(
            window[1],
            window[0] + 1,
            "the feed sequence is server-owned and dense"
        );
    }
    let model_calls = model.calls.load(Ordering::SeqCst);
    assert!(model_calls >= 1, "the run must actually call the model");

    // terminal 之后不得再出现 delta；伪造的 late delta 在源层被拒绝。
    assert!(
        tokio::time::timeout(Duration::from_millis(300), live.recv_frame())
            .await
            .is_err(),
        "a terminal must be the last live frame"
    );
    let facts = host.persisted_events().await.expect("ledger read").unwrap();
    assert!(facts.iter().any(|event| event.kind == "run.completed"));

    // 心跳是显式帧，并且带服务端当前 feed cursor。
    let heartbeat = host.feed_heartbeat(run_id).expect("bounded heartbeat");
    assert_eq!(heartbeat.kind, UiFeedFrameKind::Heartbeat);
    heartbeat.validate().expect("heartbeat DTO is valid");
    assert_eq!(
        heartbeat.cursor.feed_sequence,
        terminal.cursor.feed_sequence
    );

    // 用终态 cursor 重连只是重复位置：只给 boundary，不重复投递 terminal。
    let mut caught_up = host.subscribe_run_feed_after(run_id, Some(&terminal.cursor));
    assert!(!caught_up.has_pending_gap());
    assert_eq!(
        caught_up.recv_frame().await.expect("resume boundary").kind,
        UiFeedFrameKind::SnapshotBoundary
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), caught_up.recv_frame())
            .await
            .is_err(),
        "an already-seen position must not be replayed twice"
    );

    // 没有 cursor 的重连仍然看得到被保留的 terminal（terminal retention）。
    let mut retained = host.subscribe_run_feed_after(run_id, None);
    assert_eq!(
        retained
            .recv_frame()
            .await
            .expect("retention boundary")
            .kind,
        UiFeedFrameKind::SnapshotBoundary
    );
    let replayed = retained.recv_frame().await.expect("retained terminal");
    assert_eq!(replayed.kind, UiFeedFrameKind::Terminal);
    assert!(replayed.terminal);
    assert!(replayed.replay);
    assert_eq!(replayed.cursor, terminal.cursor);

    let metrics = host.feed_backpressure_metrics();
    assert_eq!(metrics.queue_capacity, UI_FEED_QUEUE_CAPACITY);
    assert_eq!(metrics.replay_window, REPLAY_WINDOW);
    assert_eq!(metrics.lagged_receives, 0);

    // replay 只是投影：不追加事实，也不重新执行模型。
    assert_eq!(
        host.persisted_events()
            .await
            .expect("ledger read")
            .unwrap()
            .len(),
        facts.len()
    );
    assert_eq!(model.calls.load(Ordering::SeqCst), model_calls);
}

#[tokio::test]
async fn gap_frames_use_the_server_cursor_and_hydrate_through_the_snapshot_page() {
    let _environment_lock = environment_lock();
    let home = temp_dir("gap-home");
    let root = temp_dir("gap-root");
    let _home = EnvGuard::set("KIANA_HOME", &home);
    let _distill = EnvGuard::remove("KIANA_MEMORY_DISTILL_AUTO");
    let model = StreamingModel::new(4);
    let host = host_with_model(model.clone());
    let run_id = RunId::new();

    let mut live = host.subscribe_run_feed_after(run_id, None);
    assert_eq!(
        live.recv_frame().await.expect("boundary frame").kind,
        UiFeedFrameKind::SnapshotBoundary
    );
    let response = host.handle(run_envelope(&root, run_id)).await;
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    let (terminal, _) = drain_until_terminal(&mut live).await;
    drop(live);

    let instance = host.feed_instance_id();
    let epoch = host.ui_cursor().epoch;
    let facts = host.persisted_events().await.expect("ledger read").unwrap();
    let facts_before = facts.len();

    // (a) old epoch：旧 epoch 的 cursor 只能拿到 gap，并且 gap 目标必须是服务端当前 cursor。
    let old_epoch = sample_cursor(&instance, "epoch-retired", 1);
    let mut stale = host.subscribe_run_feed_after(run_id, Some(&old_epoch));
    let frame = stale.recv_frame().await.expect("old epoch gap");
    let gap = assert_gap(&frame, UiFeedGapReason::OldEpoch);
    assert_eq!(gap.from.as_ref(), Some(&old_epoch));
    assert_eq!(gap.to.feed_sequence, terminal.cursor.feed_sequence);
    assert_ne!(gap.to.feed_sequence, old_epoch.feed_sequence);
    let boundary = stale.recv_frame().await.expect("boundary after gap");
    assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
    assert_eq!(boundary.cursor, gap.to);
    boundary.validate().expect("gap boundary is a valid frame");

    // (b) 用服务端 cursor 复订：不再产生第二个 gap，位置由服务端决定。
    let mut recovered = host.subscribe_run_feed_after(run_id, Some(&gap.to));
    assert!(!recovered.has_pending_gap());
    assert_eq!(
        recovered
            .recv_frame()
            .await
            .expect("recovered boundary")
            .kind,
        UiFeedFrameKind::SnapshotBoundary
    );

    // (c) foreign instance 与 (d) sequence ahead 都拒绝。
    let foreign = sample_cursor("foreign-instance", &epoch, terminal.cursor.feed_sequence);
    let mut foreign_stream = host.subscribe_run_feed_after(run_id, Some(&foreign));
    let frame = foreign_stream
        .recv_frame()
        .await
        .expect("foreign instance gap");
    assert_gap(&frame, UiFeedGapReason::InstanceChanged);

    let ahead = sample_cursor(&instance, &epoch, terminal.cursor.feed_sequence + 5);
    let mut ahead_stream = host.subscribe_run_feed_after(run_id, Some(&ahead));
    let frame = ahead_stream.recv_frame().await.expect("sequence ahead gap");
    let gap = assert_gap(&frame, UiFeedGapReason::SequenceAhead);
    assert_eq!(gap.to.feed_sequence, terminal.cursor.feed_sequence);

    // gap 之后按服务端 cursor 重新 hydrate：UI-05 只读页覆盖同一账本的全部已提交事实。
    let hydration = UiSnapshotQuery::new(
        "local-user",
        Some(run_id.to_string()),
        epoch.clone(),
        facts_before as u64,
        Some(facts_before as u64),
        1,
        1,
        0,
        16,
        None,
        1_700_000_000_000,
    )
    .expect("ui06 hydration query");
    let page = host
        .ui_snapshot_page(&hydration)
        .await
        .expect("ui06 hydration page");
    assert_eq!(page.owner_id, "local-user");
    assert_eq!(page.epoch, epoch);
    assert_eq!(page.source_cursor, facts_before as u64);
    assert_eq!(page.snapshot_cursor, page.source_cursor);
    assert_eq!(page.projection_cursor, page.source_cursor);
    assert!(page
        .entries
        .iter()
        .any(|entry| entry.kind == UiSnapshotEntryKind::Run && entry.id == run_id.to_string()));
    let repeat = host
        .ui_snapshot_page(&hydration)
        .await
        .expect("ui06 repeated hydration page");
    assert_eq!(repeat.snapshot_digest, page.snapshot_digest);
    assert_eq!(
        host.persisted_events()
            .await
            .expect("ledger read")
            .unwrap()
            .len(),
        facts_before,
        "hydration must not append facts"
    );
}

#[tokio::test]
async fn a_stalled_consumer_gaps_but_never_blocks_committed_facts() {
    let _environment_lock = environment_lock();
    let home = temp_dir("stalled-home");
    let root = temp_dir("stalled-root");
    let _home = EnvGuard::set("KIANA_HOME", &home);
    let _distill = EnvGuard::remove("KIANA_MEMORY_DISTILL_AUTO");
    // 超过广播队列容量，慢消费者必然滞后。
    let model = StreamingModel::new(UI_FEED_QUEUE_CAPACITY + 32);
    let host = host_with_model(model.clone());
    let run_id = RunId::new();

    let mut stalled = host.subscribe_run_feed_after(run_id, None);
    assert_eq!(
        stalled.recv_frame().await.expect("boundary frame").kind,
        UiFeedFrameKind::SnapshotBoundary
    );

    let response = host.handle(run_envelope(&root, run_id)).await;
    assert_eq!(
        response.status,
        ExecutionStatus::Completed,
        "a stalled display consumer must never fail the run"
    );
    let facts = host.persisted_events().await.expect("ledger read").unwrap();
    assert!(
        facts.iter().any(|event| event.kind == "run.completed"),
        "facts must keep committing while a consumer is stalled"
    );

    // 慢消费者拿到的是显式 backpressure gap，而不是静默丢帧；指标在观察点被记录。
    let lagged = stalled.recv_frame().await;
    assert!(
        matches!(
            &lagged,
            Err(RunStreamFeedError::Gap(gap))
                if gap.reason == UiFeedGapReason::Backpressure && gap.snapshot_required
        ),
        "a lagged subscription must fail closed with a backpressure gap: {lagged:?}"
    );

    let metrics = host.feed_backpressure_metrics();
    assert_eq!(metrics.queue_capacity, UI_FEED_QUEUE_CAPACITY);
    assert_eq!(metrics.replay_window, REPLAY_WINDOW);
    assert!(
        metrics.lagged_receives > 0,
        "exceeding the bounded queue must be counted, not silent"
    );
    assert!(
        metrics.gap_frames > 0,
        "the stalled consumer must receive an explicit gap"
    );

    // 慢消费者拿到 gap 后仍可用服务端 cursor 重新建立有界位置。
    let mut recovered = host.subscribe_run_feed_after(run_id, None);
    let boundary = recovered.recv_frame().await.expect("post-gap boundary");
    assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
    assert!(
        boundary.cursor.feed_sequence >= 1,
        "the server cursor advances with the committed facts"
    );
}

#[tokio::test]
async fn restarted_host_requires_hydration_and_keeps_the_same_ledger() {
    let _environment_lock = environment_lock();
    let home = temp_dir("restart-home");
    let root = temp_dir("restart-root");
    let _home = EnvGuard::set("KIANA_HOME", &home);
    let _distill = EnvGuard::remove("KIANA_MEMORY_DISTILL_AUTO");
    let events_path = temp_dir("restart-ledger").join("events.jsonl");
    let run_id = RunId::new();

    let (first_instance, terminal_cursor, facts_after_run) = {
        let model = StreamingModel::new(3);
        let host = disk_host(model, &events_path);
        let mut live = host.subscribe_run_feed_after(run_id, None);
        assert_eq!(
            live.recv_frame().await.expect("boundary frame").kind,
            UiFeedFrameKind::SnapshotBoundary
        );
        let response = host.handle(run_envelope(&root, run_id)).await;
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
        let (terminal, _) = drain_until_terminal(&mut live).await;
        drop(live);
        let facts = host.persisted_events().await.expect("ledger read").unwrap();
        let count = facts.len();
        host.close_event_store()
            .await
            .expect("flush the ui06 ledger");
        (host.feed_instance_id(), terminal.cursor, count)
    };

    // 第一个宿主的账本句柄随 `host` 一起释放，新宿主实例重开同一文件。
    let model = StreamingModel::new(1);
    let restarted = disk_host(model, &events_path);
    assert_ne!(
        restarted.feed_instance_id(),
        first_instance,
        "a restarted host is a new feed instance"
    );

    let mut resumed = restarted.subscribe_run_feed_after(run_id, Some(&terminal_cursor));
    let frame = resumed.recv_frame().await.expect("instance changed gap");
    let gap = assert_gap(&frame, UiFeedGapReason::InstanceChanged);
    assert_eq!(
        gap.from.as_ref().map(|from| from.instance_id.as_str()),
        Some(first_instance.as_str())
    );
    assert_eq!(gap.to.instance_id, restarted.feed_instance_id());
    let boundary = resumed.recv_frame().await.expect("boundary after restart");
    assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
    assert_eq!(boundary.cursor, gap.to);
    assert_eq!(
        boundary.cursor.feed_sequence, 0,
        "a fresh instance has no replay window for the old cursor"
    );

    // 同一账本：重启后 hydrate 仍然看得到这条 run 的全部事实。
    let facts = restarted
        .persisted_events()
        .await
        .expect("ledger read")
        .unwrap();
    assert_eq!(facts.len(), facts_after_run);
    assert!(facts.iter().any(|event| event.kind == "run.completed"));
    assert!(events_path.exists());
}
