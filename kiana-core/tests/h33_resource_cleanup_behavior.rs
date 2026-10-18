//! H33 行为夹具：运行时清理生产者、panic 证据与短 run 基线回收。
//!
//! 这些夹具只断言本进程能举证的 owner 状态：`build_run_cleanup_report` 是运行时生产者，
//! 它把 run 结束、取消、panic 的 owner 证据归约为 `Released` / `IsolatedUnknown`；
//! reducer 不杀进程、不释放 OS 锁，也不把 drop 当成停止确认。
//!
//! 顺序：先拒绝（未确认停止不得交还写所有权、panic 不得留下未登记后台工作），
//! 再成功（多个短 run 回到句柄/锁/内存基线）。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{build_run_cleanup_report, ControlPlane, RunCleanupEvidence};
use kiana_domain::{
    ApprovalChallenge, ApprovalId, CapabilityRequest, CleanupCause, CleanupResourceKind,
    CleanupResourceStatus, ExecutionId, ExecutionStatus, ExecutionStopKind, ModelOutput,
    ModelRequest, PendingApproval, PermissionProfile, RequestContext, ResourceRetention,
    RunExecutionStopSet, RunId, StopSetResolution,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, PortError, RunnerPort};
use kiana_runner::{KianaHarness, ModelClient, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent, HARNESS_PANIC_ERROR};
use serde_json::json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 固定拒绝码：没有停止确认的资源不能释放可写 scope（路径锁/写集）。
const WRITABLE_SCOPE_REFUSAL: &str = "writable_scope_release_requires_confirmed_stop";
/// 固定拒绝码：路径锁只能在停止集合确认后交还。
const PATH_LOCK_REFUSAL: &str = "path_lock_release_requires_confirmed_stop";

fn temp_project() -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-h33-core-{stamp}"));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn trusted_context_in(root: &std::path::Path, session: &str) -> RequestContext {
    let mut context = RequestContext::local(session, root.to_string_lossy());
    context.project_trusted = true;
    context.permission_profile = PermissionProfile::Balanced;
    context
}

/// 只读 run 不走审批门；这里保持端口不可用而不是伪造审批。
struct NoApprovals;

#[async_trait]
impl ApprovalStorePort for NoApprovals {
    async fn stage(
        &self,
        _: &RequestContext,
        _: CapabilityRequest,
        _: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("h33_fixture_no_approval".to_owned()))
    }
    async fn activate(&self, _: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("h33_fixture_no_approval".to_owned()))
    }
    async fn consume(
        &self,
        _: &RequestContext,
        _: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("h33_fixture_no_approval".to_owned()))
    }
}

fn scripted_runner(outputs: serde_json::Value) -> Arc<KianaHarness> {
    Arc::new(KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&outputs).unwrap(),
    )))
}

fn core_with_runner(runner: Arc<dyn RunnerPort>) -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(NoApprovals),
        runner,
    )
}

