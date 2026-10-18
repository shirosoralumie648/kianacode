//! AUT-21 启动恢复的 Core 门面：静态校验、显式 resume 推导与唯一命令入口。
//!
//! 调用方提供 `AutomationBootRecoveryFact` → 本文件复核 Domain 规则 → 返回成功、稳定错误码
//! 或一份**由 Core 重新推导**的 Ready fact。
//!
//! # 边界
//!
//! 本文件不读取 EventStore 来重建投影、不写恢复事件、不启动 worker；它没有 a background task spawn，
//! 不持有 Broker，也不是第二条执行循环。启动观测与 `ProjectionDriver::rebuild` 由
//! `kiana-daemon::DaemonHost::boot_recovery` 完成；命令入口在 `commands.rs` 的
//! `automation.boot_recovery.resume.v1` 分支，仍然经过同一个 `ControlPlane::handle_command`。
//!
//! # 为什么 resume 要重新推导 Ready
//!
//! 调用方提交的是**观测事实**（重启默认 `Rebuilding` 或 `ReconcileRequired`）。如果 Core 直接
//! 相信调用方声明的 `Ready`，那么一次「把 state 改成 ready」的 JSON 编辑就能解除 dispatch
//! fence。因此这里拒绝 `Ready` 输入，只在所有屏障都通过时自己产出 Ready；同时用**实时**
//! authority ledger epoch 复核调用方观测值，旧 epoch 快照不能 resume。

use super::*;
use kiana_domain::{
    validate_automation_boot_recovery, AutomationBootRecoveryFact, AutomationBootRecoveryState,
    AUTOMATION_BOOT_RECOVERY_SCHEMA,
};

/// 启动恢复显式 resume 的命令名；由 `commands.rs` 路由，不经过 broker。
pub const AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND: &str = "automation.boot_recovery.resume.v1";

/// resume 成功响应携带的固定 schema；客户端据它区分「恢复结论」和普通命令输出。
pub const AUTOMATION_BOOT_RECOVERY_RESUME_SCHEMA: &str = AUTOMATION_BOOT_RECOVERY_SCHEMA;

/// 用领域层的静态规则校验调用方提供的启动恢复事实。
///
/// 【调用者】`DaemonHost::boot_recovery` 在观测后自检，以及测试。
///
/// 【输出/失败】成功返回 `Ok(())`，失败保留 Domain 的稳定拒绝码。通过仅表示字段规则成立，
/// 不能证明恢复进度来自真实事件源或投影。
pub fn validate_boot_recovery(fact: &AutomationBootRecoveryFact) -> Result<(), &'static str> {
    validate_automation_boot_recovery(fact)
}

/// 把一份**重启观测事实**推导为可 resume 的 `Ready` fact。
///
/// 【输入】必须是 `Rebuilding` 状态且自身通过 `validate()` 的 fact。`Ready` 输入被拒绝
/// （不能自证已恢复），`Blocked` 被拒绝（恢复被阻断时先修证据，而不是 resume），
/// `ReconcileRequired` 被拒绝（先用既有对账路径结清未结项，再重新观测）。
///
/// 【输出】一份 `state = Ready`、摘要重新计算过的副本。它只有在投影追平、零 Unknown、
/// 零未结 case、索引不 stale、无租约、journal healthy、authority epoch 与观测值相等时才产出；
/// 任一屏障失败都返回 Domain 的对应稳定拒绝码（例如
/// `automation_boot_recovery_projection_lagging` / `automation_boot_recovery_stale_lease`）。
///
/// 【副作用】无。不写事件、不改存储、不打开 projection。
pub fn derive_boot_recovery_resume(
    fact: &AutomationBootRecoveryFact,
) -> Result<AutomationBootRecoveryFact, &'static str> {
    fact.validate()?;
    match fact.state {
        AutomationBootRecoveryState::Ready => {
            return Err("automation_boot_recovery_resume_already_ready")
        }
        AutomationBootRecoveryState::Blocked => return Err("automation_boot_recovery_blocked"),
        AutomationBootRecoveryState::ReconcileRequired => {
            // 未结项必须显式结清；resume 不是 reconcile。
            return Err("automation_boot_recovery_reconcile_required");
        }
        AutomationBootRecoveryState::Rebuilding => {}
    }
    let mut ready = fact.clone();
    ready.state = AutomationBootRecoveryState::Ready;
    ready.recovery_digest = ready.digest();
    // 复用同一份 Domain 规则：这里返回的拒绝码就是 Ready 屏障的稳定码。
    ready.validate()?;
    Ok(ready)
}

