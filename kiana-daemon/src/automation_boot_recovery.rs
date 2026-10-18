//! AUT-21 启动恢复：DaemonHost 的观测、投影重建与显式 resume fence。
//!
//! # 在系统中的位置
//!
//! ```text
//! DaemonHost::observe_automation_boot_recovery(project_root)
//!     -> Core::cursor_snapshot / persisted_events / event_store_health / observed_authority_epoch
//!     -> ProjectionDriver::rebuild + apply（在内存里重放已提交事件）
//!     -> Domain AutomationBootRecoveryFact::validate + sealed
//!     -> 记录 fenced 状态（dispatch 必须等显式 resume）
//! DaemonHost::resume_automation_after_recovery(context, project_root)
//!     -> ControlPlane::handle_command("automation.boot_recovery.resume.v1")
//!     -> Core 拒绝自证 Ready，重新推导 Ready 并复核实时 authority epoch
//!     -> 成功后只解除 daemon 的 dispatch fence
//! ```
//!
//! # 关键纪律
//!
//! * 重启默认 `Rebuilding`（干净）或 `ReconcileRequired`（有 Unknown / stale lease），
//!   **从不默认 `Ready`**；只有显式 operator resume 才产出 Ready。
//! * resume 走 `ControlPlane::handle_command`，没有第二条执行循环，也不写恢复事件。
//! * 观测与 resume 都不会启动 `automation_dispatch_service` / `workflow_queue_service`。
//! * `ProjectionDriver::rebuild` 只重建内存读模型；本模块不持久化 checkpoint。
//!
//! 当前限制：pending Unknown 由「每个 run 的最后一条事件是否 `*.result_unknown`」推断，
//! 不是对 provider 真实结果的核对；索引新鲜度还无法观测，因此 `stale_index` 始终为 false。
//! 这些限制写进 baseline，不会被当成 durable 恢复证明。

use super::*;
use kiana_core::{
    derive_boot_recovery_resume, CoreError, ProjectionDriver,
    AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND,
};
use kiana_domain::{
    AutomationBootJournal, AutomationBootLease, AutomationBootRecoveryFact,
    AutomationBootRecoveryState, AutomationReconcileCase, JOURNAL_HEADER_SCHEMA,
    JOURNAL_WRITER_VERSION,
};
use serde_json::json;

/// 启动恢复投影的稳定 projector 名称；重建时用作 checkpoint 身份。
pub const AUTOMATION_BOOT_RECOVERY_PROJECTOR: &str = "kiana.automation.boot_recovery.v1";

/// 一个 host 生命周期内的恢复 gate。
///
/// `Unobserved` 保留 AUT-21 之前的兼容路径；`Fenced` 表示已完成启动观测但尚未显式 resume；
/// `Admitted` 只可能在 Core 返回一份推导出的 Ready fact 之后出现。
#[derive(Clone, Debug, Default)]
pub(crate) struct BootRecoveryGate {
    fact: Option<AutomationBootRecoveryFact>,
    admitted: bool,
}

/// 只读的恢复 gate 快照，用于展示和测试。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationBootRecoveryGate {
    /// 最近一次启动观测事实；`None` 表示该 host 未走过启动观测。
    pub fact: Option<AutomationBootRecoveryFact>,
    /// 是否已有显式 operator resume 解除 dispatch fence。
    pub dispatch_admitted: bool,
}

impl AutomationBootRecoveryGate {
    /// dispatch 是否仍被启动恢复 fence 阻挡。
    pub fn dispatch_fenced(&self) -> bool {
        self.fact.is_some() && !self.dispatch_admitted
    }
}

/// 把 EventStore 健康快照映射为启动恢复的 journal 输入。
///
/// `healthy == false` 保留 `corrupt` 判定；writer 版本漂移映射成另一个 header schema，
/// 从而以 `automation_boot_recovery_journal_schema_mismatch` 稳定拒绝。映射本身不做 I/O。
pub fn boot_journal_from_health(health: &kiana_domain::EventStoreHealth) -> AutomationBootJournal {
    if !health.healthy {
        return AutomationBootJournal {
            schema: JOURNAL_HEADER_SCHEMA.to_owned(),
            healthy: false,
        };
    }
    if health.writer_version != JOURNAL_WRITER_VERSION {
        return AutomationBootJournal {
            schema: format!("kiana.journal-header.v{}", health.writer_version),
            healthy: true,
        };
    }
    AutomationBootJournal::healthy()
}