/// 【拒绝】停止未确认的进程/路径锁不能把写所有权交给下一个 run。
///
/// 运行时生产者必须把仍欠停止确认的 execution 与 path lock 保持为 `IsolatedUnknown`，
/// 并且 `safe_to_release_all()` 为 false；下一个 run 只能从 `Confirmed` 的新集合开始，
/// 不能继承上一个 run 的「已释放」结论。
#[tokio::test]
async fn unconfirmed_process_cannot_release_write_ownership_to_next_run() {
    let run_id = RunId::new();
    let execution = ExecutionId::new();
    let mut stops = RunExecutionStopSet::new(run_id);
    stops
        .register(execution, ExecutionStopKind::Capability)
        .unwrap();
    stops.begin_stop();
    stops.mark_stop_unconfirmed(execution).unwrap();
    assert_eq!(stops.resolution(), StopSetResolution::Unconfirmed);
    assert_eq!(
        stops.release_writable_scope(execution).unwrap_err(),
        WRITABLE_SCOPE_REFUSAL
    );

    let report = build_run_cleanup_report(&RunCleanupEvidence {
        run_id,
        cause: CleanupCause::Cancel,
        in_flight: true,
        path_locks_held: true,
        stops: Some(&stops),
        panic: None,
        log_failure: None,
    })
    .unwrap();

    assert_eq!(report.cause, CleanupCause::Cancel);
    assert!(!report.safe_to_release_all());
    assert_eq!(report.released_count, 0);

    let path_lock = report
        .resources
        .iter()
        .find(|resource| resource.kind == CleanupResourceKind::PathLock)
        .expect("path lock observation missing");
    assert_eq!(path_lock.status, CleanupResourceStatus::IsolatedUnknown);
    assert_eq!(path_lock.reason.as_deref(), Some(PATH_LOCK_REFUSAL));

    let execution_resource = report
        .resources
        .iter()
        .find(|resource| resource.kind == CleanupResourceKind::ToolFuture)
        .expect("execution observation missing");
    assert_eq!(
        execution_resource.status,
        CleanupResourceStatus::IsolatedUnknown
    );
    assert_eq!(
        execution_resource.reason.as_deref(),
        Some("execution_stop_unconfirmed")
    );

    let model_task = report
        .resources
        .iter()
        .find(|resource| resource.kind == CleanupResourceKind::ModelTask)
        .expect("model task observation missing");
    assert_eq!(model_task.status, CleanupResourceStatus::IsolatedUnknown);

    // 下一个 run 拿到的是新集合（空集合视为已解决），但上一个 run 的报告仍然拒绝释放；
    // 写所有权只能通过显式对账交还，不能靠「run 已经结束」。
    let next_run = RunId::new();
    assert_eq!(
        RunExecutionStopSet::new(next_run).resolution(),
        StopSetResolution::Confirmed
    );
    assert!(!report.safe_to_release_all());
    // 下一次 run 的停止集合是新的、空的；它不能通过复用旧报告来假装已释放。
    assert_eq!(RunExecutionStopSet::new(next_run).len(), 0);
}

struct PanickingModel;

#[async_trait]
impl ModelClient for PanickingModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        panic!("h33_model_task_aborted");
    }
}

/// 【拒绝】panic 中止的模型步不能留下「未登记但可释放」的后台工作。
///
/// Runner 捕获 unwind 后留下的 panic 证据必须逐项进入清理报告并保持 `IsolatedUnknown`
/// （`stop_confirmed == false`、`effect_known == false`），不能因为 run 结束就当作已释放。
#[tokio::test]
async fn panic_does_not_leave_untracked_background_work() {
    let root = temp_project();
    let harness = Arc::new(KianaHarness::new(Arc::new(PanickingModel)));
    let run_id = RunId::new();
    let events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "panic mid model step",
            root.to_string_lossy(),
            "read-only",
        ))
        .await
        .unwrap();
    assert!(
        events.iter().any(|event| matches!(
            event,
            RunnerEvent::Failed { error, .. } if error == HARNESS_PANIC_ERROR
        )),
        "panic must surface as result_unknown:harness_panic: {events:?}"
    );

    let panic = RunnerPort::take_panic_evidence(harness.as_ref(), run_id)
        .expect("panic evidence must be drained from the owning run");
    assert!(!panic.resources.is_empty());
    for resource in &panic.resources {
        assert!(
            !resource.stop_confirmed && !resource.effect_known,
            "panic cannot claim a confirmed effect: {resource:?}"
        );
    }

    let report = build_run_cleanup_report(&RunCleanupEvidence {
        run_id,
        cause: CleanupCause::Normal,
        in_flight: false,
        path_locks_held: false,
        stops: None,
        panic: Some(panic.clone()),
        log_failure: None,
    })
    .unwrap();
    // panic 证据优先于调用方给出的普通原因。
    assert_eq!(report.cause, CleanupCause::Panic);
    assert_eq!(report.resources.len(), panic.resources.len());
    assert_eq!(report.released_count, 0);
    assert_eq!(report.unknown_count as usize, panic.resources.len());
    assert_eq!(report.pending_count, 0);
    assert!(!report.safe_to_release_all());
    for resource in &report.resources {
        assert_eq!(resource.status, CleanupResourceStatus::IsolatedUnknown);
        assert!(!resource.stop_confirmed && !resource.effect_known);
    }

    // 证据只被认领一次；重复读取不能合成第二份 panic 记录。
    assert!(RunnerPort::take_panic_evidence(harness.as_ref(), run_id).is_none());
    // 进程内留存状态可以丢弃，但这只清 map，不宣称任何外部效果已停。
    harness.release_retained_run_state(run_id);
    harness.release_retained_run_state(run_id);
    assert!(RunnerPort::take_panic_evidence(harness.as_ref(), run_id).is_none());
}