impl ControlPlane {
    /// 处理显式启动恢复 resume 命令。
    ///
    /// 【调用者】`commands.rs` 识别 [`AUTOMATION_BOOT_RECOVERY_RESUME_COMMAND`] 后路由到这里；
    /// `DaemonHost::resume_automation_after_recovery` 是唯一的产品调用者。
    ///
    /// 【输入】`arguments` 必须是对象：`project_root` 字符串必须与请求上下文一致，`fact` 是
    /// daemon 启动时观测到的 `AutomationBootRecoveryFact`。
    ///
    /// 【拒绝顺序】非 operator → 项目不匹配 → fact 缺失/非法 → Domain 静态规则 → 状态谓词
    /// （见 [`derive_boot_recovery_resume`]）→ **实时** authority epoch 与观测值不一致。
    ///
    /// 【输出】成功返回 `{schema, fact}`，其中 fact 为 Core 推导的 Ready 副本。
    /// 【副作用】无：不写事件、不消费审批、不调用 Broker；resume 只是解除 daemon 的 dispatch
    /// fence，真正的 dispatch 仍由调用方显式启动服务并走既有 ControlPlane 路径。
    pub(crate) async fn handle_boot_recovery_resume(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        // 与 effect.reconcile 相同的 operator 边界：恢复结论会决定「重启后能不能继续派发」，
        // 不能让 cell 内的 worker 为自己的恢复状态作证。
        if context.cell_id.is_some()
            || context.actor_id.as_deref().is_none_or(str::is_empty)
            || !arguments.is_object()
        {
            return Ok(CoreResponse::blocked(
                context.request_id.clone(),
                "automation_boot_recovery_operator_required",
            ));
        }
        let declared_root = arguments["project_root"].as_str().map(str::trim);
        if declared_root.is_none_or(|root| root != context.project_root.trim()) {
            return Ok(CoreResponse::blocked(
                context.request_id.clone(),
                "automation_boot_recovery_project_mismatch",
            ));
        }
        let Some(raw_fact) = arguments.get("fact") else {
            return Ok(CoreResponse::blocked(
                context.request_id.clone(),
                "automation_boot_recovery_fact_required",
            ));
        };
        let fact: AutomationBootRecoveryFact = match serde_json::from_value(raw_fact.clone()) {
            Ok(fact) => fact,
            Err(_) => {
                return Ok(CoreResponse::blocked(
                    context.request_id.clone(),
                    "automation_boot_recovery_fact_invalid",
                ))
            }
        };
        if let Err(reason) = validate_boot_recovery(&fact) {
            return Ok(CoreResponse::blocked(context.request_id, reason));
        }
        let ready = match derive_boot_recovery_resume(&fact) {
            Ok(ready) => ready,
            Err(reason) => return Ok(CoreResponse::blocked(context.request_id, reason)),
        };
        // 实时 epoch 复核：daemon 观测之后授权代次可能已经推进，旧快照不得 resume。
        let live_epoch = self.observed_authority_epoch(&context.project_root).await?;
        if live_epoch != ready.observed_authority_epoch {
            return Ok(CoreResponse::blocked(
                context.request_id,
                "automation_boot_recovery_authority_epoch_stale",
            ));
        }
        Ok(CoreResponse::completed(
            context.request_id,
            json!({
                "schema": AUTOMATION_BOOT_RECOVERY_RESUME_SCHEMA,
                "fact": ready,
            }),
        ))
    }
}
