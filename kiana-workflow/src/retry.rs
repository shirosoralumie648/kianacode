//! AUT-16 有界重试分类器、可审计 retry attempt/lease/deadline 事实与 capability 幂等描述符。
//!
//! # 在系统中的位置
//!
//! ```text
//! 调用侧收集一次失败尝试的权威快照（attempt、effect 结果确定性、receipt、审批、预算、deadline、
//! 现有 lease）与 capability 幂等描述符
//!     -> plan_automation_retry_attempt(request)   纯分类：只返回 ReconcileUnknown / Refused /
//!                                                 NewAttempt(可记录的 attempt 计划)
//!     -> record_retry_attempt(&mut ledger, attempt) 把该计划登记为按 subject 排序的可审计事实
//!     -> validate_retry_attempt_start(...)         只有登记时的 lease owner、未过期 lease 与未过
//!                                                deadline 才允许后续持有人开始这次尝试
//!     -> 调用侧自行决定是否预留新 execution、重新授权并发起 effect（不在本文件）
//! ```
//!
//! 本文件是 `kiana-workflow` 的纯规划模块，和 `durable.rs` 一样不读写时钟、不访问 EventStore、
//! 不调用 Broker/模型/网络，也不执行任何 effect。它回答的是“这次失败是否允许按有界策略建立新
//! attempt”，不是“授权这次重试”。`AutomationRetryAttempt` 只是登记事实：它记录 lease 与 deadline
//! 计划、被重试的源尝试、幂等键和决策摘要，持有它不代表外部动作已经发生或已获准。
//!
//! # 先拒绝优先的判定顺序
//!
//! 1. 输入结构/摘要错误 → `automation_retry_request_invalid`（不产生计划）；
//! 2. effect 已开始但结果未知 → `ReconcileUnknown`，绝不进入重试分支；
//! 3. effect 已开始但缺少 receipt → `Refused(EffectReceiptMissing)`；
//! 4. 动作内容与源尝试不一致 → `Refused(ActionChanged)`；
//! 5. attempt/deadline/lease/审批/预算/幂等/退避逐个围栏，任一不满足即 `Refused(<稳定码>)`；
//! 6. 全部通过才返回 `NewAttempt`，且新 attempt 的 lease 落在 deadline 之内。
//!
//! ⚠ 所有布尔字段都是调用方声明的事实快照：本模块不查询 approval store、预算账本、运行证据或
//! provider receipt，也没有密钥签名。因此 `NewAttempt` 不是授权，未经验证的声明不能被当成证据。
//! `durable.rs::plan_command()` 的 `AutomationCommand::Retry` 是另一条（纯节点）路径，不调用本文件。

use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// capability 幂等描述符使用的固定 schema 名称。
pub const AUTOMATION_IDEMPOTENCY_DESCRIPTOR_SCHEMA: &str =
    "kiana.automation-capability-idempotency.v1";
/// 重试决策输入快照使用的固定 schema 名称。
pub const AUTOMATION_RETRY_REQUEST_SCHEMA: &str = "kiana.automation-retry-request.v1";
/// 可审计 retry attempt 事实使用的固定 schema 名称。
pub const AUTOMATION_RETRY_ATTEMPT_SCHEMA: &str = "kiana.automation-retry-attempt.v1";

/// 上游建议等待时长的上界（24 小时）；更大的值在输入校验阶段被拒绝，而不是被静默截断。
pub const MAX_AUTOMATION_RETRY_AFTER_MS: u64 = 86_400_000;
/// 单次执行允许声明的最大 attempt 数上界；真实上限由定义/策略给出，这里只拒绝荒谬值。
pub const MAX_AUTOMATION_RETRY_ATTEMPTS: u32 = 32;

/// 被重试对象是否会产生外部 effect。
///
/// 这个分类决定重试是否必须依赖幂等键：`PureNoDispatch`（例如 Literal/Gate 这类纯节点）没有
/// 需要去重的外部动作，`ExternalEffect` 则必须在幂等描述符下方可重发。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationEffectClass {
    /// 纯计算/无派发：失败重试不会重复产生外部副作用。
    PureNoDispatch,
    /// 外部 effect：重发必须由幂等描述符与幂等键约束。
    ExternalEffect,
}

/// capability 声明的幂等语义。
///
/// 本枚举只是声明值，不证明提供方真的实现了去重；`Idempotent` 必须同时给出幂等键 scope，
/// `NonIdempotent` 一律不进入自动重试。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationIdempotencyClass {
    /// 声明可安全重发（依赖同一幂等键）。
    Idempotent,
    /// 声明不可安全重发；自动重试被拒绝。
    NonIdempotent,
}

