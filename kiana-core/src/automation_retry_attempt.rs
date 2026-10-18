//! AUT-16 Core 侧 retry 协调器：从权威 dispatch command 派生失败事实，判断是否允许按有界策略建立
//! 新的 retry attempt，并把可审计的 attempt/lease/deadline 事实登记到调用方账本。
//!
//! # 在系统中的位置
//!
//! ```text
//! 调用方（组合根/受控服务）准备 AutomationRetryCommandRequest（新 attempt 身份、lease、deadline、
//! 审批/预算快照、capability 幂等描述符）
//!     -> record_automation_retry_attempt(store, ledger, command_id, request)
//!        1. store.fetch_command() 取权威命令副本，调用方不能自带“失败事实”
//!        2. 命令状态决定：Prepared 未失败 → 拒绝；Claimed / Unknown → ReconcileUnknown；
//!           Observed+Succeeded → 拒绝；Observed+Failed → 用结构化失败码与 retryable 继续
//!        3. kiana_workflow::plan_automation_retry_attempt() 做先拒绝优先的有界分类
//!        4. 只有 NewAttempt 才把 attempt 事实登记进 ledger（序号单调、lease 不重叠、幂等键唯一）
//!     -> AttemptRecorded / AlreadyRecorded / ReconcileUnknown / Refused
//! ```
//!
//! # 边界
//!
//! 本函数只读取 store，不 claim、不 observe、不 commit 任何命令事实，也不调用 runner、Broker、
//! 模型、EventStore 或网络：它不执行任何 effect，也不把“已登记 attempt 事实”当成执行权。
//! Unknown 结果一律进入对账，绝不自动重试。真正派发仍必须走 AUT-15 的可重取命令与 ControlPlane
//! 的当前授权检查；本模块是 source 契约，尚未接入生产 worker/队列。
//!
//! 幂等描述符与审批/预算/外部 receipt 都是调用方声明的事实：本函数不查询 registry、approval
//! store 或预算账本，也不证明声明为真。它只保证“用权威命令事实 + 声明快照做出一致判定，并把决策
//! 摘要绑定进 attempt 事实”。

use kiana_domain::{
    AutomationDispatchCommand, AutomationDispatchState, AutomationObservationOutcome,
};
use kiana_ports::{AutomationDispatchStore, PortError};
use kiana_workflow::{
    plan_automation_retry_attempt, AutomationIdempotencyDescriptor, AutomationRetryAttempt,
    AutomationRetryAttemptLedger, AutomationRetryDecision, AutomationRetryRefusal,
    AutomationRetryRequest, AUTOMATION_RETRY_REQUEST_SCHEMA, MAX_AUTOMATION_RETRY_AFTER_MS,
    MAX_AUTOMATION_RETRY_ATTEMPTS,
};
use serde::{Deserialize, Serialize};

/// 本协调器请求 DTO 的固定 schema 名称。
pub const AUTOMATION_RETRY_COMMAND_REQUEST_SCHEMA: &str =
    "kiana.automation-retry-command-request.v1";

/// 命令尚未失败（从未 claim/dispatch）时的稳定拒绝码。
pub const AUTOMATION_RETRY_COMMAND_NOT_FAILED: &str = "automation_retry_command_not_failed";
/// 命令已经成功时的稳定拒绝码；成功不能靠重试改写。
pub const AUTOMATION_RETRY_COMMAND_SUCCEEDED: &str = "automation_retry_command_succeeded";

