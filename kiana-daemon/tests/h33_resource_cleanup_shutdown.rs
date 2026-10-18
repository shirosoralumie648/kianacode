//! H33 daemon 关闭夹具：先取消在飞 run 并写清理证据，再 flush，最后关闭 EventStore。
//!
//! 这里只走组合根自带的路径：
//!
//! ```text
//! DaemonHost::with_runner_and_events
//!   -> ControlPlane::start_run（RunnerPort::Start 阻塞，run 真正在飞）
//!   -> DaemonHost::shutdown
//!        -> ControlPlane::shutdown_in_flight_runs   （Shutdown 清理报告，IsolatedUnknown）
//!        -> flush_event_store
//!        -> drain_observability_bounded            （没有消费者 -> 取消）
//!        -> record_shutdown_log_failures           （LogFailure 清理报告）
//!        -> flush_event_store
//!        -> close_event_store
//! ```
//!
//! 拒绝优先：第一个用例证明关闭**不能**把仍在飞的模型任务报告成「已停止」；第二个用例证明
//! 两类报告都在 `close()` 之前落账。reducer 不杀进程、不释放 OS 锁，Host drop 也不是停止确认。
//!
//! 证明上限是 local_behavior：EventStore 是进程内适配器，Runner 是阻塞夹具，观测队列也在
//! 进程内。这里不产生 durable/跨进程证据。