/// capability 的幂等描述符：重试安全性的唯一声明来源。
///
/// 【为什么存在】没有描述符就无法区分“重发同一个动作是安全的”和“重发会重复扣费/重复写入”。
/// 本对象把该判断固定成一份带自校验摘要的可序列化事实，使重试分类器不必猜测 capability 名称。
///
/// 【证明边界】`descriptor_digest` 是无密钥内容指纹，不是签名；本对象不查询 capability registry，
/// 也不证明该 capability 真的幂等。描述符由调用侧提供，其真实性仍由 ControlPlane/Broker 合同负责。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationIdempotencyDescriptor {
    /// 必须精确等于 `AUTOMATION_IDEMPOTENCY_DESCRIPTOR_SCHEMA`。
    pub schema: String,
    /// 被描述的 capability 名称；本模块只检查文本形状，不查询 registry。
    pub capability: String,
    /// 该 capability 是否产生外部 effect。
    pub effect_class: AutomationEffectClass,
    /// capability 声明的幂等语义。
    pub idempotency: AutomationIdempotencyClass,
    /// 幂等键作用域；`ExternalEffect + Idempotent` 必须给出，其他组合必须为空。
    pub idempotency_key_scope: Option<String>,
    /// 对上方字段的 `sha256:` 内容摘要；validate 会重算并比较。
    pub descriptor_digest: String,
}

impl AutomationIdempotencyDescriptor {
    /// 构造并自校验一份描述符；形状不合法时返回稳定静态错误码。
    pub fn new(
        capability: impl Into<String>,
        effect_class: AutomationEffectClass,
        idempotency: AutomationIdempotencyClass,
        idempotency_key_scope: Option<String>,
    ) -> Result<Self, &'static str> {
        let mut descriptor = Self {
            schema: AUTOMATION_IDEMPOTENCY_DESCRIPTOR_SCHEMA.to_owned(),
            capability: capability.into(),
            effect_class,
            idempotency,
            idempotency_key_scope,
            descriptor_digest: String::new(),
        };
        descriptor.descriptor_digest = descriptor.digest();
        descriptor.validate()?;
        Ok(descriptor)
    }

    /// 检查 schema、claim 文本、幂等键 scope 组合与自身摘要。
    ///
    /// 幂等键 scope 的组合规则是封闭的：只有 `ExternalEffect + Idempotent` 允许（并要求）scope，
    /// 其他组合携带 scope 会被拒绝。它不校验 capability 是否存在、是否真的幂等。
    pub fn validate(&self) -> Result<(), &'static str> {
        let external_idempotent = self.effect_class == AutomationEffectClass::ExternalEffect
            && self.idempotency == AutomationIdempotencyClass::Idempotent;
        if self.schema != AUTOMATION_IDEMPOTENCY_DESCRIPTOR_SCHEMA
            || !valid_text(&self.capability)
            || external_idempotent != self.idempotency_key_scope.is_some()
            || self
                .idempotency_key_scope
                .as_deref()
                .is_some_and(|scope| !valid_text(scope))
            || !valid_digest(&self.descriptor_digest)
            || self.descriptor_digest != self.digest()
        {
            return Err("automation_idempotency_descriptor_invalid");
        }
        Ok(())
    }

    /// 按固定字段集合重算描述符摘要；摘要不含密钥，不是签名。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "capability": self.capability,
            "effect_class": self.effect_class,
            "idempotency": self.idempotency,
            "idempotency_key_scope": self.idempotency_key_scope,
        }))
    }
}

/// 分类器拒绝重试时给出的稳定原因；这些码是策略结果，不是权限。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomationRetryRefusal {
    /// effect 已开始且调用方声明已有确定结果，但缺少该 effect 的 receipt，无法把新尝试绑定到证据。
    EffectReceiptMissing,
    /// 失败未被声明为可重试（例如确定性业务失败）；重试不会改变结果。
    NotRetryableFailure,
    /// 已经尝试过外部动作却没有幂等描述符；无法证明重发安全。
    DescriptorRequired,
    /// 幂等描述符声明为 `PureNoDispatch`，不能用来为外部 effect 的重复执行背书。
    EffectClassMismatch,
    /// 幂等描述符声明 capability 非幂等；自动重试一律拒绝。
    NonIdempotentCapability,
    /// 声明幂等但新尝试没有给出幂等键，无法让提供方去重。
    IdempotencyKeyMissing,
    /// 审批当前无效（被拒绝/撤销/越权）。
    ApprovalInvalid,
    /// 审批已过期；必须重新走审批而不是自动重试。
    ApprovalExpired,
    /// 剩余预算为 0；重试不得超预算。
    BudgetInsufficient,
    /// 已达到 attempt 上界。
    AttemptBoundExhausted,
    /// 已到达总 deadline。
    DeadlineExceeded,
    /// 退避等待长于剩余 deadline，等待本身就会越界。
    RetryAfterExceedsDeadline,
    /// 同一 subject 存在尚未过期的 lease；直接再领可能重复执行。
    LeaseHeld,
    /// 计划 lease 的到期时间超过总 deadline。
    LeaseExceedsDeadline,
    /// 本次动作内容与失败尝试不同；不能借重试替换动作。
    ActionChanged,
}

