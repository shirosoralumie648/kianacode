//! 运行时清理计划/报告的**生产者**与 owner 证据读取。
//!
//! [`kiana_domain::ResourceCleanupPlan`] 是只读 reducer：它不杀进程、不释放 OS 锁，只把
//! owner 提供的 stop/effect 证据归约为 `Released` / `Pending` / `IsolatedUnknown`。
//! 本模块负责用**真实存在的 owner 状态**构造该计划，并把报告写入事件账本：
//!
//! - run 级 owner：in-flight 登记、capability 执行停止集合、Builder 路径锁、turn 预算租约；
//! - panic owner：Runner 捕获 unwind 后留下的 panic 证据；
//! - shutdown owner：daemon 关闭时仍在执行的 run，以及观测队列 flush/close 失败。
//!
//! 【写入位置】报告写进 `resource_cleanup` 聚合（按 run id 分线），事件 kind 为
//! `resource.cleanup_report`。它**不是** `run` 聚合的一部分：清理报告不改变 run 终态，
//! 也不能让 `read_stream("run")` 的终态序列出现第二个 terminal。它也**绝不**写入
//! `resource_quarantine` 聚合——该聚合只接受 `resource.quarantined` / `resource.released`。
//!
//! 【为什么 payload 不用顶层 `run_id` 键】`aggregate_for_event` 会把任何带顶层 `run_id`
//! 的载荷路由到 run 聚合。这里使用 `run` 作为顶层键，report 内部的 `RunId` 仍按 domain
//! 序列化形式保存。

use super::*;
use kiana_domain::{
    CleanupCause, CleanupResource, CleanupResourceKind, CleanupResourceStatus, ExecutionStopState,
    ResourceCleanupPlan, ResourceCleanupReport, ResourceRetention, RunExecutionStopSet,
    StopSetResolution,
};
use kiana_runner_protocol::RunnerPanicRecord;

/// 清理报告事件的稳定 kind。
pub const RESOURCE_CLEANUP_REPORT_KIND: &str = "resource.cleanup_report";
/// 清理报告使用的聚合类型（按 run id 分线）。
pub const RESOURCE_CLEANUP_AGGREGATE: &str = "resource_cleanup";
/// 默认 retained run/frame/history 上限，供 daemon 组合根显式设置。
pub const DEFAULT_RESOURCE_RETENTION: (usize, usize, usize) = (256, 1_024, 8 * 1024 * 1024);

/// 一个 run 当前由本进程持有的 owner 资源快照。
///
/// 只描述**本进程能举证**的资源；跨进程目标、外部 provider 会话和尚未登记的副作用不在
/// 其中，因此不会被误记为 Released。
#[derive(Clone, Debug, Default)]
pub(crate) struct RunCleanupOwners {
    /// in-flight 表里仍有该 run 的取消句柄。
    pub in_flight: bool,
    /// Builder 会话仍持有路径锁。
    pub path_locks_held: bool,
    /// 本 turn 使用过项目/角色预算（存在预算账本条目）。
    pub budget_lease_held: bool,
    /// run 级停止集合快照。
    pub stops: Option<RunExecutionStopSet>,
    /// Runner 捕获 unwind 后留下的 panic 证据。
    pub panic: Option<RunnerPanicRecord>,
    /// 观测队列 flush/close 失败时的稳定原因码。
    pub log_failure: Option<String>,
}

impl RunCleanupOwners {
    /// 是否存在任何需要写清理报告的证据。
    pub(crate) fn has_evidence(&self) -> bool {
        self.in_flight
            || self.path_locks_held
            || self.budget_lease_held
            || self.panic.is_some()
            || self.log_failure.is_some()
            || self.stops.as_ref().is_some_and(|stops| !stops.is_empty())
    }
}