/// 调用方为“基于某条权威 dispatch command 建立新 attempt”提交的请求快照。
///
/// 无法从权威命令派生、必须由调用方声明的字段都在这里：新 attempt 身份、lease 计划、剩余预算、
/// 审批有效性、外部 effect receipt 与 capability 幂等描述符。`deny_unknown_fields` 与自校验摘要
/// 使版本外字段和篡改无法静默进入契约；这些声明不是授权，也不由本模块核实。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRetryCommandRequest {
    /// 必须精确等于 `AUTOMATION_RETRY_COMMAND_REQUEST_SCHEMA`。
    pub schema: String,
    /// 稳定逻辑 subject（例如 workflow 节点 ID）；同一 subject 的 attempt 链按它排序。
    ///
    /// 本模块不验证该 ID 是否稳定存在：跨重试复用它是调用方的审计义务。
    pub retry_subject_id: String,
    /// 新 attempt 的稳定 ID。
    pub new_attempt_id: String,
    /// 新 attempt 绑定的 fresh execution ID；不得复用失败尝试的 execution。
    pub new_execution_id: String,
    /// 新 attempt 的 lease ID。
    pub new_lease_id: String,
    /// 允许开始新 attempt 的 lease holder。
    pub new_lease_owner: String,
    /// 新 attempt lease 的时长（毫秒），必须大于 0 且与观测时间相加不越过 deadline。
    pub new_lease_ttl_ms: u64,
    /// 本 subject 允许的最大 attempt 数；失败尝试序号由权威命令给出。
    pub max_attempts: u32,
    /// 该 subject 的总 deadline（Unix 毫秒）。
    pub deadline_ms: u64,
    /// 本次判断采用的时钟快照（Unix 毫秒），由调用方提供以保持确定性。
    pub observed_at_ms: u64,
    /// 原审批当前是否仍有效。
    pub approval_valid: bool,
    /// 审批的绝对过期时间；小于等于观测时间即视为过期。
    pub approval_expires_at_ms: Option<u64>,
    /// 新 attempt 可用的剩余预算；0 表示不足。
    pub budget_remaining: u64,
    /// 上游建议等待时长（毫秒）；None 表示无建议。
    pub retry_after_ms: Option<u64>,
    /// 调用方声明的外部 effect receipt 是否存在。
    ///
    /// 命令自身的结构化失败结果是 attempt 层的 receipt；外部动作的 receipt（例如 provider 回执）
    /// 必须由调用方另行确认。任一缺失都会拒绝自动重试。
    pub effect_receipt_present: bool,
    /// 被重试 capability 的幂等描述符；决定重发是否可能重复产生外部副作用。
    pub capability: AutomationIdempotencyDescriptor,
    /// 新 attempt 使用的幂等键；提供方据此去重。
    pub idempotency_key: String,
    /// 除自身外全部字段的内容摘要；validate 会重算并比较。
    pub request_digest: String,
}

impl AutomationRetryCommandRequest {
    /// 检查 schema、身份文本、数值边界、描述符与自身摘要。
    ///
    /// 结构问题一律返回 `automation_retry_command_request_invalid`；策略是否允许重试由分类器给出
    /// 稳定 refusal，而不是这里。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RETRY_COMMAND_REQUEST_SCHEMA
            || !valid_text(&self.retry_subject_id)
            || !valid_text(&self.new_attempt_id)
            || !valid_text(&self.new_execution_id)
            || !valid_text(&self.new_lease_id)
            || !valid_text(&self.new_lease_owner)
            || !valid_text(&self.idempotency_key)
            || self.new_lease_ttl_ms == 0
            || self.max_attempts == 0
            || self.max_attempts > MAX_AUTOMATION_RETRY_ATTEMPTS
            || self.deadline_ms == 0
            || self
                .observed_at_ms
                .checked_add(self.new_lease_ttl_ms)
                .is_none()
            || self
                .retry_after_ms
                .is_some_and(|value| value > MAX_AUTOMATION_RETRY_AFTER_MS)
            || !valid_digest(&self.request_digest)
            || self.request_digest != self.digest()
        {
            return Err("automation_retry_command_request_invalid");
        }
        self.capability.validate()?;
        Ok(())
    }

    /// 按固定字段集合重算请求指纹；摘要不含密钥，不是签名。
    fn digest(&self) -> String {
        kiana_domain::json_digest(&serde_json::json!({
            "schema": self.schema,
            "retry_subject_id": self.retry_subject_id,
            "new_attempt_id": self.new_attempt_id,
            "new_execution_id": self.new_execution_id,
            "new_lease_id": self.new_lease_id,
            "new_lease_owner": self.new_lease_owner,
            "new_lease_ttl_ms": self.new_lease_ttl_ms,
            "max_attempts": self.max_attempts,
            "deadline_ms": self.deadline_ms,
            "observed_at_ms": self.observed_at_ms,
            "approval_valid": self.approval_valid,
            "approval_expires_at_ms": self.approval_expires_at_ms,
            "budget_remaining": self.budget_remaining,
            "retry_after_ms": self.retry_after_ms,
            "effect_receipt_present": self.effect_receipt_present,
            "capability": self.capability,
            "idempotency_key": self.idempotency_key,
        }))
    }

    /// 用当前字段重算摘要并校验，返回可直接提交的请求快照。
    pub fn seal(mut self) -> Result<Self, &'static str> {
        self.request_digest = self.digest();
        self.validate()?;
        Ok(self)
    }
}

