//! UI-08 daemon 行为夹具：真实 `DaemonHost` 的 run-stream feed 只能经显式 entity 命名空间进入共享 store。
//!
//! 这些用例把共享 reducer 接到组合根的真实输出上，验证的唯一路径是：
//!
//! ```text
//! 真实 run（KianaHarness + EventLog 事实）
//!     -> DaemonHost::subscribe_run_feed_after()    boundary / delta / terminal / gap
//!     -> kiana_client::UiEntityStore               只读、按 scope/revision/事件 ID 合并的展示投影
//!     -> DaemonHost::persisted_events()             事实仍由 EventStore/ControlPlane 拥有
//! ```
//!
//! 先拒绝：没有 entity 命名空间的 run-stream 帧不得被猜成某个实体；跨 tab / 跨 session 的帧必须
//! 拒绝；真实 gap 与 authority epoch 变化必须要求 snapshot，而不是让 store 悄悄继续吃 delta。
//!
//! 这里不启动 UI、不执行 effect、不调用第二个模型循环：`UiEntityStore` 是进程内一次性投影，
//! 证明上限是 source/local 行为，不产生 durable/live/physical 证据。

use async_trait::async_trait;
use kiana_client::{
    UiEntityKey, UiEntityKind, UiEntityLifecycle, UiEntityStore, UiOptimisticUpdate, UiStoreChange,
    UiStoreError, UiStoreEvent, UiStoreScope,
};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::RunId;
use kiana_protocol::{
    ExecutionStatus, RequestEnvelope, RequestId, RequestMetadata, ResponseEnvelope, SessionId,
    UiCursor, UiFeedCursorV1, UiFeedFrameKind, UiFeedFrameV1, UI_FEED_FRAME_SCHEMA,
};
use kiana_runner::{KianaHarness, ModelClient, ModelDelta, ModelOutput, ModelRequest};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
/// 把帧投影进 store 不会重新执行模型。
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
        Err("ui08_model_complete_must_not_be_called".to_owned())
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

fn host_with_model(model: Arc<StreamingModel>) -> Arc<DaemonHost> {
    let model: Arc<dyn ModelClient> = model;
    let harness = KianaHarness::new(model);
    Arc::new(
        DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProjects))
            .expect("ui08 daemon"),
    )
}

fn temp_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("kiana-ui08-{label}-{stamp}"));
    std::fs::create_dir_all(&path).expect("ui08 temp dir");
    path
}

/// run 身份来自 session：`RequestMetadata::local` + `SessionId == run_id` 是仓内既有约定。
fn run_envelope(project_root: &Path, run_id: RunId) -> RequestEnvelope {
    let mut metadata = RequestMetadata::local(run_id.to_string(), project_root.to_string_lossy());
    metadata.session_id = SessionId::new(run_id.to_string());
    metadata.project_trusted = true;
    RequestEnvelope::run(metadata, "project the feed", None)
}

fn response_run_id(response: &ResponseEnvelope) -> RunId {
    response
        .output
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(RunId::parse_str)
        .expect("a run response carries its run id")
}

/// 收下所有 live 帧直到终态（含终态）；live 订阅不得被喂 gap。
async fn drain_frames(
    subscription: &mut kiana_daemon::RunStreamFeedSubscription,
) -> Vec<UiFeedFrameV1> {
    let mut frames = Vec::new();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), subscription.recv_frame())
            .await
            .expect("ui08 feed frame timeout")
            .expect("ui08 feed frame");
        assert_ne!(
            frame.kind,
            UiFeedFrameKind::Gap,
            "a live subscription must not be served a gap"
        );
        let terminal = frame.terminal;
        frames.push(frame);
        if terminal {
            return frames;
        }
    }
}

fn cursor(instance: &str, epoch: &str, sequence: u64) -> UiFeedCursorV1 {
    UiFeedCursorV1::new(
        instance,
        epoch,
        sequence,
        UiCursor {
            epoch: epoch.to_owned(),
            sequence: 1,
        },
    )
    .expect("ui08 fixture cursor")
}