/// 每个 run 的最后一条事件若是 `*.result_unknown`，该 run 仍有一个待对账 Unknown。
///
/// 这是**观测启发式**：它只读已提交事件，不查询 provider，也不判断结果是否其实成功。
fn pending_unknown_runs(events: &[RuntimeEvent]) -> Vec<String> {
    let mut last_kind: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
    for event in events {
        let key = event
            .data
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| event.request_id.to_string());
        last_kind.insert(key, event.kind.as_str());
    }
    last_kind
        .into_iter()
        .filter(|(_, kind)| kind.ends_with(".result_unknown"))
        .map(|(run, _)| run)
        .collect()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(1)
        .max(1)
}

/// 观测实例租约。
///
/// 只在 `<project>/.kiana/instances` 已存在时读盘，避免在虚拟项目根上制造 I/O 失败；
/// 目录存在但锁不可读/不完整视为 crashed instance 的 stale lease，而不是「无租约」。
fn observe_lease(project_root: &str) -> AutomationBootLease {
    let directory = Path::new(project_root).join(".kiana").join("instances");
    if !directory.exists() {
        return AutomationBootLease::absent();
    }
    let observed_at = now_unix_ms();
    match instance::discover(project_root) {
        Ok(record) => AutomationBootLease {
            lease_id: Some(record.instance_id),
            // 有效实例记录意味着另一个进程仍持有写权；保留为「active」而不是伪造到期时间。
            expires_at_unix_ms: observed_at.saturating_add(1),
            observed_at_unix_ms: observed_at,
        },
        Err(PortError::Unavailable(reason)) if reason == "ui_instance_not_running" => {
            AutomationBootLease::absent()
        }
        Err(_) => AutomationBootLease {
            lease_id: Some("instance-lock-unreadable".to_owned()),
            expires_at_unix_ms: observed_at,
            observed_at_unix_ms: observed_at,
        },
    }
}

impl DaemonHost {
    /// 只读查看当前 host 的启动恢复 gate。
    pub fn automation_boot_recovery_gate(&self) -> AutomationBootRecoveryGate {
        let gate = self
            .boot_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        AutomationBootRecoveryGate {
            fact: gate.fact.clone(),
            dispatch_admitted: gate.admitted,
        }
    }

    /// dispatch 是否仍被启动恢复 fence 阻挡。
    ///
    /// 未观测过的 host 返回 false，保留 AUT-21 之前的兼容路径；一旦观测过且未显式 resume，
    /// 所有 dispatch 启动都必须先通过 [`Self::resume_automation_after_recovery`]。
    pub fn automation_dispatch_fenced(&self) -> bool {
        self.automation_boot_recovery_gate().dispatch_fenced()
    }

