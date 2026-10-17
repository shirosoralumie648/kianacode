//! ER-20 daemon-side fixtures: startup/restart reads the journal read-only and never resumes.
//!
//! The `DaemonHost` composition root owns startup: it must validate the facts journal before
//! exposing a recovery view, treat a corrupt journal as a refusal (not an empty store), and
//! rebuild only read models.  A restarted host over the same facts must stay paused.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::ControlPlane;
use kiana_daemon::DaemonHost;
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, PendingApproval, RequestContext, RequestId,
    RunId, RuntimeEvent,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &RequestContext,
        _: CapabilityRequest,
        _: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
    async fn activate(&self, _: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
    async fn consume(
        &self,
        _: &RequestContext,
        _: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("fixture_no_approval".to_owned()))
    }
}

struct ZeroCallRunner {
    sends: Arc<AtomicUsize>,
    restores: Arc<AtomicUsize>,
}

#[async_trait]
impl RunnerPort for ZeroCallRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable("restart_must_not_send".to_owned()))
    }

    async fn restore(&self, _run_id: RunId, _checkpoint: Value) -> Result<(), PortError> {
        self.restores.fetch_add(1, Ordering::SeqCst);
        Err(PortError::Unavailable(
            "restart_must_not_restore".to_owned(),
        ))
    }
}

fn event(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

/// A single well-formed legacy journal line, as PD-08 uses to build a corrupt store fixture.
fn legacy_event_line(run_id: &str) -> String {
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "run.started",
        json!({"run_id": run_id, "sequence": 1}),
    )
    .unwrap()
    .with_stream_metadata("run", run_id, 1);
    serde_json::to_string(&event).unwrap() + "\n"
}

fn zero_runner() -> (Arc<ZeroCallRunner>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let sends = Arc::new(AtomicUsize::new(0));
    let restores = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(ZeroCallRunner {
        sends: sends.clone(),
        restores: restores.clone(),
    });
    (runner, sends, restores)
}

async fn paused_store() -> Arc<MemoryEventLog> {
    let store = Arc::new(MemoryEventLog::new());
    let run_id = RunId::new();
    let events: Arc<dyn EventStorePort> = store.clone();
    for event in [
        event(1, "run.authorized", json!({"run_id": run_id})),
        event(2, "approval.requested", json!({"run_id": run_id})),
    ] {
        events.append(event).await.expect("append paused fact");
    }
    store
}

fn host(store: Arc<MemoryEventLog>, runner: Arc<dyn RunnerPort>) -> DaemonHost {
    let core = Arc::new(ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        store,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        runner,
    ));
    DaemonHost::new(core)
}

fn unique_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-er20-daemon-{label}-{stamp}.jsonl"))
}

fn write_private(path: &Path, contents: &[u8]) {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(contents).unwrap();
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents).unwrap();
    }
}

/// 【拒绝】启动健康/就绪只读重建重开日志，不自动恢复待批 run，也不追加事实。
#[tokio::test]
async fn er20_daemon_restart_reads_journal_without_resuming() {
    let store = paused_store().await;
    let (runner, sends, restores) = zero_runner();
    let initial = host(store.clone(), runner);
    let before = store.read_all().await.unwrap().len();

    initial.startup_health().await.expect("startup health");
    initial.readiness().await.expect("readiness");

    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(restores.load(Ordering::SeqCst), 0);
    assert_eq!(store.read_all().await.unwrap().len(), before);

    // Drop/new host over the same facts must stay read-only and paused.
    let (new_runner, new_sends, _new_restores) = zero_runner();
    let restarted = host(store.clone(), new_runner);
    restarted.startup_health().await.expect("restart health");
    assert_eq!(new_sends.load(Ordering::SeqCst), 0);
    assert_eq!(store.read_all().await.unwrap().len(), before);
}

/// 【拒绝】损坏 journal 在暴露恢复状态前被拒绝，且与合法空 store 可区分。
#[tokio::test]
async fn er20_daemon_refuses_corrupt_journal_before_recovery_state() {
    let store = paused_store().await;
    let (runner, _sends, _restores) = zero_runner();
    let host = host(store, runner);

    // Missing journal is a valid, empty store and is admitted.
    let empty = unique_path("empty");
    host.admit_storage_integrity(&empty)
        .await
        .expect("empty journal admitted");

    // A corrupt middle frame is refused, never treated as an empty store.
    let corrupt = unique_path("corrupt");
    let contents = legacy_event_line("er20-corrupt") + "this is not a json record\n";
    write_private(&corrupt, contents.as_bytes());
    let error = host
        .admit_storage_integrity(&corrupt)
        .await
        .expect_err("corrupt journal must be refused");
    assert!(
        error.to_string().contains("eventlog_integrity"),
        "unexpected refusal: {error}"
    );

    let _ = std::fs::remove_file(&corrupt);
    let _ = std::fs::remove_file(&corrupt.with_extension("jsonl.lock"));
}