/// 只看位置的 marker 帧（boundary / heartbeat），没有 entity 载荷。
fn marker_frame(
    instance: &str,
    epoch: &str,
    sequence: u64,
    event_id: &str,
    kind: UiFeedFrameKind,
) -> UiFeedFrameV1 {
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind,
        cursor: cursor(instance, epoch, sequence),
        event_id: event_id.to_owned(),
        replay: false,
        terminal: false,
        event: None,
        gap: None,
    };
    frame.validate().expect("ui08 marker frame is valid");
    frame
}

/// surface adapter 的显式投影：把真实帧的位置包上 entity 命名空间，供 store 使用。
///
/// 本轮 daemon 只发布 run-stream 帧；这里手工补齐命名空间，正是为了证明「谁提供身份谁负责」：
/// store 不会自己猜，也不会把展示文本写成实体。
#[allow(clippy::too_many_arguments)]
fn entity_frame(
    instance: &str,
    epoch: &str,
    sequence: u64,
    event_id: &str,
    entity_kind: &str,
    entity_id: &str,
    session_id: &str,
    revision: u64,
    terminal: bool,
) -> UiFeedFrameV1 {
    let frame = UiFeedFrameV1 {
        schema: UI_FEED_FRAME_SCHEMA.to_owned(),
        kind: if terminal {
            UiFeedFrameKind::Terminal
        } else {
            UiFeedFrameKind::Delta
        },
        cursor: cursor(instance, epoch, sequence),
        event_id: event_id.to_owned(),
        replay: false,
        terminal,
        event: Some(json!({
            "entity_kind": entity_kind,
            "entity_id": entity_id,
            "session_id": session_id,
            "revision": revision,
        })),
        gap: None,
    };
    frame.validate().expect("ui08 entity frame is valid");
    frame
}

/// 真实 run 的 live 投影夹具：宿主、模型、run id 与按位置排序的全部 live 帧。
struct LiveRun {
    host: Arc<DaemonHost>,
    model: Arc<StreamingModel>,
    run_id: RunId,
    instance: String,
    epoch: String,
    frames: Vec<UiFeedFrameV1>,
    // 环境隔离必须在宿主之后释放，因此声明在最后。
    _home: EnvGuard,
    _distill: EnvGuard,
}

impl LiveRun {
    fn session_id(&self) -> String {
        self.run_id.to_string()
    }

    /// 按真实位置把全部 live 帧投影进一个 tab-local store：位置来自服务端，命名空间由调用方补齐。
    fn project_into(&self, store: &mut UiEntityStore) {
        for (index, frame) in self.frames.iter().enumerate() {
            let projected = if frame.kind == UiFeedFrameKind::SnapshotBoundary {
                marker_frame(
                    &self.instance,
                    &self.epoch,
                    frame.cursor.feed_sequence,
                    &frame.event_id,
                    UiFeedFrameKind::SnapshotBoundary,
                )
            } else {
                entity_frame(
                    &self.instance,
                    &self.epoch,
                    frame.cursor.feed_sequence,
                    &format!("{}-entity", frame.event_id),
                    "run",
                    &self.session_id(),
                    &self.session_id(),
                    frame.cursor.feed_sequence,
                    frame.kind == UiFeedFrameKind::Terminal,
                )
            };
            let frame_scope = store.scope().clone();
            store
                .apply(UiStoreEvent::feed(frame_scope, projected))
                .unwrap_or_else(|error| panic!("live frame {index} must be projectable: {error}"));
        }
    }
}

async fn live_run(label: &str, chunks: usize) -> LiveRun {
    let home = temp_dir(&format!("{label}-home"));
    let root = temp_dir(&format!("{label}-root"));
    let home_guard = EnvGuard::set("KIANA_HOME", &home);
    let distill_guard = EnvGuard::remove("KIANA_MEMORY_DISTILL_AUTO");
    let model = StreamingModel::new(chunks);
    let host = host_with_model(model.clone());
    let run_id = RunId::new();
    let mut live = host.subscribe_run_feed_after(run_id, None);
    let boundary = live.recv_frame().await.expect("boundary frame");
    assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
    assert_eq!(boundary.cursor.feed_sequence, 0);
    let response = host.handle(run_envelope(&root, run_id)).await;
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response_run_id(&response), run_id);
    let mut frames = drain_frames(&mut live).await;
    assert_eq!(
        frames.last().map(|frame| frame.kind.clone()),
        Some(UiFeedFrameKind::Terminal)
    );
    frames.insert(0, boundary);
    let instance = host.feed_instance_id();
    let epoch = host.ui_cursor().epoch;
    LiveRun {
        host,
        model,
        run_id,
        instance,
        epoch,
        frames,
        _home: home_guard,
        _distill: distill_guard,
    }
}