    /// 观测一次启动恢复状态，并把 host 置于 dispatch fenced 状态。
    ///
    /// 【核心流程】
    /// 1. `ControlPlane::cursor_snapshot` 取已提交游标；不支持时直接进入 `Blocked`。
    /// 2. `ControlPlane::persisted_events` 读取全部已提交事件；不支持时进入 `Blocked`。
    /// 3. `ProjectionDriver::new(..).rebuild()` 后 `apply()` 重放事件，投影游标取 checkpoint。
    /// 4. `event_store_health` 映射成 typed journal；authority epoch 由 Core 从账本读出。
    /// 5. 有 pending Unknown / 未结租约 → `ReconcileRequired`；否则默认 `Rebuilding`。
    ///
    /// 【输出】通过 Domain `validate()` 的 fact；观测本身**不**产出 Ready。
    /// 【副作用】只读 EventStore 与（可选）实例锁文件；不写事件、不启动 worker。
    pub async fn observe_automation_boot_recovery(
        &self,
        project_root: &str,
    ) -> Result<AutomationBootRecoveryFact, PortError> {
        // `event_store_health_unsupported` 不是「损坏」：内存 adapter 不实现该查询。此时 journal
        // 视为形状可用但未观测，并把 limitation 记录进 fact 之外的 baseline；真正的损坏仍由
        // adapter 报告的 `healthy == false` 表达。
        let journal = match self.core.event_store_health().await {
            Ok(health) => boot_journal_from_health(&health),
            Err(CoreError::Port(PortError::Unavailable(reason)))
                if reason == "event_store_health_unsupported" =>
            {
                AutomationBootJournal::healthy()
            }
            Err(error) => return Err(PortError::Failed(error.to_string())),
        };
        let authority_epoch = self
            .core
            .observed_authority_epoch(project_root)
            .await
            .map_err(|error| PortError::Failed(error.to_string()))?;
        let lease = observe_lease(project_root);

        let snapshot = self
            .core
            .cursor_snapshot()
            .await
            .map_err(|error| PortError::Failed(error.to_string()))?;
        let source_cursor = snapshot
            .as_ref()
            .map(|snapshot| snapshot.last_committed_cursor)
            .unwrap_or_default();
        let events = self.persisted_events().await?;

        // 唯一的重建路径：显式 rebuild 后从零重放；失败只把状态降级成 Blocked，不自动重试。
        let mut driver =
            ProjectionDriver::new(AUTOMATION_BOOT_RECOVERY_PROJECTOR).map_err(PortError::Failed)?;
        driver.rebuild();
        let mut rebuilt = true;
        let mut observation_failure: Option<String> = None;
        if let (Some(_), Some(events)) = (snapshot.as_ref(), events.as_ref()) {
            match driver.apply(
                json!({"schema": AUTOMATION_BOOT_RECOVERY_PROJECTOR, "events": 0}),
                events,
                source_cursor,
                |mut state, _event| {
                    let count = state["events"].as_u64().unwrap_or_default() + 1;
                    state["events"] = json!(count);
                    Ok(state)
                },
            ) {
                Ok(()) => {}
                Err(reason) => {
                    rebuilt = false;
                    observation_failure = Some(reason);
                }
            }
        }
        let projection_cursor = driver
            .checkpoint()
            .map(|checkpoint| checkpoint.source_cursor)
            .unwrap_or_default();
        if !rebuilt && observation_failure.is_none() {
            observation_failure = Some("automation_boot_recovery_eventlog_unreadable".to_owned());
        }

        let mut pending_runs = events
            .as_deref()
            .map(pending_unknown_runs)
            .unwrap_or_default();
        let pending_unknown_count = u32::try_from(pending_runs.len()).unwrap_or(u32::MAX);
        // 每个 run 一个 typed case；总数必须与 pending_unknown_count 一致，由 Domain 校验。
        let reconcile_cases = pending_runs
            .drain(..)
            .map(|_| AutomationReconcileCase {
                source_cursor,
                projection_generation: 1,
                authority_epoch,
                pending_unknown_count: 1,
            })
            .collect::<Vec<_>>();

        let state = if observation_failure.is_some()
            || !journal.healthy
            || journal.schema != JOURNAL_HEADER_SCHEMA
        {
            // 启动被证据挡住：Blocked 保留证据，但永远不会被当作可 dispatch。
            AutomationBootRecoveryState::Blocked
        } else if pending_unknown_count > 0 || lease.lease_id.is_some() {
            AutomationBootRecoveryState::ReconcileRequired
        } else {
            // 干净重启的默认值：投影刚重建，但 dispatch 仍要等显式 operator resume。
            AutomationBootRecoveryState::Rebuilding
        };
        let fact = AutomationBootRecoveryFact {
            schema: kiana_domain::AUTOMATION_BOOT_RECOVERY_SCHEMA.to_owned(),
            source_cursor,
            projection_cursor,
            projection_generation: 1,
            authority_epoch,
            observed_authority_epoch: authority_epoch,
            pending_unknown_count,
            // 索引新鲜度当前不可观测；重建成功视为不 stale，并作为 limitation 记录。
            stale_index: false,
            lease,
            journal,
            state,
            reconcile_cases,
            recovery_digest: String::new(),
        }
        .sealed();
        // 观测结果必须自身合法；不合法说明 daemon 观测有 bug，fail closed 而不是放行。
        fact.validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;

        let mut gate = self
            .boot_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        gate.fact = Some(fact.clone());
        gate.admitted = false;
        Ok(fact)
    }