impl AutomationRetryRefusal {
    /// 返回稳定的拒绝码，供事件/测试/入口按码断言。
    pub const fn reason(self) -> &'static str {
        match self {
            Self::EffectReceiptMissing => "automation_retry_effect_receipt_missing",
            Self::NotRetryableFailure => "automation_retry_not_retryable_failure",
            Self::DescriptorRequired => "automation_retry_descriptor_required",
            Self::EffectClassMismatch => "automation_retry_effect_class_mismatch",
            Self::NonIdempotentCapability => "automation_retry_non_idempotent_capability",
            Self::IdempotencyKeyMissing => "automation_retry_idempotency_key_missing",
            Self::ApprovalInvalid => "automation_retry_approval_invalid",
            Self::ApprovalExpired => "automation_retry_approval_expired",
            Self::BudgetInsufficient => "automation_retry_budget_insufficient",
            Self::AttemptBoundExhausted => "automation_retry_attempt_bound_exhausted",
            Self::DeadlineExceeded => "automation_retry_deadline_exceeded",
            Self::RetryAfterExceedsDeadline => "automation_retry_retry_after_exceeds_deadline",
            Self::LeaseHeld => "automation_retry_lease_held",
            Self::LeaseExceedsDeadline => "automation_retry_lease_exceeds_deadline",
            Self::ActionChanged => "automation_retry_action_changed",
        }
    }
}

/// 调用侧为单次重试判断提交的有界事实快照。
///
/// 字段来源由调用方负责：本模块只检查形状、摘要和字段间可静态判定的关系，不查询审批/预算/运行
/// 证据/clock。`deny_unknown_fields` 拒绝版本外字段，避免未知数据静默进入已校验契约。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRetryRequest {
    /// 必须精确等于 `AUTOMATION_RETRY_REQUEST_SCHEMA`。
    pub schema: String,
    /// 失败尝试所属的 execution ID；只做审计绑定，未查询 execution registry。
    pub execution_id: String,
    /// 逻辑 subject（workflow 节点或 automation command）。同一 subject 的 attempt 链按它排序。
    pub subject_id: String,
    /// 被重试的那次尝试身份（workflow 可用 execution_id，automation 可用 command_id）。
    pub previous_attempt_id: String,
    /// 失败尝试的序号，从 1 开始；新尝试序号是它加一。
    pub attempt: u32,
    /// 本 subject 允许的最大 attempt 数；新尝试超过它即拒绝。
    pub max_attempts: u32,
    /// 失败尝试是否已经触达外部动作（已 claim/派发即为 true）。
    pub effect_started: bool,
    /// 调用方是否已得到确定结果；false 表示结果未知。
    pub effect_known: bool,
    /// 该 effect 是否已有 receipt 事实；effect 已开始但没有 receipt 时不得自动重试。
    pub effect_receipt_present: bool,
    /// 失败是否被声明为可重试（瞬时/可恢复）；确定性失败不进入重试。
    pub failure_retryable: bool,
    /// 上游建议等待时长（毫秒）；None 表示无建议。
    pub retry_after_ms: Option<u64>,
    /// 原审批当前是否仍然有效。
    pub approval_valid: bool,
    /// 审批的绝对过期时间；小于等于观测时间即视为过期。
    pub approval_expires_at_ms: Option<u64>,
    /// 可用于新尝试的剩余预算；0 表示不足。
    pub budget_remaining: u64,
    /// 该 subject 的总 deadline（Unix 毫秒）。
    pub deadline_ms: u64,
    /// 本次判断采用的时钟快照（Unix 毫秒）；由调用方提供，使分类确定性可重放。
    pub observed_at_ms: u64,
    /// 同一 subject 当前未过期 lease 的到期时间；存在且未过期即拒绝直接再领。
    pub active_lease_expires_at_ms: Option<u64>,
    /// 失败尝试冻结的动作摘要。
    pub previous_action_digest: String,
    /// 新尝试将要执行的动作摘要；必须与失败尝试相同。
    pub action_digest: String,
    /// capability 幂等描述符；纯/无派发重试可以为空。
    pub descriptor: Option<AutomationIdempotencyDescriptor>,
    /// 新尝试使用的幂等键；声明幂等时必填。
    pub idempotency_key: Option<String>,
    /// 新尝试的稳定 ID。
    pub new_attempt_id: String,
    /// 新尝试绑定的 fresh execution ID。
    pub new_execution_id: String,
    /// 新尝试的 lease ID。
    pub new_lease_id: String,
    /// 新尝试的 lease holder（后续 start 校验必须匹配）。
    pub new_lease_owner: String,
    /// 新尝试 lease 的时长（毫秒）；必须大于 0，且与观测时间相加不得越过 deadline。
    pub new_lease_ttl_ms: u64,
    /// 除自身外全部字段的内容摘要；validate 会重算并比较。
    pub request_digest: String,
}