#[tokio::test]
async fn raw_run_stream_frames_are_refused_without_an_entity_envelope() {
    let _environment_lock = environment_lock();
    let live = live_run("raw-frame", 3).await;
    let facts_before = live
        .host
        .persisted_events()
        .await
        .expect("ledger read")
        .expect("memory ledger is readable");

    let mut store = UiEntityStore::new(
        UiStoreScope::new("/workspace", live.session_id(), "tab-1").expect("ui08 store scope"),
        64,
    )
    .expect("ui08 store");
    // The store starts unhydrated: the boundary is the position marker that lets a following
    // delta reach entity parsing instead of failing closed with `GapRequired`.
    let boundary = live
        .frames
        .iter()
        .find(|frame| frame.kind == UiFeedFrameKind::SnapshotBoundary)
        .cloned()
        .expect("live run starts with a boundary");
    let boundary_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(boundary_scope, boundary.clone()))
            .expect("the daemon's initial boundary is adoptable"),
        UiStoreChange::SnapshotHydrated
    );
    let mut entity_bearing = 0usize;
    for frame in &live.frames {
        if matches!(
            frame.kind,
            UiFeedFrameKind::SnapshotBoundary | UiFeedFrameKind::Heartbeat
        ) {
            // Position marker frames carry no entity payload; only data frames are refused for
            // the missing entity namespace this test proves.
            continue;
        }
        // 真实 run-stream 载荷只有 RunStreamEvent，没有 entity 命名空间：先证明这一点，
        // 再证明 store 拒绝它，而不是把它猜成 run 实体。
        let payload = frame.event.as_ref().expect("live frames carry a payload");
        let object = payload
            .as_object()
            .expect("run-stream payload is an object");
        assert!(
            !object.contains_key("entity_kind") && !object.contains_key("entity_id"),
            "a raw run-stream frame must not carry an entity envelope: {payload}"
        );
        entity_bearing += 1;
        // Each refused frame leaves the shared store's position untouched (a refusal is not a
        // fact), so a later frame would trip the sequence fence before entity parsing. Probe with
        // a fresh hydrated store per frame so every raw payload is proven to fail on its missing
        // entity namespace rather than on the gap the previous refusal created.
        let mut probe = UiEntityStore::new(store.scope().clone(), 64).expect("ui08 probe store");
        let probe_scope = probe.scope().clone();
        let position = frame.cursor.feed_sequence;
        let preceding = marker_frame(
            &live.instance,
            &live.epoch,
            position.saturating_sub(1),
            "probe-boundary",
            UiFeedFrameKind::SnapshotBoundary,
        );
        probe
            .apply(UiStoreEvent::feed(probe_scope, preceding))
            .expect("probe boundary");
        let frame_scope = probe.scope().clone();
        match probe.apply(UiStoreEvent::feed(frame_scope, frame.clone())) {
            Err(UiStoreError::Invalid(code)) => {
                assert_eq!(code, "ui_feed_entity_kind_missing");
            }
            other => panic!("a raw run-stream frame must be refused, got {other:?}"),
        }
    }
    assert!(
        entity_bearing >= 2,
        "the run must publish more than one frame"
    );
    assert!(store.is_empty());
    assert_eq!(store.count_of_kind(UiEntityKind::Run), 0);
    // 边界帧本身是位置 marker：store 采纳了它，但采纳它不会凭空造出实体，也不移动位置。
    assert_eq!(store.epoch(), live.epoch.as_str());
    assert_eq!(store.feed_sequence(), 0);

    // 投影不追加事实，也不重新调用模型：事实仍归 EventStore/ControlPlane。
    assert_eq!(
        live.host
            .persisted_events()
            .await
            .expect("ledger read")
            .expect("memory ledger is readable")
            .len(),
        facts_before.len()
    );
    assert!(live.model.calls.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn store_projection_is_scope_bound_and_never_writes_facts() {
    let _environment_lock = environment_lock();
    let live = live_run("scope", 2).await;
    let facts_before = live
        .host
        .persisted_events()
        .await
        .expect("ledger read")
        .expect("memory ledger is readable");

    let mut tab_one = UiEntityStore::new(
        UiStoreScope::new("/workspace", live.session_id(), "tab-1").expect("tab-1 scope"),
        4,
    )
    .expect("tab-1 store");
    let mut tab_two = UiEntityStore::new(
        UiStoreScope::new("/workspace", live.session_id(), "tab-2").expect("tab-2 scope"),
        4,
    )
    .expect("tab-2 store");

    // 用真实帧的位置与载荷喂第一张表，并把同一帧交给第二张表：tab 不匹配必须拒绝。
    for (index, frame) in live.frames.iter().enumerate() {
        let projected = if frame.kind == UiFeedFrameKind::SnapshotBoundary {
            marker_frame(
                &live.instance,
                &live.epoch,
                frame.cursor.feed_sequence,
                &frame.event_id,
                UiFeedFrameKind::SnapshotBoundary,
            )
        } else {
            entity_frame(
                &live.instance,
                &live.epoch,
                frame.cursor.feed_sequence,
                &format!("{}-entity", frame.event_id),
                "run",
                &live.session_id(),
                &live.session_id(),
                frame.cursor.feed_sequence,
                frame.kind == UiFeedFrameKind::Terminal,
            )
        };
        let tab_one_scope = tab_one.scope().clone();
        let change = tab_one
            .apply(UiStoreEvent::feed(tab_one_scope, projected.clone()))
            .unwrap_or_else(|error| {
                panic!("tab-1 must accept the projected frame {index}: {error}")
            });
        if index == 0 {
            assert_eq!(change, UiStoreChange::SnapshotHydrated);
        }
        // The frame is bound to tab-1's scope; handing it to tab-2 must fail closed instead of
        // projecting the same authoritative fact into a second tab.
        let tab_one_scope = tab_one.scope().clone();
        assert_eq!(
            tab_two
                .apply(UiStoreEvent::feed(tab_one_scope, projected))
                .unwrap_err(),
            UiStoreError::ScopeMismatch
        );
    }
    assert!(
        live.frames.len() >= 3,
        "the run must publish several frames"
    );
    let run_key =
        UiEntityKey::new(UiEntityKind::Run, live.session_id()).expect("ui08 run entity key");
    let terminal_sequence = live
        .frames
        .last()
        .expect("terminal frame")
        .cursor
        .feed_sequence;
    assert_eq!(tab_one.len(), 1);
    assert_eq!(tab_one.count_of_kind(UiEntityKind::Run), 1);
    assert_eq!(
        tab_one.entity(&run_key).expect("projected run").revision,
        terminal_sequence
    );
    assert!(tab_two.is_empty());

    // 本地 pending 状态只属于它自己的 scope，并且永远不会被投影成服务端事实。
    let command_key =
        UiEntityKey::new(UiEntityKind::Submission, "ui08-command-1").expect("submission key");
    tab_one
        .apply(UiStoreEvent::Optimistic(UiOptimisticUpdate {
            idempotency_key: "ui08-command-1".to_owned(),
            command_id: RequestId::new(),
            key: command_key.clone(),
            revision: 1,
            value: json!({"status": "sending"}),
            expires_at_unix_ms: u64::MAX,
            previous: None,
        }))
        .expect("tab-1 stages a pending command");
    assert_eq!(tab_one.pending_count(), 1);
    assert_eq!(
        tab_one
            .entity(&command_key)
            .expect("pending submission")
            .lifecycle,
        UiEntityLifecycle::OptimisticPending
    );
    assert_eq!(tab_two.pending_count(), 0);

    // 投影仍然只是读：不追加事实，也不重新执行模型。
    assert_eq!(
        live.host
            .persisted_events()
            .await
            .expect("ledger read")
            .expect("memory ledger is readable")
            .len(),
        facts_before.len()
    );
    assert!(live.model.calls.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn real_gap_fences_later_deltas_until_a_snapshot_and_writes_no_facts() {
    let _environment_lock = environment_lock();
    let live = live_run("gap", 3).await;
    let facts_before = live
        .host
        .persisted_events()
        .await
        .expect("ledger read")
        .expect("memory ledger is readable");

    let mut store = UiEntityStore::new(
        UiStoreScope::new("/workspace", live.session_id(), "tab-1").expect("ui08 store scope"),
        8,
    )
    .expect("ui08 store");
    // 按真实位置投影到终态：位置与 revision 全部来自服务端帧。
    live.project_into(&mut store);
    let run_key =
        UiEntityKey::new(UiEntityKind::Run, live.session_id()).expect("ui08 run entity key");
    let terminal_sequence = live
        .frames
        .last()
        .expect("terminal frame")
        .cursor
        .feed_sequence;
    assert_eq!(
        store.entity(&run_key).expect("projected run").revision,
        terminal_sequence
    );
    assert!(!store.needs_snapshot());

    // 旧 epoch 的 cursor 只能拿到 gap：store 必须要求 snapshot，而不是继续吃 delta。
    let stale = cursor(&live.instance, "epoch-retired", 1);
    let mut stale_stream = live
        .host
        .subscribe_run_feed_after(live.run_id, Some(&stale));
    let gap = stale_stream.recv_frame().await.expect("old epoch gap");
    assert_eq!(gap.kind, UiFeedFrameKind::Gap);
    assert!(gap.gap.as_ref().expect("gap range").snapshot_required);
    let gap_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(gap_scope, gap))
            .expect("the gap frame itself is recorded"),
        UiStoreChange::SnapshotRequired
    );
    assert!(store.needs_snapshot());
    assert_eq!(store.feed_sequence(), terminal_sequence);
    // gap 只记位置，不回退本地位置，也不假装已经追上：同 epoch 的权威视图保留，delta 被围栏。
    assert_eq!(
        store.entity(&run_key).expect("projected run").revision,
        terminal_sequence
    );

    // gap 之后的 boundary 只报告服务端当前位置；它不能解除 snapshot 要求。
    let boundary = stale_stream.recv_frame().await.expect("boundary after gap");
    assert_eq!(boundary.kind, UiFeedFrameKind::SnapshotBoundary);
    let boundary_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(boundary_scope, boundary.clone()))
            .expect("the boundary after a gap is a position marker"),
        UiStoreChange::SnapshotRequired
    );
    assert_eq!(store.epoch(), boundary.cursor.authority_epoch.as_str());
    assert_eq!(store.feed_sequence(), boundary.cursor.feed_sequence);
    assert!(store.needs_snapshot());

    let post_gap_epoch = store.epoch().to_owned();
    let fenced = entity_frame(
        &live.instance,
        &post_gap_epoch,
        boundary.cursor.feed_sequence + 1,
        "post-gap-entity",
        "run",
        &live.session_id(),
        &live.session_id(),
        1,
        false,
    );
    let fenced_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(fenced_scope, fenced))
            .unwrap_err(),
        UiStoreError::GapRequired
    );

    // marker 帧不会因为被围栏而报错：心跳只更新位置，不制造实体。
    let heartbeat = live
        .host
        .feed_heartbeat(live.run_id)
        .expect("bounded heartbeat");
    assert_eq!(heartbeat.kind, UiFeedFrameKind::Heartbeat);
    let heartbeat_scope = store.scope().clone();
    assert_eq!(
        store
            .apply(UiStoreEvent::feed(heartbeat_scope, heartbeat))
            .expect("a heartbeat is a position marker"),
        UiStoreChange::EntityUpdated
    );
    assert_eq!(store.len(), 1);
    assert_eq!(store.count_of_kind(UiEntityKind::Run), 1);

    // 事实层完全没被 projection 触碰。
    assert_eq!(
        live.host
            .persisted_events()
            .await
            .expect("ledger read")
            .expect("memory ledger is readable")
            .len(),
        facts_before.len()
    );
    assert!(live.model.calls.load(Ordering::SeqCst) >= 1);
}
