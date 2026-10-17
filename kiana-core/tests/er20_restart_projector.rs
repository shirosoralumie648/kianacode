//! ER-20 behavioural fixtures: restart projection rebuild and default pause.
//!
//! The `er20_restart_projector_guard` source guard proves the shape of the restart read path;
//! these fixtures exercise it against real durable/event stores and projections.  Reopening a
//! journal must rebuild pending/Unknown/resource state read-only: never resume a paused run,
//! never reopen authority, never fabricate an empty "all clear" view from a corrupt source, and
//! keep the restart reconstruction deterministic across a drop/new-host cycle.

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{
    project_operational_metrics, project_recovery_resources, project_run_state, ControlPlane,
    ProjectionDriver, ProjectionDriverStatus, ReplayProjection, RunPhase,
};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, ApprovalState, CapabilityRequest, CellId, MetricSnapshot,
    PendingApproval, RequestContext, RequestId, RunId, RuntimeEvent, StorageLockId,
};
use kiana_eventlog::JsonlEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, EventStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Approval store that never has usable material: the restart read path must not need it.
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

/// Runner that only records whether the restart path (illegally) advanced it.
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

fn plane(events: Arc<dyn EventStorePort>, runner: Arc<dyn RunnerPort>) -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        events,
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        runner,
    )
}

fn event(sequence: u64, kind: &str, data: Value) -> RuntimeEvent {
    // No stream metadata: the storage layer then preserves append order instead of applying the
    // domain event contract, which is irrelevant to a read-only restart rebuild.
    RuntimeEvent::new(RequestId::new(), sequence, kind, data).unwrap()
}

fn unique_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-er20-{label}-{stamp}.jsonl"))
}

fn metric_value(snapshot: &MetricSnapshot, name: &str) -> f64 {
    snapshot
        .points
        .iter()
        .find(|point| point.name == name)
        .map(|point| point.value)
        .expect("registered metric point")
}

/// 【拒绝】重开日志后的只读重建不得自动恢复待批 run，也不得推进 Runner。
#[tokio::test]
async fn er20_restart_never_auto_resumes_pending_run() {
    let path = unique_path("restart");
    let run_id = RunId::new();
    let approval_id = ApprovalId::new();
    {
        let events: Arc<dyn EventStorePort> =
            Arc::new(JsonlEventLog::open(&path).expect("open durable journal"));
        for event in [
            event(1, "run.authorized", json!({"run_id": run_id})),
            event(
                2,
                "approval.requested",
                json!({"run_id": run_id, "approval_id": approval_id}),
            ),
            event(
                3,
                "approval.staged",
                json!({"approval_id": approval_id, "state": ApprovalState::Staged}),
            ),
            event(
                4,
                "approval.activated",
                json!({"approval_id": approval_id, "state": ApprovalState::Active}),
            ),
        ] {
            events.append(event).await.expect("append restart fact");
        }
    }

    // Drop and reopen: a new host over the same facts must only rebuild read models.
    let events: Arc<dyn EventStorePort> =
        Arc::new(JsonlEventLog::open(&path).expect("reopen durable journal"));
    let sends = Arc::new(AtomicUsize::new(0));
    let restores = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(ZeroCallRunner {
        sends: sends.clone(),
        restores: restores.clone(),
    });
    let core = plane(events.clone(), runner);
    let before = events.read_all().await.unwrap();

    let resources = core.recovery_resources().await.expect("resource rebuild");
    assert_eq!(resources.pending_approval_ids, vec![approval_id]);

    let state = project_run_state(run_id, &before).expect("run projection");
    assert_eq!(state.phase, RunPhase::AwaitingApproval);
    assert!(state.outcome.is_none());

    // Default pause: no auto-resume, no restore, and no new facts appended by the rebuild.
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(restores.load(Ordering::SeqCst), 0);
    let after = events.read_all().await.unwrap();
    assert_eq!(before.len(), after.len());
    for kind in [
        "execution.prepared",
        "invocation.dispatching",
        "run.resume_prepared",
    ] {
        assert!(
            after.iter().all(|event| event.kind.as_str() != kind),
            "restart rebuild must not append {kind}"
        );
    }

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}