/// 依据 owner 证据选择清理 cause；日志失败与 panic 优先。
pub(crate) fn cleanup_cause_for_owners(owners: &RunCleanupOwners) -> CleanupCause {
    if owners.log_failure.is_some() {
        return CleanupCause::LogFailure;
    }
    if owners.panic.is_some() {
        return CleanupCause::Panic;
    }
    match owners.stops.as_ref().map(RunExecutionStopSet::resolution) {
        Some(StopSetResolution::Unconfirmed) | Some(StopSetResolution::Pending) => {
            CleanupCause::Cancel
        }
        _ if owners.in_flight => CleanupCause::Cancel,
        _ => CleanupCause::Normal,
    }
}

/// 把 owner 快照转换成 domain cleanup 报告。
///
/// 【判定规则】
/// - 能力执行项：只有停止集合里的 `Stopped` 项才有停止证据；`Stopping`/`StopUnconfirmed`
///   一律 IsolatedUnknown；取消前的 `Completed` 视为 effect 已知。
/// - in-flight 模型任务：`Normal` 结束才算已 join；取消或 panic 时无法举证，保持 Unknown。
/// - 路径锁：允许新写者进入，永远要求停止确认；这里只记录 owner，实际释放仍由
///   `release_builder_path_locks_if_confirmed` 的守卫决定。
/// - panic 记录：全部资源按记录里的 `stop_confirmed` / `effect_known` 观察（当前均为 false）。
/// - 日志失败：观测投递失败不影响已提交的 EventLog 事实，但没有停止证据的订阅保持 Unknown。
pub(crate) fn cleanup_report_for_run(
    run_id: RunId,
    cause: CleanupCause,
    owners: &RunCleanupOwners,
) -> Result<ResourceCleanupReport, String> {
    let mut resources: Vec<CleanupResource> = Vec::new();
    let mut observations: Vec<(String, bool, bool, String)> = Vec::new();
    {
        let mut add = |resource_id: String,
                       kind: CleanupResourceKind,
                       requires_stop_confirmation: bool,
                       observation: (bool, bool, String)| {
            resources.push(CleanupResource {
                resource_id: resource_id.clone(),
                owner_run_id: run_id,
                kind,
                requires_stop_confirmation,
                stop_confirmed: false,
                effect_known: false,
                status: CleanupResourceStatus::Pending,
                reason: None,
            });
            observations.push((resource_id, observation.0, observation.1, observation.2));
        };

        if owners.in_flight || owners.budget_lease_held {
            let (stop_confirmed, effect_known, reason) = match cause {
                CleanupCause::Normal => (true, true, "model_task_joined".to_owned()),
                CleanupCause::Cancel => (false, false, "model_task_stop_unconfirmed".to_owned()),
                CleanupCause::Panic => (false, false, "model_task_panic_unwound".to_owned()),
                CleanupCause::LogFailure => {
                    (false, false, "model_task_log_failure_unobserved".to_owned())
                }
                CleanupCause::Shutdown => {
                    (false, false, "model_task_shutdown_unconfirmed".to_owned())
                }
            };
            if owners.in_flight {
                add(
                    "model_task".to_owned(),
                    CleanupResourceKind::ModelTask,
                    true,
                    (stop_confirmed, effect_known, reason.clone()),
                );
            }
            if owners.budget_lease_held {
                add(
                    "budget_lease".to_owned(),
                    CleanupResourceKind::BudgetLease,
                    false,
                    (stop_confirmed, effect_known, reason),
                );
            }
        }

        if let Some(stops) = owners.stops.as_ref() {
            for execution_id in stops.execution_ids() {
                let entry = stops.entry(&execution_id);
                let (stop_confirmed, effect_known, reason) = match entry.map(|entry| entry.state) {
                    Some(ExecutionStopState::Stopped) => {
                        (true, true, "execution_stop_confirmed".to_owned())
                    }
                    Some(ExecutionStopState::Completed) => {
                        (true, true, "execution_completed_before_cancel".to_owned())
                    }
                    Some(ExecutionStopState::StopUnconfirmed) => {
                        (false, false, "execution_stop_unconfirmed".to_owned())
                    }
                    Some(ExecutionStopState::Stopping) => {
                        (false, false, "execution_stopping".to_owned())
                    }
                    Some(ExecutionStopState::Inflight) | None => {
                        (false, false, "execution_inflight".to_owned())
                    }
                };
                add(
                    format!("execution:{execution_id}"),
                    CleanupResourceKind::ToolFuture,
                    true,
                    (stop_confirmed, effect_known, reason),
                );
            }
        }

        if owners.path_locks_held {
            // 路径锁允许新写者进入，因此永远要求停止确认；只有已确认停止（且不是 panic
            // 中止）的 run 才能把写所有权交还给下一个写者。
            let stops_confirmed = owners
                .stops
                .as_ref()
                .is_some_and(|stops| stops.resolution() == StopSetResolution::Confirmed);
            let (stop_confirmed, effect_known, reason) =
                if stops_confirmed && cause != CleanupCause::Panic {
                    (
                        true,
                        true,
                        "path_lock_released_after_confirmed_stop".to_owned(),
                    )
                } else {
                    (
                        false,
                        false,
                        "path_lock_release_requires_confirmed_stop".to_owned(),
                    )
                };
            add(
                "path_lock".to_owned(),
                CleanupResourceKind::PathLock,
                true,
                (stop_confirmed, effect_known, reason),
            );
        }

        if let Some(panic) = owners.panic.as_ref() {
            for resource in &panic.resources {
                let kind = match resource.kind.as_str() {
                    "tool_future" => CleanupResourceKind::ToolFuture,
                    "mcp_connection" => CleanupResourceKind::McpConnection,
                    "temporary_artifact" => CleanupResourceKind::TemporaryArtifact,
                    "job_handle" => CleanupResourceKind::JobHandle,
                    "path_lock" => CleanupResourceKind::PathLock,
                    "budget_lease" => CleanupResourceKind::BudgetLease,
                    "subscription" => CleanupResourceKind::Subscription,
                    _ => CleanupResourceKind::ModelTask,
                };
                add(
                    resource.resource_id.clone(),
                    kind,
                    resource.requires_stop_confirmation,
                    (
                        resource.stop_confirmed,
                        resource.effect_known,
                        resource.reason.clone(),
                    ),
                );
            }
        }

        if let Some(reason) = owners.log_failure.as_ref() {
            // 日志投递失败不能证明外部资源已停；保留订阅为 Unknown 以便对账。
            add(
                "observability_subscription".to_owned(),
                CleanupResourceKind::Subscription,
                false,
                (false, false, reason.clone()),
            );
        }
    }

    let plan = ResourceCleanupPlan::new(run_id, cause, resources)?;
    // 状态只能来自上面的证据：计划在观察前全部保持 Pending。
    plan.observe_all(&observations)
}