impl AutomationRetryRequest {
    /// 检查 schema、身份文本、数值边界、描述符、幂等键与自身摘要。
    ///
    /// 任意结构问题都返回 `automation_retry_request_invalid`；它不判断策略是否允许重试
    /// （那是 `plan_automation_retry_attempt` 的稳定 refusal），也不验证布尔字段的现实真伪。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RETRY_REQUEST_SCHEMA
            || !valid_text(&self.execution_id)
            || !valid_text(&self.subject_id)
            || !valid_text(&self.previous_attempt_id)
            || !valid_text(&self.new_attempt_id)
            || !valid_text(&self.new_execution_id)
            || !valid_text(&self.new_lease_id)
            || !valid_text(&self.new_lease_owner)
            || self.attempt == 0
            || self.max_attempts == 0
            || self.max_attempts > MAX_AUTOMATION_RETRY_ATTEMPTS
            || self.attempt > self.max_attempts
            || self.new_lease_ttl_ms == 0
            || self.deadline_ms == 0
            || self
                .observed_at_ms
                .checked_add(self.new_lease_ttl_ms)
                .is_none()
            || self
                .retry_after_ms
                .is_some_and(|value| value > MAX_AUTOMATION_RETRY_AFTER_MS)
            || self
                .idempotency_key
                .as_deref()
                .is_some_and(|key| !valid_text(key))
            || (self.idempotency_key.is_some() && self.descriptor.is_none())
            || !valid_digest(&self.previous_action_digest)
            || !valid_digest(&self.action_digest)
            || !valid_digest(&self.request_digest)
            || self.request_digest != self.digest()
        {
            return Err("automation_retry_request_invalid");
        }
        if let Some(descriptor) = &self.descriptor {
            descriptor.validate()?;
        }
        Ok(())
    }

    /// 按固定字段集合调用 Domain `json_digest()` 重算请求指纹；摘要不是签名。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "subject_id": self.subject_id,
            "previous_attempt_id": self.previous_attempt_id,
            "attempt": self.attempt,
            "max_attempts": self.max_attempts,
            "effect_started": self.effect_started,
            "effect_known": self.effect_known,
            "effect_receipt_present": self.effect_receipt_present,
            "failure_retryable": self.failure_retryable,
            "retry_after_ms": self.retry_after_ms,
            "approval_valid": self.approval_valid,
            "approval_expires_at_ms": self.approval_expires_at_ms,
            "budget_remaining": self.budget_remaining,
            "deadline_ms": self.deadline_ms,
            "observed_at_ms": self.observed_at_ms,
            "active_lease_expires_at_ms": self.active_lease_expires_at_ms,
            "previous_action_digest": self.previous_action_digest,
            "action_digest": self.action_digest,
            "descriptor": self.descriptor,
            "idempotency_key": self.idempotency_key,
            "new_attempt_id": self.new_attempt_id,
            "new_execution_id": self.new_execution_id,
            "new_lease_id": self.new_lease_id,
            "new_lease_owner": self.new_lease_owner,
            "new_lease_ttl_ms": self.new_lease_ttl_ms,
        }))
    }

    /// 用当前字段重算摘要并校验，返回可以直接提交给分类器的快照。
    ///
    /// 调用方先组装字段（`request_digest` 置空字符串），再用本方法封口；这样重试链上的任何字段
    /// 变化都会反映到 `request_digest` 与最终 attempt 的 `decision_digest` 中。
    pub fn seal(mut self) -> Result<Self, &'static str> {
        self.request_digest = self.digest();
        self.validate()?;
        Ok(self)
    }
}

/// 分类结果：一个可登记的新 attempt、一次 Unknown 对账要求，或一个稳定拒绝。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationRetryDecision {
    /// 唯一允许重试的分支；`AutomationRetryAttempt` 仍只是待登记事实，不含执行权。
    NewAttempt(Box<AutomationRetryAttempt>),
    /// effect 已开始但结果未知；必须先读权威事实，不能盲目新建 attempt。
    ReconcileUnknown,
    /// 条件不满足；调用方应保留拒绝/失败处理路径，并可使用稳定码上报。
    Refused(AutomationRetryRefusal),
}