    /// 显式 operator resume：经 ControlPlane 推导 Ready 后才解除 dispatch fence。
    ///
    /// 【输入】`context` 必须是无 cell、具名 actor 的 operator 上下文；`project_root` 必须与
    /// 观测事实所属项目一致（由 Core 复核）。
    ///
    /// 【失败】未观测、Core 拒绝（operator 缺失、项目不匹配、fact 非法、状态不是 Rebuilding、
    /// Ready 屏障未清、实时 authority epoch 已推进）都原样返回 `Blocked` 响应或 `PortError`；
    /// 任何失败路径都不会解除 fence。
    ///
    /// 【副作用】只调用 `ControlPlane::handle_command`；不写事件、不启动服务。
    pub async fn resume_automation_after_recovery(
        &self,
        context: RequestContext,
        project_root: &str,
    ) -> Result<kiana_protocol::ResponseEnvelope, PortError> {
        let fact = {
            let gate = self
                .boot_recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gate.fact.clone()
        }
        .ok_or_else(|| {
            PortError::Conflict("automation_boot_recovery_observation_required".to_owned())
        })?;
        let request_id = context.request_id.clone();
        let response = self
            .core
            .handle_command(
                context,
                CommandIntent::new(
                    AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND,
                    json!({"project_root": project_root, "fact": fact}),
                ),
            )
            .await
            .map_err(|error| PortError::Failed(error.to_string()))?;
        if response.status != kiana_domain::ExecutionStatus::Completed {
            return Ok(kiana_protocol::ResponseEnvelope::rejected(
                request_id,
                response
                    .error
                    .unwrap_or_else(|| "automation_boot_recovery_resume_denied".to_owned()),
            ));
        }
        let ready: AutomationBootRecoveryFact =
            serde_json::from_value(response.output["fact"].clone()).map_err(|_| {
                PortError::Failed("automation_boot_recovery_fact_invalid".to_owned())
            })?;
        ready
            .validate()
            .map_err(|reason| PortError::Failed(reason.to_owned()))?;
        if ready.state != AutomationBootRecoveryState::Ready {
            return Err(PortError::Failed(
                "automation_boot_recovery_not_ready".to_owned(),
            ));
        }
        // 只有 Core 推导出的 Ready fact 才能解除 fence；这里不重新推导，避免第二份规则。
        let mut gate = self
            .boot_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        gate.fact = Some(ready);
        gate.admitted = true;
        Ok(kiana_protocol::ResponseEnvelope {
            schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
            request_id,
            status: kiana_domain::ExecutionStatus::Completed,
            output: response.output,
            error: None,
        })
    }

    /// 服务启动前的 fence 检查：完成观测但未显式 resume 时拒绝启动 effect 服务。
    ///
    /// 这是 [`Self::automation_dispatch_fenced`] 的产品调用点：`start_automation_dispatch_service`
    /// 与 `start_workflow_queue_service` 都必须先通过它，因此重启后不可能绕过 operator reconcile
    /// 直接让 worker 跑起来。未观测过的 host 保持 AUT-21 之前的兼容行为。
    pub(crate) async fn ensure_boot_recovery_admits_dispatch(
        &self,
        service: &str,
    ) -> Result<(), PortError> {
        if self.automation_dispatch_fenced() {
            return Err(PortError::Conflict(format!(
                "automation_boot_recovery_resume_required:{service}"
            )));
        }
        Ok(())
    }

    /// 计算 resume 的预期结论但不修改 fence；供调用方在提交前预览拒绝原因。
    ///
    /// 该函数复用 Core 的同一套推导规则，不读存储、不写事件。
    pub fn preview_automation_boot_recovery_resume(
        &self,
    ) -> Option<Result<AutomationBootRecoveryFact, &'static str>> {
        self.automation_boot_recovery_gate()
            .fact
            .map(|fact| derive_boot_recovery_resume(&fact))
    }
}