/// 构造清理报告的公开输入：一次 run 结束时本进程实际能举证的 owner 状态。
///
/// 这是纯数据：调用方不能通过它声称外部效果已停，只能把已有的 stop/effect 证据交进来。
#[derive(Clone, Debug)]
pub struct RunCleanupEvidence<'a> {
    /// 被清理的 run。
    pub run_id: RunId,
    /// 结束原因；panic / 日志失败会覆盖调用方给出的普通原因。
    pub cause: CleanupCause,
    /// 本进程是否仍持有该 run 的模型任务登记。
    pub in_flight: bool,
    /// Builder 会话是否仍持有路径锁。
    pub path_locks_held: bool,
    /// run 级执行停止集合。
    pub stops: Option<&'a RunExecutionStopSet>,
    /// Runner 捕获 unwind 后留下的 panic 证据。
    pub panic: Option<RunnerPanicRecord>,
    /// 观测队列 flush/close 失败原因码。
    pub log_failure: Option<String>,
}

/// 把公开证据转换为 domain 清理报告。
///
/// 【规则】与内部 `cleanup_report_for_run` 相同：没有停止确认的资源保持
/// `IsolatedUnknown`，绝不因为「run 已经结束」就当作已释放。
pub fn build_run_cleanup_report(
    evidence: &RunCleanupEvidence<'_>,
) -> Result<ResourceCleanupReport, String> {
    let owners = RunCleanupOwners {
        in_flight: evidence.in_flight,
        path_locks_held: evidence.path_locks_held,
        budget_lease_held: false,
        stops: evidence.stops.cloned(),
        panic: evidence.panic.clone(),
        log_failure: evidence.log_failure.clone(),
    };
    let cause = if owners.panic.is_some() || owners.log_failure.is_some() {
        cleanup_cause_for_owners(&owners)
    } else {
        evidence.cause
    };
    cleanup_report_for_run(evidence.run_id, cause, &owners)
}