/// 一次已登记的重试尝试事实：绑定源尝试、动作、lease、deadline 与决策摘要。
///
/// 【为什么存在】“重试了多少次、每次谁在什么时候开始、lease 到几点、用了哪个幂等键”必须可审计，
/// 否则无法区分安全重试与重复副作用。本对象把这些信息固定成带自校验摘要的不可变记录。
///
/// 【证明边界】它不证明 lease 真的被遵守、不证明 deadline 由权威时钟计算、不证明动作被允许；
/// lease 与 deadline 的强制执行仍要由实际 dispatch/queue 路径完成。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRetryAttempt {
    /// 必须精确等于 `AUTOMATION_RETRY_ATTEMPT_SCHEMA`。
    pub schema: String,
    /// 本次尝试的稳定 ID。
    pub attempt_id: String,
    /// 被重试的失败尝试所属 execution。
    pub execution_id: String,
    /// 本次尝试绑定的 fresh execution。
    pub new_execution_id: String,
    /// 逻辑 subject；同一 subject 的 attempt 链按序号排序。
    pub subject_id: String,
    /// 被重试的尝试身份。
    pub previous_attempt_id: String,
    /// 本次尝试序号（= 失败 attempt + 1，至少为 2）。
    pub attempt: u32,
    /// 本 subject 允许的最大 attempt 数。
    pub max_attempts: u32,
    /// 本次尝试执行的动作摘要（与失败尝试一致）。
    pub action_digest: String,
    /// 被重试 capability 名称；纯/无派发重试为空。
    pub capability: Option<String>,
    /// 幂等描述符摘要；与 `capability` 同时存在或同时为空。
    pub descriptor_digest: Option<String>,
    /// 本次尝试使用的幂等键。
    pub idempotency_key: Option<String>,
    /// 登记时签发的 lease ID。
    pub lease_id: String,
    /// 允许开始本次尝试的 lease holder。
    pub lease_owner: String,
    /// lease 到期时间；过期后不得开始，只能按有界策略重新登记。
    pub lease_expires_at_ms: u64,
    /// 该 subject 的总 deadline。
    pub deadline_ms: u64,
    /// 登记时间（调用方提供的时钟快照）。
    pub created_at_ms: u64,
    /// 计划等待时长；登记时已保证不会越过 deadline。
    pub retry_after_ms: Option<u64>,
    /// 产生该计划的请求摘要；把决策快照绑定进审计链。
    pub decision_digest: String,
    /// 对上方字段的 `sha256:` 内容摘要；validate 会重算并比较。
    pub attempt_digest: String,
}

impl AutomationRetryAttempt {
    /// 检查 schema、身份文本、序号/lease/deadline 组合、幂等字段一致性与自身摘要。
    ///
    /// 结构问题一律返回 `automation_retry_attempt_invalid`；它不检查外部事实，也不修改字段。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_RETRY_ATTEMPT_SCHEMA
            || !valid_text(&self.attempt_id)
            || !valid_text(&self.execution_id)
            || !valid_text(&self.new_execution_id)
            || !valid_text(&self.subject_id)
            || !valid_text(&self.previous_attempt_id)
            || !valid_text(&self.lease_id)
            || !valid_text(&self.lease_owner)
            || self
                .capability
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self.capability.is_some() != self.descriptor_digest.is_some()
            || self
                .idempotency_key
                .as_deref()
                .is_some_and(|key| !valid_text(key))
            || (self.idempotency_key.is_some() && self.descriptor_digest.is_none())
            || self.attempt < 2
            || self.attempt > self.max_attempts
            || self.attempt > MAX_AUTOMATION_RETRY_ATTEMPTS
            || self.created_at_ms >= self.deadline_ms
            || self.lease_expires_at_ms <= self.created_at_ms
            || self.lease_expires_at_ms > self.deadline_ms
            || self
                .retry_after_ms
                .is_some_and(|value| value > MAX_AUTOMATION_RETRY_AFTER_MS)
            || !valid_digest(&self.action_digest)
            || self
                .descriptor_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || !valid_digest(&self.decision_digest)
            || !valid_digest(&self.attempt_digest)
            || self.attempt_digest != self.digest()
        {
            return Err("automation_retry_attempt_invalid");
        }
        if let Some(retry_after) = self.retry_after_ms {
            if self
                .created_at_ms
                .checked_add(retry_after)
                .is_none_or(|ready_at| ready_at >= self.deadline_ms)
            {
                return Err("automation_retry_attempt_invalid");
            }
        }
        Ok(())
    }

    /// 该 lease 在 `now_ms` 是否仍然有效（未到期）。
    ///
    /// 只做时间比较，不查询 holder 是否存活，也不自动延长 lease。
    pub fn is_lease_active_at(&self, now_ms: u64) -> bool {
        now_ms < self.lease_expires_at_ms
    }

    /// 按固定字段集合重算尝试摘要；摘要不含密钥，不是签名。
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "attempt_id": self.attempt_id,
            "execution_id": self.execution_id,
            "new_execution_id": self.new_execution_id,
            "subject_id": self.subject_id,
            "previous_attempt_id": self.previous_attempt_id,
            "attempt": self.attempt,
            "max_attempts": self.max_attempts,
            "action_digest": self.action_digest,
            "capability": self.capability,
            "descriptor_digest": self.descriptor_digest,
            "idempotency_key": self.idempotency_key,
            "lease_id": self.lease_id,
            "lease_owner": self.lease_owner,
            "lease_expires_at_ms": self.lease_expires_at_ms,
            "deadline_ms": self.deadline_ms,
            "created_at_ms": self.created_at_ms,
            "retry_after_ms": self.retry_after_ms,
            "decision_digest": self.decision_digest,
        }))
    }

    /// 用当前字段重算摘要并校验，返回可登记事实。
    pub fn seal(mut self) -> Result<Self, &'static str> {
        self.attempt_digest = self.digest();
        self.validate()?;
        Ok(self)
    }
}