use async_trait::async_trait;
use kiana_daemon::DaemonHost;
use kiana_domain::{EventStoreHealth, RequestId, RunId, RuntimeEvent};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{
    EventAppendResult, EventStorePort, ModelBudgetPort, ObservabilityQueueClass, PortError,
    QueuedObservabilityItem, RunnerPort,
};
use kiana_protocol::{ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use kiana_types::ProjectTrust;
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

/// `KIANA_HOME` 是进程级变量；同二进制内的用例串行持有它，并在 Drop 中恢复。
static ENV_LOCK: Mutex<()> = Mutex::new(());

const SHUTDOWN_REASON: &str = "model_task_shutdown_unconfirmed";
const LOG_FAILURE_REASON: &str = "observability_flush_cancelled";
const CLEANUP_KIND: &str = "resource.cleanup_report";

struct IsolatedHome {
    _lock: MutexGuard<'static, ()>,
    previous: Option<OsString>,
    root: PathBuf,
    project: PathBuf,
}

impl IsolatedHome {
    fn new(label: &str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-h33-daemon-{label}-{stamp}"));
        let home = root.join("kiana-home");
        fs::create_dir_all(&home).expect("kiana home");
        let project = root.join("project");
        fs::create_dir_all(project.join(".git")).expect("project fixture");
        let previous = std::env::var_os("KIANA_HOME");
        std::env::set_var("KIANA_HOME", &home);
        kiana_types::write_project_trust(&project, ProjectTrust::Trusted).expect("trust fixture");
        Self {
            _lock: lock,
            previous,
            root,
            project,
        }
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        match self.previous.as_ref() {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// 记录关闭时刻的账本快照：`snapshot` 只在 `close()` 内采集，因此它证明清理报告先于 close。
struct RecordingEventStore {
    inner: MemoryEventLog,
    close_started: AtomicBool,
    closes: AtomicUsize,
    snapshot: Mutex<Option<Vec<RuntimeEvent>>>,
}

impl RecordingEventStore {
    fn new() -> Self {
        Self {
            inner: MemoryEventLog::new(),
            close_started: AtomicBool::new(false),
            closes: AtomicUsize::new(0),
            snapshot: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> Option<Vec<RuntimeEvent>> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    async fn health_at(&self, closed: bool) -> Result<EventStoreHealth, PortError> {
        let cursor = self.inner.read_all().await?.len() as u64;
        Ok(EventStoreHealth::new(2, true, false, cursor, closed))
    }
}

#[async_trait]
impl EventStorePort for RecordingEventStore {
    fn supports_atomic_transitions(&self) -> bool {
        self.inner.supports_atomic_transitions()
    }

    fn capabilities(&self) -> kiana_domain::EventStoreCapabilities {
        self.inner.capabilities()
    }

    async fn flush(&self) -> Result<EventStoreHealth, PortError> {
        // 进程内适配器没有持久化屏障；只报告进程内提交边界，不冒充 durable。
        self.health_at(false).await
    }

    async fn health(&self) -> Result<EventStoreHealth, PortError> {
        self.health_at(false).await
    }

    async fn close(&self) -> Result<EventStoreHealth, PortError> {
        self.close_started.store(true, Ordering::SeqCst);
        self.closes.fetch_add(1, Ordering::SeqCst);
        let events = self.inner.read_all().await?;
        *self
            .snapshot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(events);
        self.health_at(true).await
    }

    async fn commit_transition(
        &self,
        batch: kiana_domain::TransitionBatch,
    ) -> Result<kiana_domain::CommitOutcome, PortError> {
        self.inner.commit_transition(batch).await
    }

    async fn read_command(
        &self,
        id: &RequestId,
    ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
        self.inner.read_command(id).await
    }

    async fn read_from(
        &self,
        cursor: u64,
        limit: usize,
    ) -> Result<kiana_domain::JournalPage, PortError> {
        self.inner.read_from(cursor, limit).await
    }

    async fn cursor_snapshot(&self) -> Result<kiana_domain::EventLogCursorSnapshot, PortError> {
        self.inner.cursor_snapshot().await
    }

    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.inner.append(event).await
    }

    async fn append_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<(), PortError> {
        self.inner.append_expected(event, expected).await
    }

    async fn append_idempotent(&self, event: RuntimeEvent) -> Result<EventAppendResult, PortError> {
        self.inner.append_idempotent(event).await
    }

    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        self.inner.append_idempotent_expected(event, expected).await
    }

    async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_request(id).await
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_all().await
    }

    async fn read_stream(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        self.inner.read_stream(aggregate_type, aggregate_id).await
    }
}

/// `Start` 一直挂起，让 run 在关闭时**确实**还在飞；`Cancel` 只记录已送达并立即返回。
///
/// 这里刻意不执行任何外部副作用：它证明的是「关闭会取消并写证据」，不是「外部进程已停」。
struct BlockingRunner {
    started: Notify,
    run_id: Mutex<Option<RunId>>,
    cancel_calls: AtomicUsize,
}

impl BlockingRunner {
    fn new() -> Self {
        Self {
            started: Notify::new(),
            run_id: Mutex::new(None),
            cancel_calls: AtomicUsize::new(0),
        }
    }

    fn run_id(&self) -> Option<RunId> {
        *self
            .run_id
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

#[async_trait]
impl RunnerPort for BlockingRunner {
    fn bind_model_assignment(
        &self,
        run_id: RunId,
        assignment: kiana_domain::ModelAssignment,
    ) -> Result<(), PortError> {
        if assignment.run_id != run_id {
            return Err(PortError::Failed(
                "h33_fixture_assignment_run_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn install_model_budget(&self, _budget: Arc<dyn ModelBudgetPort>) -> Result<(), PortError> {
        // 组合根要求 RunnerPort 接受预算端口；本夹具不调用模型，所以只接收不记账。
        Ok(())
    }

    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        match command {
            RunnerCommand::Start { run_id, .. } => {
                *self
                    .run_id
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) = Some(run_id);
                self.started.notify_one();
                // 永不完成：关闭只能发取消，不能把「发过取消」当成停止确认。
                std::future::pending::<()>().await;
                Ok(Vec::new())
            }
            RunnerCommand::Cancel { .. } => {
                self.cancel_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }
}

fn metadata_in(project: &Path) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(
        "h33-shutdown-session",
        project.to_string_lossy().into_owned(),
    );
    metadata.project_trusted = true;
    metadata.permission_profile = PermissionProfile::Balanced;
    metadata
}

struct Fixture {
    _home: IsolatedHome,
    host: Arc<DaemonHost>,
    store: Arc<RecordingEventStore>,
    runner: Arc<BlockingRunner>,
    task: tokio::task::JoinHandle<()>,
    run_id: RunId,
}

async fn start_in_flight_run(home: IsolatedHome) -> Fixture {
    let store = Arc::new(RecordingEventStore::new());
    let runner = Arc::new(BlockingRunner::new());
    let host = Arc::new(
        DaemonHost::with_runner_and_events(runner.clone(), store.clone())
            .expect("daemon with blocking runner"),
    );
    let project = home.project.clone();
    let request_host = host.clone();
    let task = tokio::spawn(async move {
        let response = request_host
            .handle(RequestEnvelope::run(
                metadata_in(&project),
                "h33 shutdown fixture",
                Some("read-only".to_owned()),
            ))
            .await;
        // Start 永不返回，因此正常路径不会到达这里；若到达则说明夹具失效。
        assert_ne!(response.status, ExecutionStatus::Completed);
    });
    tokio::time::timeout(Duration::from_secs(5), runner.started.notified())
        .await
        .expect("run must reach the runner Start command");
    let run_id = runner.run_id().expect("runner saw Start");
    Fixture {
        _home: home,
        host,
        store,
        runner,
        task,
        run_id,
    }
}

fn cleanup_reports(events: &[RuntimeEvent], run_id: RunId) -> Vec<Value> {
    let run = run_id.to_string();
    events
        .iter()
        .filter(|event| event.kind == CLEANUP_KIND)
        .filter(|event| event.data["run"].as_str() == Some(run.as_str()))
        .map(|event| event.data.clone())
        .collect()
}

fn resource<'a>(report: &'a Value, kind: &str) -> &'a Value {
    resources(report)
        .into_iter()
        .find(|resource| resource["kind"].as_str() == Some(kind))
        .unwrap_or_else(|| panic!("missing {kind} resource in {report}"))
}

fn resource_by_id<'a>(report: &'a Value, id: &str) -> &'a Value {
    resources(report)
        .into_iter()
        .find(|resource| resource["resource_id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("missing {id} resource in {report}"))
}

fn resources(report: &Value) -> Vec<&Value> {
    report["report"]["resources"]
        .as_array()
        .map(|resources| resources.iter().collect())
        .unwrap_or_default()
}

/// 【拒绝】关闭不能把仍在飞的模型任务报告成「已停止」。
///
/// `Shutdown` 报告必须保留 `IsolatedUnknown`、`safe_to_release_all == false`，并且
/// `LogFailure` 只追加第二条独立事实，不覆盖停止未知。
#[tokio::test]
async fn shutdown_cannot_claim_in_flight_run_stopped() {
    let home = IsolatedHome::new("refuse");
    let fixture = start_in_flight_run(home).await;
    fixture
        .host
        .try_enqueue_observability(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Event,
            1,
        ))
        .expect("observability item");

    let health = fixture.host.shutdown().await.expect("shutdown");
    assert!(health.closed, "close() must acknowledge closed state");
    assert_eq!(fixture.store.closes.load(Ordering::SeqCst), 1);
    assert!(fixture.store.close_started.load(Ordering::SeqCst));

    let events = fixture.store.snapshot().expect("close-time snapshot");
    let reports = cleanup_reports(&events, fixture.run_id);
    assert!(
        reports.len() >= 2,
        "expected shutdown + log failure: {reports:?}"
    );

    let shutdown = reports
        .iter()
        .find(|report| report["cause"] == "shutdown")
        .expect("shutdown cleanup report");
    assert_eq!(shutdown["safe_to_release_all"], Value::Bool(false));
    assert_eq!(shutdown["released_count"], 0);
    let model_task = resource(shutdown, "model_task");
    assert_eq!(model_task["status"], "isolated_unknown");
    assert_eq!(model_task["reason"], SHUTDOWN_REASON);
    assert_eq!(model_task["stop_confirmed"], Value::Bool(false));
    assert_eq!(model_task["effect_known"], Value::Bool(false));

    // 取消消息确实送达 runner，但它只表示「已请求停止」，不是停止证明。
    assert!(fixture.runner.cancel_calls.load(Ordering::SeqCst) >= 1);

    let log_failure = reports
        .iter()
        .find(|report| report["cause"] == "log_failure")
        .expect("log failure cleanup report");
    let subscription = resource_by_id(log_failure, "observability_subscription");
    assert_eq!(subscription["kind"], "subscription");
    assert_eq!(subscription["status"], "isolated_unknown");
    assert_eq!(subscription["reason"], LOG_FAILURE_REASON);
    assert_eq!(log_failure["safe_to_release_all"], Value::Bool(false));

    fixture.task.abort();
    let _ = fixture.task.await;
}

/// 【成功】两类清理报告都在 `close_event_store` 之前落账，顺序为 Shutdown -> LogFailure。
#[tokio::test]
async fn shutdown_persists_cleanup_reports_before_event_store_close() {
    let home = IsolatedHome::new("order");
    let fixture = start_in_flight_run(home).await;
    fixture
        .host
        .try_enqueue_observability(QueuedObservabilityItem::critical(
            ObservabilityQueueClass::Terminal,
            7,
        ))
        .expect("observability item");

    let health = fixture.host.shutdown().await.expect("shutdown");
    assert!(health.closed);

    // 快照只在 close() 内部采集：能在其中看到报告，就证明它们先于 close 提交。
    let events = fixture.store.snapshot().expect("close-time snapshot");
    let reports = cleanup_reports(&events, fixture.run_id);
    let shutdown_index = reports
        .iter()
        .position(|report| report["cause"] == "shutdown")
        .expect("shutdown report persisted before close");
    let log_failure_index = reports
        .iter()
        .position(|report| report["cause"] == "log_failure")
        .expect("log failure report persisted before close");
    assert!(
        shutdown_index < log_failure_index,
        "shutdown evidence must precede the log-failure fact: {reports:?}"
    );

    // run 聚合不应出现清理报告：它是独立聚合，不能改变 run 终态。
    assert!(events.iter().all(|event| {
        !(event.kind == CLEANUP_KIND && event.aggregate_type.as_deref() == Some("run"))
    }));

    fixture.task.abort();
    let _ = fixture.task.await;
}