impl ControlPlane {
    /// 读取一个 run 的清理 owner 证据。
    ///
    /// - `in_flight` 由调用方传入：只有实际还持有登记的调用者才能声称模型任务在飞；
    /// - 停止集合来自本进程的 RunStopChannel；
    /// - 路径锁按 Builder 会话归属判断。
    pub(crate) fn cleanup_owners(
        &self,
        run_id: RunId,
        in_flight: bool,
        session_id: Option<&str>,
    ) -> RunCleanupOwners {
        let stops = self.execution_stop_snapshot(run_id);
        let path_locks_held = session_id.is_some_and(|session_id| self.path_lock_owner(session_id));
        RunCleanupOwners {
            in_flight,
            path_locks_held,
            // Core 没有为本 run 单独持有预算租约；预算记账在 cell registry，不能在这里
            // 虚构一个租约来让报告显得更完整。
            budget_lease_held: false,
            stops: Some(stops),
            panic: None,
            log_failure: None,
        }
    }

    /// 测试/诊断用的路径锁快照（只读）。
    #[cfg(test)]
    pub(crate) fn path_lock_snapshot(&self) -> Vec<(String, String)> {
        let mut snapshot = self
            .path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(path, owner)| (path.clone(), owner.clone()))
            .collect::<Vec<_>>();
        snapshot.sort();
        snapshot
    }

    /// 该 Builder 会话是否仍持有路径锁。
    pub(crate) fn path_lock_owner(&self, session_id: &str) -> bool {
        self.path_locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|owner| owner == session_id)
    }

    /// 写一份清理报告到 `resource_cleanup` 聚合（幂等、CAS）。
    ///
    /// 【输出】成功返回 `Some(report)`；同一 run 的同一摘要已落账时返回 `None`。
    /// 【副作用】只追加 `resource.cleanup_report` 事件；不写 run 聚合、不写
    /// `resource_quarantine`，也不改变任何 run 状态或释放任何锁。
    pub(crate) async fn record_cleanup_report(
        &self,
        run_id: RunId,
        report: &ResourceCleanupReport,
    ) -> Result<Option<ResourceCleanupReport>, CoreError> {
        // 清理报告使用 run 派生的 request id：它不能污染原命令的 request stream
        // （read_request 会被既有夹具逐条断言），同时保持同一 run 的幂等键稳定。
        let request_id = kiana_domain::derived_request_id("resource.cleanup", &run_id.to_string());
        report
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        let aggregate_id = run_id.to_string();
        let key = kiana_domain::json_digest(&serde_json::json!({"run": aggregate_id}));
        let data = serde_json::json!({
            "schema": kiana_domain::RESOURCE_CLEANUP_REPORT_SCHEMA,
            "run": aggregate_id,
            "report_digest": report.report_digest,
            "cause": report.cause,
            "released_count": report.released_count,
            "unknown_count": report.unknown_count,
            "pending_count": report.pending_count,
            "safe_to_release_all": report.safe_to_release_all(),
            "report": report,
        });
        let expected = self
            .events
            .read_stream(RESOURCE_CLEANUP_AGGREGATE, &key)
            .await?;
        let current_version = expected
            .iter()
            .map(|event| event.stream_version.unwrap_or(event.sequence))
            .max()
            .unwrap_or(0);
        // 幂等键必须包含报告摘要：同一个 run 可以先后拥有 `Shutdown` 与 `LogFailure`
        // 两条独立事实（观测投递失败不能吞掉停止证据）。只按 run 固定键会让第二条报告
        // 命中 `event_idempotency_key_payload_mismatch` 而被静默丢弃。
        let idempotency_key = format!(
            "{request_id}:{RESOURCE_CLEANUP_AGGREGATE}:{key}:{RESOURCE_CLEANUP_REPORT_KIND}:{}",
            report.report_digest
        );
        let event = RuntimeEvent::new(
            request_id,
            current_version.saturating_add(1),
            RESOURCE_CLEANUP_REPORT_KIND,
            data,
        )?
        .with_stream_metadata(
            RESOURCE_CLEANUP_AGGREGATE,
            key,
            current_version.saturating_add(1),
        )
        .with_idempotency_key(idempotency_key);
        match self
            .events
            .append_idempotent_expected(event, Some(current_version))
            .await
        {
            Ok(result) => Ok((!result.replayed).then(|| report.clone())),
            Err(error) => Err(error.into()),
        }
    }

    /// 为一个 run 构造并写入清理报告。
    ///
    /// 【失败语义】报告构造失败（证据不自洽）返回错误；账本写入失败返回端口错误。
    /// 调用方必须把失败视为「清理未确认」，不能降级成成功。
    pub(crate) async fn record_run_cleanup(
        &self,
        run_id: RunId,
        cause: CleanupCause,
        owners: &RunCleanupOwners,
    ) -> Result<Option<ResourceCleanupReport>, CoreError> {
        if !owners.has_evidence() {
            return Ok(None);
        }
        let report = cleanup_report_for_run(run_id, cause, owners)
            .map_err(|error| PortError::Failed(format!("cleanup_plan_invalid:{error}")))?;
        self.record_cleanup_report(run_id, &report).await
    }

    /// 读取某个 run 已落账的清理报告。
    ///
    /// 【输出】没有报告时返回空列表，而不是合成一份「全部已释放」。
    pub async fn run_cleanup_reports(
        &self,
        run_id: RunId,
    ) -> Result<Vec<ResourceCleanupReport>, CoreError> {
        let key = kiana_domain::json_digest(&serde_json::json!({"run": run_id.to_string()}));
        let events = self
            .events
            .read_stream(RESOURCE_CLEANUP_AGGREGATE, &key)
            .await?;
        let mut reports = Vec::new();
        for event in events {
            if event.kind != RESOURCE_CLEANUP_REPORT_KIND {
                continue;
            }
            let report: ResourceCleanupReport =
                serde_json::from_value(event.data["report"].clone())
                    .map_err(|_| PortError::Failed("resource_cleanup_report_invalid".to_owned()))?;
            report
                .validate()
                .map_err(|error| PortError::Failed(error.to_owned()))?;
            reports.push(report);
        }
        Ok(reports)
    }

    /// 释放一个 run 的 owner 资源，并记录清理报告。
    ///
    /// 【与 reducer 的边界】本方法**不**杀进程、**不**释放 OS 文件锁、**不**把 Host drop
    /// 当作停止确认。它只：
    /// 1. 读取 owner 证据并生成 `ResourceCleanupReport`；
    /// 2. 只有在报告确认「全部可释放」时才调用受守卫的路径锁释放；
    /// 3. 把报告写入 `resource_cleanup` 聚合。
    ///
    /// 【输出】`Ok(Some(report))` 表示报告已落账；`Ok(None)` 表示该 run 没有可报告证据；
    /// 错误表示证据或账本写入失败，调用方必须保留 Unknown 语义。
    pub async fn release_run_resources(
        &self,
        run_id: RunId,
        cause: CleanupCause,
        session_id: Option<&str>,
        in_flight: bool,
        project_root: Option<&str>,
        panic: Option<RunnerPanicRecord>,
        log_failure: Option<String>,
    ) -> Result<Option<ResourceCleanupReport>, CoreError> {
        let mut owners = self.cleanup_owners(run_id, in_flight, session_id);
        owners.panic = panic;
        owners.log_failure = log_failure;
        let cause = if owners.panic.is_some() || owners.log_failure.is_some() {
            cleanup_cause_for_owners(&owners)
        } else {
            cause
        };
        let report = self.record_run_cleanup(run_id, cause, &owners).await?;
        // 只有报告确认「全部可释放」时才交还路径锁；Unknown 必须继续保留 fencing，
        // 让下一个写者无法绕过对账。
        if report
            .as_ref()
            .is_some_and(|report| report.safe_to_release_all())
        {
            if let (Some(session_id), Some(project_root)) = (session_id, project_root) {
                self.release_builder_path_locks_if_confirmed(run_id, project_root, session_id);
            }
        }
        Ok(report)
    }

    /// run 结束时的清理入口：读取 owner 证据、写报告，并在确认安全时交还路径锁。
    ///
    /// 【失败语义】这是证据写入，不是终态写入：任何错误都被吞掉并保留 Unknown 语义，
    /// 绝不让清理失败把已提交的 terminal 改写成成功或失败。
    /// 【边界】不杀进程、不释放 OS 锁；路径锁只在报告 `safe_to_release_all()` 时释放。
    pub(crate) async fn record_end_of_run_cleanup(
        &self,
        context: &RequestContext,
        run_id: RunId,
        base_cause: CleanupCause,
    ) {
        let panic = self.runner.take_panic_evidence(run_id);
        let _ = self
            .release_run_resources(
                run_id,
                base_cause,
                Some(context.session_id.as_str()),
                true,
                Some(context.project_root.as_str()),
                panic,
                None,
            )
            .await;
    }

    /// 关闭时取消仍在执行的 run，并为每个 run 写 `Shutdown` 清理报告。
    ///
    /// 【输出】返回被取消/记录的 run 数量与 run id 列表，供 daemon 在观测失败时补充
    /// `LogFailure` 证据。
    /// 【边界】只发送取消命令与写证据；不等待外部副作用、不杀进程、不把关闭当作停止确认。
    pub async fn shutdown_in_flight_runs(&self, request_id: RequestId) -> Vec<RunId> {
        let run_ids = self
            .cancellations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for run_id in &run_ids {
            // 取消消息尽力送达；Runner 的停止证明仍由后续对账决定。
            let _ = self
                .runner
                .send(RunnerCommand::Cancel {
                    run_id: *run_id,
                    reason: "shutdown".to_owned(),
                })
                .await;
        }
        for run_id in &run_ids {
            let panic = self.runner.take_panic_evidence(*run_id);
            let _ = self
                .release_run_resources(
                    *run_id,
                    CleanupCause::Shutdown,
                    None,
                    true,
                    None,
                    panic,
                    None,
                )
                .await;
        }
        let _ = request_id;
        run_ids
    }

    /// 为关闭期间观测投递失败的 run 追加 `LogFailure` 证据。
    ///
    /// 【为什么单独一条报告】`Shutdown` 报告已经记录停止未知；观测失败是另一个事实，
    /// 需要独立原因码，不能被前一条报告吞掉。
    pub async fn record_shutdown_log_failures(&self, run_ids: &[RunId], reason: String) {
        for run_id in run_ids {
            let _ = self
                .release_run_resources(
                    *run_id,
                    CleanupCause::LogFailure,
                    None,
                    true,
                    None,
                    self.runner.take_panic_evidence(*run_id),
                    Some(reason.clone()),
                )
                .await;
        }
    }

    /// 按 owner 证据检查 retained run/frame/history 是否在上限内。
    ///
    /// 返回 `Ok(())` 或稳定的 `resource_retention_exceeded`；它不裁掉正在执行的 run。
    pub fn check_resource_retention(
        &self,
        retention: &ResourceRetention,
        retained_runs: usize,
        retained_frames: usize,
        history_bytes: usize,
    ) -> Result<(), String> {
        retention.within(retained_runs, retained_frames, history_bytes)
    }
}