/// 【成功/回归】重建可从同一事实列出多个待批审批并冻结资源，且只读。
#[test]
fn er20_rebuild_lists_multiple_pending_approvals_and_fences_resources() {
    let approval_a = ApprovalId::new();
    let approval_b = ApprovalId::new();
    let lease_id = StorageLockId::new();
    let cell_id = CellId::new();
    let events = vec![
        event(
            1,
            "approval.staged",
            json!({"approval_id": approval_a, "state": ApprovalState::Staged}),
        ),
        event(
            2,
            "approval.activated",
            json!({"approval_id": approval_a, "state": ApprovalState::Active}),
        ),
        event(
            3,
            "approval.staged",
            json!({"approval_id": approval_b, "state": ApprovalState::Staged}),
        ),
        event(
            4,
            "approval.activated",
            json!({"approval_id": approval_b, "state": ApprovalState::Active}),
        ),
        event(5, "lease.issued", json!({"lease_id": lease_id})),
        event(
            6,
            "cell.started",
            json!({"cell_id": cell_id, "lifecycle": "running"}),
        ),
        event(
            7,
            "cell.cancel_requested",
            json!({"cell_id": cell_id, "lifecycle": "cancel_requested"}),
        ),
    ];
    let snapshot = project_recovery_resources(&events).unwrap();

    let mut pending = snapshot.pending_approval_ids.clone();
    pending.sort();
    let mut expected = vec![approval_a, approval_b];
    expected.sort();
    assert_eq!(pending, expected);
    assert_eq!(snapshot.active_resource_lease_ids, vec![lease_id]);
    assert_eq!(snapshot.fenced_cell_ids, vec![cell_id]);
    assert_eq!(snapshot.source_cursor, events.len() as u64);
}

/// 【拒绝】合法空 store 必须报告独立的「空事实源」，不能被当成重建成功或全清。
#[test]
fn er20_valid_empty_store_is_distinct_from_populated_rebuild() {
    assert_eq!(
        project_recovery_resources(&[]).unwrap_err(),
        "recovery_resource_source_empty"
    );

    let approval_id = ApprovalId::new();
    let populated = vec![
        event(
            1,
            "approval.staged",
            json!({"approval_id": approval_id, "state": ApprovalState::Staged}),
        ),
        event(
            2,
            "approval.activated",
            json!({"approval_id": approval_id, "state": ApprovalState::Active}),
        ),
    ];
    let snapshot = project_recovery_resources(&populated).unwrap();
    assert_eq!(snapshot.pending_approval_ids, vec![approval_id]);
}

/// 【拒绝/回归】投影 checkpoint 不匹配时拒绝 tail 恢复，并要求显式从零重建。
#[test]
fn er20_projection_checkpoint_mismatch_forces_rebuild() {
    let checkpoint =
        kiana_domain::ProjectionCheckpoint::new("projector-a", 2, Vec::new(), json!({"total": 2}))
            .unwrap();
    let fold = |state: Value, _event: &RuntimeEvent| -> Result<Value, String> { Ok(state) };
    assert_eq!(
        ReplayProjection::from_checkpoint("projector-b", &checkpoint, &[], 2, fold).unwrap_err(),
        "projection_checkpoint_projector_mismatch"
    );
    assert_eq!(
        ReplayProjection::from_checkpoint("projector-a", &checkpoint, &[], 1, fold).unwrap_err(),
        "projection_checkpoint_cursor_gap"
    );

    let sample = vec![event(1, "projection.add", json!({"n": 1}))];
    let mut driver = ProjectionDriver::new("projector-a").unwrap();
    driver
        .apply(json!({"total": 0}), &sample, 1, |mut state, _event| {
            state["total"] = json!(state["total"].as_u64().unwrap() + 1);
            Ok(state)
        })
        .unwrap();
    assert!(driver.checkpoint().is_some());
    driver.rebuild();
    assert_eq!(driver.status(), ProjectionDriverStatus::RebuildRequired);
    assert!(driver.checkpoint().is_none());
}

/// 【成功/回归】重启后 orphan dispatch（已分派但无终态）仍可见，不被吞掉。
#[test]
fn er20_orphan_dispatch_is_visible_after_restart() {
    let events = vec![event(
        1,
        "invocation.dispatching",
        json!({
            "source_cursor": 1,
            "request_id": "er20-orphan-request",
            "execution_id": "er20-orphan-execution",
            "attempt": 1,
            "effect_started": true,
        }),
    )];
    let snapshot = project_operational_metrics(&events, None).unwrap();
    assert_eq!(metric_value(&snapshot, "kiana.recovery.orphan_total"), 1.0);
    assert!(
        snapshot
            .limitations
            .iter()
            .any(|limitation| limitation == "orphan_dispatch_detected"),
        "{:?}",
        snapshot.limitations
    );
}

/// 【成功/回归】同一事实上的重建结果可确定性重复，不依赖进程内缓存或运行顺序。
#[test]
fn er20_restart_rebuild_is_deterministic() {
    let approval_id = ApprovalId::new();
    let events = vec![
        event(
            1,
            "approval.staged",
            json!({"approval_id": approval_id, "state": ApprovalState::Staged}),
        ),
        event(
            2,
            "approval.activated",
            json!({"approval_id": approval_id, "state": ApprovalState::Active}),
        ),
    ];
    assert_eq!(
        project_recovery_resources(&events).unwrap(),
        project_recovery_resources(&events).unwrap()
    );
}