/// 根据一次失败尝试的事实快照，给出有界重试分类。
///
/// 【调用者】`kiana-core` 的 retry 协调器（从权威 dispatch command 派生事实后转调）与 Domain/
/// Workflow 契约测试。
/// 【输入】`request` 是调用方封口（`seal()`）后的快照，包含 attempt/deadline/lease、审批、预算、
/// 幂等描述符与源动作摘要。
/// 【输出】`ReconcileUnknown`（effect 已开始但结果未知）、`Refused(<稳定码>)`（其余任一围栏不满足）
/// 或 `NewAttempt`（全部通过；返回带 lease/deadline/decision digest 的可登记事实）。
/// 【失败】输入结构/摘要错误返回 `automation_retry_request_invalid`；策略拒绝不是错误。
/// 【副作用】纯计算：无时钟读取、无等待、无预算扣减、无 EventStore、无网络、无 dispatch。
///
/// ⚠ 判断顺序先保证“Unknown 绝不重试”，再保证“effect 已开始必须有 receipt 与幂等描述符”。
/// 因此非幂等 capability、过期审批、预算不足、attempt/deadline 越界都返回拒绝码，不返回尝试计划。
pub fn plan_automation_retry_attempt(
    request: &AutomationRetryRequest,
) -> Result<AutomationRetryDecision, &'static str> {
    request.validate()?;
    // 1. 结果未知优先：拿不准就不能新建 attempt，必须先对账。
    if request.effect_started && !request.effect_known {
        return Ok(AutomationRetryDecision::ReconcileUnknown);
    }
    // 2. effect 已开始但没有 receipt：没有证据可绑定，不自动重试。
    if request.effect_started && !request.effect_receipt_present {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::EffectReceiptMissing,
        ));
    }
    // 3. 动作必须与失败尝试完全一致，重试不能成为替换动作的通道。
    if request.previous_action_digest != request.action_digest {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::ActionChanged,
        ));
    }
    // 4. 失败本身必须被声明为可重试。
    if !request.failure_retryable {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::NotRetryableFailure,
        ));
    }
    // 5. attempt/deadline/lease 边界。
    if request.attempt >= request.max_attempts {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::AttemptBoundExhausted,
        ));
    }
    if request.observed_at_ms >= request.deadline_ms {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::DeadlineExceeded,
        ));
    }
    if request
        .active_lease_expires_at_ms
        .is_some_and(|expires_at| expires_at > request.observed_at_ms)
    {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::LeaseHeld,
        ));
    }
    // 6. 审批：无效与过期分开报告，避免把“需要重新审批”写成“重试被策略拒绝”。
    if !request.approval_valid {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::ApprovalInvalid,
        ));
    }
    if request
        .approval_expires_at_ms
        .is_some_and(|expires_at| expires_at <= request.observed_at_ms)
    {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::ApprovalExpired,
        ));
    }
    // 7. 预算：没有剩余预算就不能建立新尝试。
    if request.budget_remaining == 0 {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::BudgetInsufficient,
        ));
    }
    // 8. 幂等：已开始的外部 effect 必须有 ExternalEffect+Idempotent 描述符与幂等键。
    if let Some(descriptor) = &request.descriptor {
        if descriptor.effect_class != AutomationEffectClass::ExternalEffect {
            return Ok(AutomationRetryDecision::Refused(
                AutomationRetryRefusal::EffectClassMismatch,
            ));
        }
        if descriptor.idempotency != AutomationIdempotencyClass::Idempotent {
            return Ok(AutomationRetryDecision::Refused(
                AutomationRetryRefusal::NonIdempotentCapability,
            ));
        }
        if request.idempotency_key.is_none() {
            return Ok(AutomationRetryDecision::Refused(
                AutomationRetryRefusal::IdempotencyKeyMissing,
            ));
        }
    } else if request.effect_started {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::DescriptorRequired,
        ));
    }
    // 9. 退避与 lease 窗口都必须落在剩余 deadline 内。
    let remaining_ms = request.deadline_ms - request.observed_at_ms;
    if request
        .retry_after_ms
        .is_some_and(|retry_after| retry_after >= remaining_ms)
    {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::RetryAfterExceedsDeadline,
        ));
    }
    let lease_expires_at_ms = request.observed_at_ms + request.new_lease_ttl_ms;
    if lease_expires_at_ms > request.deadline_ms {
        return Ok(AutomationRetryDecision::Refused(
            AutomationRetryRefusal::LeaseExceedsDeadline,
        ));
    }
    // 10. 全部通过：只返回可登记事实，不预留执行权。
    let next_attempt = request
        .attempt
        .checked_add(1)
        .ok_or("automation_retry_request_invalid")?;
    let descriptor = request.descriptor.as_ref();
    let attempt = AutomationRetryAttempt {
        schema: AUTOMATION_RETRY_ATTEMPT_SCHEMA.to_owned(),
        attempt_id: request.new_attempt_id.clone(),
        execution_id: request.execution_id.clone(),
        new_execution_id: request.new_execution_id.clone(),
        subject_id: request.subject_id.clone(),
        previous_attempt_id: request.previous_attempt_id.clone(),
        attempt: next_attempt,
        max_attempts: request.max_attempts,
        action_digest: request.action_digest.clone(),
        capability: descriptor.map(|value| value.capability.clone()),
        descriptor_digest: descriptor.map(|value| value.descriptor_digest.clone()),
        idempotency_key: request.idempotency_key.clone(),
        lease_id: request.new_lease_id.clone(),
        lease_owner: request.new_lease_owner.clone(),
        lease_expires_at_ms,
        deadline_ms: request.deadline_ms,
        created_at_ms: request.observed_at_ms,
        retry_after_ms: request.retry_after_ms,
        decision_digest: request.request_digest.clone(),
        attempt_digest: String::new(),
    };
    Ok(AutomationRetryDecision::NewAttempt(Box::new(
        attempt.seal()?,
    )))
}