/// 【成功】多个短 run 结束后句柄、锁与留存预算回到基线。
///
/// 每个 run 正常完成：报告全部 `Released`，`safe_to_release_all()` 为 true；
/// 留存上限在新 run 之间保持有界，超出时返回稳定的 `resource_retention_exceeded`。
#[tokio::test]
async fn many_short_runs_return_handles_locks_and_memory_to_baseline() {
    let project = temp_project();
    let harness = scripted_runner(json!([
        {"text": "run-0"},
        {"text": "run-1"},
        {"text": "run-2"},
        {"text": "run-3"},
        {"text": "run-4"},
        {"text": "run-5"},
        {"text": "run-6"},
        {"text": "run-7"},
    ]));
    let core = core_with_runner(harness.clone());
    let retention = ResourceRetention::new(8, 16, 4096).unwrap();
    let mut run_ids = Vec::new();
    for index in 0..8 {
        let mut context = trusted_context_in(&project, &format!("h33-short-{index}"));
        context.request_id = kiana_domain::RequestId::new();
        let response = core
            .start_run(context, format!("short run {index}"), None)
            .await
            .unwrap();
        assert_eq!(
            response.status,
            ExecutionStatus::Completed,
            "short run {index}: {response:?}"
        );
        let run_id = RunId::parse_str(response.output["run_id"].as_str().unwrap()).expect("run id");
        run_ids.push(run_id);

        // 结束时写入的清理报告：正常结束且没有在飞能力时，模型任务已 join，报告全 Released。
        let reports = core.run_cleanup_reports(run_id).await.unwrap();
        assert!(
            !reports.is_empty(),
            "run {index} must record a cleanup report"
        );
        for report in &reports {
            assert_eq!(report.cause, CleanupCause::Normal);
            assert_eq!(report.unknown_count, 0, "{report:?}");
            assert_eq!(report.pending_count, 0, "{report:?}");
            assert!(report.safe_to_release_all(), "{report:?}");
        }

        // 正常结束的短 run 已经把历史/分配摘除；释放一次是幂等的，第二次不再改变计数。
        let before = harness.retained_run_counts(run_id);
        harness.release_retained_run_state(run_id);
        let after = harness.retained_run_counts(run_id);
        assert_eq!(
            after,
            (0, 0, 0),
            "run {index} retained {before:?} then {after:?}"
        );
        harness.release_retained_run_state(run_id);
        assert_eq!(harness.retained_run_counts(run_id), after);
        assert!(RunnerPort::take_panic_evidence(harness.as_ref(), run_id).is_none());

        core.check_resource_retention(&retention, index + 1, 16, 4096)
            .unwrap();
    }
    assert_eq!(run_ids.len(), 8);
    assert_eq!(
        core.check_resource_retention(&retention, 8, 8, 1024),
        Ok(())
    );
    assert_eq!(
        core.check_resource_retention(&retention, 9, 16, 4096)
            .unwrap_err(),
        "resource_retention_exceeded"
    );
    assert_eq!(
        core.check_resource_retention(&retention, 8, 17, 4096)
            .unwrap_err(),
        "resource_retention_exceeded"
    );
    assert_eq!(
        core.check_resource_retention(&retention, 8, 16, 4097)
            .unwrap_err(),
        "resource_retention_exceeded"
    );
}