/// 一次 retry 协调的最终去向；四个变体都不执行 effect，也不提交命令事实。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationRetryCommandOutcome {
    /// 已按有界策略登记新 attempt 事实；命令仍未被本函数 claim/observe。
    AttemptRecorded {
        /// 权威源命令副本（未被修改）。
        command: AutomationDispatchCommand,
        /// 本次登记的重试尝试事实。
        attempt: AutomationRetryAttempt,
    },
    /// 同一 attempt 事实已经登记过（安全重放），本次没有新建记录。
    AlreadyRecorded {
        /// 权威源命令副本。
        command: AutomationDispatchCommand,
        /// 既有 attempt 事实。
        attempt: AutomationRetryAttempt,
    },
    /// 源命令的结果不确定（Claimed/Unknown）；必须先对账，不能新建 attempt。
    ReconcileUnknown {
        /// 权威源命令副本。
        command: AutomationDispatchCommand,
        /// 稳定的对账原因码。
        reason: &'static str,
    },
    /// 分类或命令状态拒绝重试；`reason` 是稳定拒绝码，命令未被修改。
    Refused {
        /// 权威源命令副本。
        command: AutomationDispatchCommand,
        /// 稳定拒绝码（分类 refusal 或命令状态码）。
        reason: &'static str,
    },
}

/// 权威命令处于已 claim/未知状态时的对账原因码。
pub const AUTOMATION_RETRY_COMMAND_OUTCOME_UNKNOWN: &str =
    "automation_retry_command_outcome_unknown";