/// 登记结果：`replayed=true` 表示同一事实已经存在，没有新建记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationRetryAttemptRecord {
    /// 当前权威副本（新建或已存在的记录）。
    pub attempt: AutomationRetryAttempt,
    /// 是否命中相同事实并重放，而不是插入新记录。
    pub replayed: bool,
}

/// 进程内 retry attempt 账本：按 subject 保存有序尝试链并维护幂等键索引。
///
/// 【当前范围】只是内存 `BTreeMap`：没有容量上限、过期清理、持久化或跨进程协调，因此
/// `record_retry_attempt` 成功只代表当前进程认下了这条 attempt 事实，不代表它已落盘或已调度。
/// 真正 durable 的 attempt/queue 事实由 AUT-08/AUT-15 的存储适配器负责，两者尚未接线。
#[derive(Clone, Debug, Default)]
pub struct AutomationRetryAttemptLedger {
    /// key = (subject_id, attempt)，值按 key 排序即为审计链。
    attempts: BTreeMap<(String, u32), AutomationRetryAttempt>,
    /// attempt_id 到 (subject_id, attempt) 的索引，用于拒绝一 ID 多记录。
    by_attempt_id: BTreeMap<String, (String, u32)>,
    /// 幂等键到首次 attempt 摘要的索引，用于区分重复登记与键冲突。
    by_idempotency_key: BTreeMap<String, String>,
}