/// 重取权威命令、从命令事实派生失败状态、分类并登记新 attempt。
///
/// 【调用者】受控组合根/daemon 服务；本函数不执行 effect，因此调用它不会派发任何动作。
///
/// 【核心流程】
/// 1. `request.validate()` 失败返回 `PortError::Failed(automation_retry_command_request_invalid)`；
/// 2. `store.fetch_command(command_id)` 取权威副本——调用方不能自带“失败事实”或旧 revision；
/// 3. 命令状态：`Prepared` 尚未失败 → `Refused`；`Claimed`/`Unknown` → `ReconcileUnknown`；
///    `Observed` + `Succeeded` → `Refused`；`Observed` + `Failed` → 用结构化失败码与
///    `retryable` 继续（失败结果缺失是通道损坏，返回 `PortError`）；
/// 4. attempt 序号由审计链驱动：同一调用方身份（`new_attempt_id` + `new_lease_id` + 幂等键）已登记时
///    沿用该 attempt 的序号（由账本判定安全重放或冲突），否则取链上 `max + 1`（链为空时取源命令
///    `attempt + 1`）；链已到 `max_attempts` 时先返回与分类器一致的 `..._attempt_bound_exhausted`；
/// 5. 命令给出的 execution/authority/action 摘要与调用方声明的 lease/审批/预算/幂等描述符一起
///    组装并封口 `AutomationRetryRequest`；活动 lease 由账本按 subject 派生（本次要重放的同一
///    attempt 事实除外），不信任调用方漏报；
/// 6. `plan_automation_retry_attempt()` 先拒绝优先分类：Unknown→对账、任一围栏不满足→稳定拒绝、
///    全部通过→新 attempt 计划；
/// 7. 只有新 attempt 计划才会写入 ledger；重复登记同一事实返回 `AlreadyRecorded`，同一序号换内容
///    或同一幂等键换 attempt 由账本以 `PortError::Conflict` 拒绝。
///
/// 【输出】`AttemptRecorded` / `AlreadyRecorded` / `ReconcileUnknown` / `Refused`，四个分支都不返回
/// “已执行”或“已成功”。
/// 【失败】请求/分类结构错误与账本冲突映射为稳定 `PortError`；store 读取错误原样返回。所有失败
/// 路径都不写 store、不写 EventStore、不调用 runner。
/// 【副作用】只读 store 与写入调用方传入的内存账本；不执行 effect、不授权、不改变命令状态。
pub async fn record_automation_retry_attempt<S>(
    store: &S,
    ledger: &mut AutomationRetryAttemptLedger,
    command_id: &str,
    request: &AutomationRetryCommandRequest,
) -> Result<AutomationRetryCommandOutcome, PortError>
where
    S: AutomationDispatchStore + ?Sized,
{
    request.validate().map_err(retry_error)?;
    // 1. 权威副本；不接受调用方快照。
    let current = store.fetch_command(command_id).await?;
    // 2. 命令状态决定能否进入分类器：只有“已知失败 + 结构化失败结果”才继续。
    let (effect_started, effect_known, failure_retryable) = match current.intent.state {
        AutomationDispatchState::Prepared => {
            return Ok(AutomationRetryCommandOutcome::Refused {
                command: current,
                reason: AUTOMATION_RETRY_COMMAND_NOT_FAILED,
            })
        }
        AutomationDispatchState::Claimed | AutomationDispatchState::Unknown => {
            return Ok(AutomationRetryCommandOutcome::ReconcileUnknown {
                command: current,
                reason: AUTOMATION_RETRY_COMMAND_OUTCOME_UNKNOWN,
            })
        }
        AutomationDispatchState::Observed => {
            let result = current
                .result
                .as_ref()
                .ok_or_else(|| retry_port_error("automation_retry_command_result_missing"))?;
            match result.observation.outcome {
                AutomationObservationOutcome::Succeeded => {
                    return Ok(AutomationRetryCommandOutcome::Refused {
                        command: current,
                        reason: AUTOMATION_RETRY_COMMAND_SUCCEEDED,
                    })
                }
                AutomationObservationOutcome::Unknown => {
                    return Ok(AutomationRetryCommandOutcome::ReconcileUnknown {
                        command: current,
                        reason: AUTOMATION_RETRY_COMMAND_OUTCOME_UNKNOWN,
                    })
                }
                AutomationObservationOutcome::Failed => {
                    let failure = result.failure.as_ref().ok_or_else(|| {
                        retry_port_error("automation_retry_command_failure_missing")
                    })?;
                    (true, true, failure.retryable)
                }
            }
        }
    };
    // 3. 命令自身的结果必须存在，且调用方必须确认外部 effect receipt；任一缺失都不自动重试。
    let effect_receipt_present = current.result.is_some() && request.effect_receipt_present;
    // 4. attempt 序号由审计链驱动：同一调用方身份（attempt_id + lease_id + 幂等键）已登记时沿用它的
    //    序号（交给账本判定安全重放或冲突）；否则取链上 max+1，链为空时取源命令 attempt+1。
    let replay = ledger
        .retry_attempt_history(&request.retry_subject_id)
        .into_iter()
        .find(|attempt| {
            attempt.attempt_id == request.new_attempt_id
                && attempt.lease_id == request.new_lease_id
                && attempt.idempotency_key.as_deref() == Some(request.idempotency_key.as_str())
        });
    let planned_attempt = match &replay {
        Some(existing) => existing.attempt,
        None => ledger
            .last_recorded_attempt(&request.retry_subject_id)
            .map_or_else(
                || current.intent.attempt.saturating_add(1),
                |highest| highest.saturating_add(1),
            ),
    };
    let failed_attempt = planned_attempt.saturating_sub(1);
    // 链已到上界时先给出与分类器一致的稳定码，避免把策略拒绝变成结构错误。
    if failed_attempt >= request.max_attempts {
        return Ok(AutomationRetryCommandOutcome::Refused {
            command: current,
            reason: AutomationRetryRefusal::AttemptBoundExhausted.reason(),
        });
    }
    // 活动 lease 由账本派生；唯一例外是它正好属于本次要重放的同一 attempt 事实。
    let active_lease_expires_at_ms = ledger
        .active_retry_lease(&request.retry_subject_id, request.observed_at_ms)
        .filter(|attempt| {
            replay
                .as_ref()
                .is_none_or(|existing| existing.attempt_digest != attempt.attempt_digest)
        })
        .map(|attempt| attempt.lease_expires_at_ms);
    let retry_request = AutomationRetryRequest {
        schema: AUTOMATION_RETRY_REQUEST_SCHEMA.to_owned(),
        execution_id: current.intent.execution_id.clone(),
        subject_id: request.retry_subject_id.clone(),
        previous_attempt_id: current.command_id.clone(),
        attempt: failed_attempt,
        max_attempts: request.max_attempts,
        effect_started,
        effect_known,
        effect_receipt_present,
        failure_retryable,
        retry_after_ms: request.retry_after_ms,
        approval_valid: request.approval_valid,
        approval_expires_at_ms: request.approval_expires_at_ms,
        budget_remaining: request.budget_remaining,
        deadline_ms: request.deadline_ms,
        observed_at_ms: request.observed_at_ms,
        active_lease_expires_at_ms,
        previous_action_digest: current.intent.action_digest.clone(),
        action_digest: current.intent.action_digest.clone(),
        descriptor: Some(request.capability.clone()),
        idempotency_key: Some(request.idempotency_key.clone()),
        new_attempt_id: request.new_attempt_id.clone(),
        new_execution_id: request.new_execution_id.clone(),
        new_lease_id: request.new_lease_id.clone(),
        new_lease_owner: request.new_lease_owner.clone(),
        new_lease_ttl_ms: request.new_lease_ttl_ms,
        request_digest: String::new(),
    }
    .seal()
    .map_err(retry_error)?;
    // 5. 先拒绝优先分类；拒绝与 Unknown 都不触碰账本。
    match plan_automation_retry_attempt(&retry_request).map_err(retry_error)? {
        AutomationRetryDecision::ReconcileUnknown => {
            Ok(AutomationRetryCommandOutcome::ReconcileUnknown {
                command: current,
                reason: AUTOMATION_RETRY_COMMAND_OUTCOME_UNKNOWN,
            })
        }
        AutomationRetryDecision::Refused(refusal) => Ok(AutomationRetryCommandOutcome::Refused {
            command: current,
            reason: refusal.reason(),
        }),
        AutomationRetryDecision::NewAttempt(attempt) => {
            let record = ledger.record_retry_attempt(*attempt).map_err(retry_error)?;
            if record.replayed {
                Ok(AutomationRetryCommandOutcome::AlreadyRecorded {
                    command: current,
                    attempt: record.attempt,
                })
            } else {
                Ok(AutomationRetryCommandOutcome::AttemptRecorded {
                    command: current,
                    attempt: record.attempt,
                })
            }
        }
    }
}

/// 把分类/账本的稳定静态码映射为端口错误；只有真正的状态冲突才归为 `Conflict`。
fn retry_error(reason: &'static str) -> PortError {
    match reason {
        "automation_retry_attempt_conflict"
        | "automation_retry_idempotency_key_conflict"
        | "automation_retry_attempt_sequence_invalid"
        | "automation_retry_lease_held" => PortError::Conflict(reason.to_owned()),
        _ => PortError::Failed(reason.to_owned()),
    }
}

/// 构造一个稳定的失败端口错误，用于命令事实损坏（结果/失败对象缺失）。
fn retry_port_error(reason: &'static str) -> PortError {
    PortError::Failed(reason.to_owned())
}

/// 检查 ID/键类文本：非空、不超过 512 UTF-8 字节、不含 NUL/换行/回车与任何 Unicode 空白。
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 512
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

/// 检查 `sha256:` 前缀加 64 位十六进制字符；不验证摘要对应内容。
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