impl AutomationRetryAttemptLedger {
    /// 建立空账本；等价于 `Default`。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一次 retry attempt 事实。
    ///
    /// 【调用者】`kiana-core` 的 retry 协调器与 Workflow 契约测试。
    /// 【核心流程】数据结构校验 → 同 (subject, attempt) 同摘要视为安全重放、不同摘要拒绝 →
    /// attempt_id 不得映射到别处 → 同幂等键不得对应不同摘要 → 同一 subject 的序号必须严格递增
    /// （已有记录时只接受 max+1）→ 同一 subject 存在未过期 lease 时不得登记新 attempt。
    /// 【输出】合法时写入并返回记录；重复登记返回既有副本且 `replayed=true`。
    /// 【失败】冲突返回 `automation_retry_attempt_conflict`、
    /// `automation_retry_idempotency_key_conflict`、`automation_retry_attempt_sequence_invalid` 或
    /// `automation_retry_lease_held`；结构问题返回 attempt 自身的错误码。所有失败路径不改变账本。
    /// 【副作用】只修改传入的内存账本；不写 EventStore、不执行 effect、不续租。
    pub fn record_retry_attempt(
        &mut self,
        attempt: AutomationRetryAttempt,
    ) -> Result<AutomationRetryAttemptRecord, &'static str> {
        attempt.validate()?;
        let key = (attempt.subject_id.clone(), attempt.attempt);
        if let Some(existing) = self.attempts.get(&key) {
            if existing.attempt_digest == attempt.attempt_digest {
                return Ok(AutomationRetryAttemptRecord {
                    attempt: existing.clone(),
                    replayed: true,
                });
            }
            return Err("automation_retry_attempt_conflict");
        }
        if self
            .by_attempt_id
            .get(&attempt.attempt_id)
            .is_some_and(|existing| *existing != key)
        {
            return Err("automation_retry_attempt_conflict");
        }
        if let Some(idempotency_key) = &attempt.idempotency_key {
            if self
                .by_idempotency_key
                .get(idempotency_key)
                .is_some_and(|digest| *digest != attempt.attempt_digest)
            {
                return Err("automation_retry_idempotency_key_conflict");
            }
        }
        if let Some(highest) = self.max_recorded_attempt(&attempt.subject_id) {
            let expected = highest
                .checked_add(1)
                .ok_or("automation_retry_attempt_sequence_invalid")?;
            if attempt.attempt != expected {
                return Err("automation_retry_attempt_sequence_invalid");
            }
        }
        let lease_held = self
            .attempts
            .range((attempt.subject_id.clone(), 0)..=(attempt.subject_id.clone(), u32::MAX))
            .any(|(_, recorded)| recorded.is_lease_active_at(attempt.created_at_ms));
        if lease_held {
            return Err("automation_retry_lease_held");
        }
        self.attempts.insert(key.clone(), attempt.clone());
        self.by_attempt_id.insert(attempt.attempt_id.clone(), key);
        if let Some(idempotency_key) = &attempt.idempotency_key {
            self.by_idempotency_key
                .insert(idempotency_key.clone(), attempt.attempt_digest.clone());
        }
        Ok(AutomationRetryAttemptRecord {
            attempt,
            replayed: false,
        })
    }

    /// 读取单条 attempt 事实；不存在返回 `automation_retry_attempt_not_found`。
    pub fn fetch_retry_attempt(
        &self,
        subject_id: &str,
        attempt: u32,
    ) -> Result<AutomationRetryAttempt, &'static str> {
        self.attempts
            .get(&(subject_id.to_owned(), attempt))
            .cloned()
            .ok_or("automation_retry_attempt_not_found")
    }

    /// 返回该 subject 的全部 attempt，按序号升序；这是重试的可审计轨迹。
    pub fn retry_attempt_history(&self, subject_id: &str) -> Vec<AutomationRetryAttempt> {
        self.attempts
            .range((subject_id.to_owned(), 0)..=(subject_id.to_owned(), u32::MAX))
            .map(|(_, attempt)| attempt.clone())
            .collect()
    }

    /// 返回该 subject 在 `now_ms` 仍然有效的 lease 中到期最晚的一条记录。
    ///
    /// 只按账本内已登记事实判断，不代表外部 worker 真的还在运行。
    pub fn active_retry_lease(
        &self,
        subject_id: &str,
        now_ms: u64,
    ) -> Option<AutomationRetryAttempt> {
        self.attempts
            .range((subject_id.to_owned(), 0)..=(subject_id.to_owned(), u32::MAX))
            .filter(|(_, attempt)| attempt.is_lease_active_at(now_ms))
            .max_by_key(|(_, attempt)| attempt.lease_expires_at_ms)
            .map(|(_, attempt)| attempt.clone())
    }

    /// 已登记的 attempt 总数。
    pub fn len(&self) -> usize {
        self.attempts.len()
    }

    /// 账本是否为空。
    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty()
    }

    /// 该 subject 已登记的最大 attempt 序号；没有记录时为空。
    ///
    /// 【调用者】Core retry 协调器用它把同一条链的下一个 attempt 序号定为 `max + 1`，使审计链单调；
    /// 它只读取已登记事实，不代表外部执行到了哪一步。
    pub fn last_recorded_attempt(&self, subject_id: &str) -> Option<u32> {
        self.max_recorded_attempt(subject_id)
    }

    /// 该 subject 已登记的最大 attempt 序号；没有记录时为空。
    fn max_recorded_attempt(&self, subject_id: &str) -> Option<u32> {
        self.attempts
            .range((subject_id.to_owned(), 0)..=(subject_id.to_owned(), u32::MAX))
            .map(|((_, attempt), _)| *attempt)
            .max()
    }
}

/// 校验某个 worker 是否可以按已登记事实开始这次尝试。
///
/// 【调用者】真正的 dispatch/queue 路径在调用 effect 之前必须先用本函数确认 lease owner、
/// 时钟未回退、lease 未过期且未越过 deadline。
/// 【输入】`attempt` 是账本中的事实；`worker_id` 是准备开始的 holder；`now_ms` 是当前时钟快照。
/// 【输出】全部匹配返回 `Ok(())`。
/// 【失败】`automation_retry_attempt_invalid`（事实结构坏）、
/// `automation_retry_lease_owner_mismatch`（不是租约持有人）、`automation_retry_clock_rollback`
/// （早于登记时间）、`automation_retry_deadline_exceeded`、`automation_retry_lease_expired`。
/// 【副作用】无；不续租、不改状态、不执行 effect。
pub fn validate_retry_attempt_start(
    attempt: &AutomationRetryAttempt,
    worker_id: &str,
    now_ms: u64,
) -> Result<(), &'static str> {
    attempt.validate()?;
    if !valid_text(worker_id) || worker_id != attempt.lease_owner {
        return Err("automation_retry_lease_owner_mismatch");
    }
    if now_ms < attempt.created_at_ms {
        return Err("automation_retry_clock_rollback");
    }
    if now_ms >= attempt.deadline_ms {
        return Err("automation_retry_deadline_exceeded");
    }
    if now_ms >= attempt.lease_expires_at_ms {
        return Err("automation_retry_lease_expired");
    }
    Ok(())
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
