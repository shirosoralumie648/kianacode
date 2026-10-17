//! H05：模型停止原因、恢复 disposition 与 retry class 的 wire 投影。
//!
//! 本模块是 H05 卡片「停止/错误/CLI/wire 映射表」的收敛点：`ModelStopReason`、
//! `ModelRecoveryDisposition`、`ModelRetryClass` 在这里映射为稳定的公开失败码、
//! CLI 退出码与 HTTP 状态码。它只做分类，不执行重试、不授权新尝试，也不替代
//! `EventLog`/`Receipt` 事实。策略位（`cli_exit`/`http_status`/`retryable`）全部来自
//! 既有 `CapabilityErrorCode::from_reason(公开码).policy()`，本模块不另建第二张策略表；
//! 未登记的模型码保守落到 `ExecutionFailed`，任何类别都不会自动重试。
//!
//! 拒绝优先（deny-first）：
//! - 成功 outcome（`end_turn`/`tool_use`、无错误码、无恢复声明）不产生 failure surface；
//! - 未知停止原因落到 `model_stop_reason_unknown`（`UnknownStop`，仍是失败）；
//! - 校验失败的 outcome、未识别的错误码和矛盾组合落到
//!   [`MODEL_FAILURE_UNCLASSIFIED_CODE`]，并显式给出 `Terminal` + `Never`，绝不当成功；
//! - 未接线的 repair（Format/Tool/Context）保留 repair disposition 与
//!   `*_repair_unavailable` 码，不隐式重试、不宣称已修复；
//! - transport retry 保留自身 typed disposition/retry class，但 `retryable=false`：
//!   是否真正重试仍由 runner 预算化路径决定。
//!
//! 本模块不引入第二套执行路径：所有输入都来自既有 domain 类型或响应错误码。

use kiana_domain::{
    CapabilityErrorCode, ExecutionStatus, ModelOutcome, ModelRecoveryDisposition, ModelRetryClass,
    ModelSideEffectState, ModelStopReason,
};
use serde::{Deserialize, Serialize};

use crate::ResponseEnvelope;

/// 模型失败 surface 的 schema 名。
pub const MODEL_FAILURE_SCHEMA: &str = "kiana.model-failure.v1";

/// 未知或未分类模型失败的通用码；任何无法确认的输入都必须落到这里。
pub const MODEL_FAILURE_UNCLASSIFIED_CODE: &str = "model_failure_unclassified";

/// H05 停止/恢复分类的闭合集合。
///
/// 每个变体的 `snake_case` 名是稳定 wire 值；`stable_code()` 仅对拥有公开错误码的
/// 分类返回 `Some`。`TransportRetry` 与 `Unclassified` 分别保留源错误码与通用码，
/// 因此没有固定的自有码。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFailureClass {
    /// 响应被 length/max_tokens 截断。
    Truncated,
    /// 模型拒绝回答（refusal/content filter）。
    Refused,
    /// provider 要求显式 continue 后才能继续的 pause 停止。
    Paused,
    /// 传输层未完整关闭的响应（incomplete）。
    TransportIncomplete,
    /// 响应带无法识别的停止原因，按失败处理。
    UnknownStop,
    /// structured-output 内容校验失败；自动修复路径尚未接线。
    FormatRepair,
    /// 已收到响应但工具参数 JSON/shape/schema 不合格；自动修复路径尚未接线。
    ToolRepair,
    /// provider 明确给出 context 超限信号；压缩/修复路径尚未接线。
    ContextRepair,
    /// 受预算约束的传输重试分类；wire 层不授权自动重试。
    TransportRetry,
    /// 校验失败、未知码或矛盾组合的兜底分类。
    Unclassified,
}

impl ModelFailureClass {
    /// 全部分类；fixture 用它保证每个自有码都能反向解析。
    pub const ALL: [Self; 10] = [
        Self::Truncated,
        Self::Refused,
        Self::Paused,
        Self::TransportIncomplete,
        Self::UnknownStop,
        Self::FormatRepair,
        Self::ToolRepair,
        Self::ContextRepair,
        Self::TransportRetry,
        Self::Unclassified,
    ];

    /// 本分类自有的稳定公开码；`None` 表示分类携带源错误码或通用兜底码。
    pub const fn stable_code(self) -> Option<&'static str> {
        match self {
            Self::Truncated => Some("model_output_truncated"),
            Self::Refused => Some("model_refused"),
            Self::Paused => Some("model_pause_requires_explicit_continue"),
            Self::TransportIncomplete => Some("model_transport_incomplete"),
            Self::UnknownStop => Some("model_stop_reason_unknown"),
            Self::FormatRepair => Some("model_format_repair_unavailable"),
            Self::ToolRepair => Some("model_tool_repair_unavailable"),
            Self::ContextRepair => Some("model_context_repair_unavailable"),
            Self::TransportRetry | Self::Unclassified => None,
        }
    }

    /// 归一化停止原因到失败分类；`end_turn`/`tool_use` 是成功停止，不产生分类。
    pub const fn from_stop_reason(reason: ModelStopReason) -> Option<Self> {
        match reason {
            ModelStopReason::Length => Some(Self::Truncated),
            ModelStopReason::Refusal => Some(Self::Refused),
            ModelStopReason::Pause => Some(Self::Paused),
            ModelStopReason::Incomplete => Some(Self::TransportIncomplete),
            ModelStopReason::Unknown => Some(Self::UnknownStop),
            // 正常结束不是失败；调用方必须在成功路径上跳过 surface。
            ModelStopReason::EndTurn | ModelStopReason::ToolUse => None,
        }
    }

    /// 映射已知的模型失败错误码；未识别的码返回 `None`，由调用方决定兜底。
    ///
    /// 解析规则与 `CapabilityErrorCode::from_reason` 一致：只剥离受控的
    /// `port_*:` 包装前缀，再取第一个冒号前的稳定码，不做任意子串搜索。
    pub fn from_code(reason: &str) -> Option<Self> {
        match normalized_failure_code(reason) {
            "model_output_truncated" => Some(Self::Truncated),
            "model_refused" => Some(Self::Refused),
            "model_pause_requires_explicit_continue" => Some(Self::Paused),
            "model_transport_incomplete" => Some(Self::TransportIncomplete),
            "model_stop_reason_unknown" => Some(Self::UnknownStop),
            "model_format_repair_unavailable" => Some(Self::FormatRepair),
            "model_tool_repair_unavailable" => Some(Self::ToolRepair),
            "model_context_repair_unavailable" => Some(Self::ContextRepair),
            MODEL_FAILURE_UNCLASSIFIED_CODE => Some(Self::Unclassified),
            _ => None,
        }
    }

    /// repair 分派只认三种明确 disposition；`TransportRetry`/`Terminal` 不属于 repair。
    pub const fn from_repair_disposition(disposition: ModelRecoveryDisposition) -> Option<Self> {
        match disposition {
            ModelRecoveryDisposition::FormatRepair => Some(Self::FormatRepair),
            ModelRecoveryDisposition::ToolRepair => Some(Self::ToolRepair),
            ModelRecoveryDisposition::ContextRepair => Some(Self::ContextRepair),
            ModelRecoveryDisposition::TransportRetry | ModelRecoveryDisposition::Terminal => None,
        }
    }

    /// 本分类显式携带的恢复 disposition。
    pub const fn disposition(self) -> ModelRecoveryDisposition {
        match self {
            Self::FormatRepair => ModelRecoveryDisposition::FormatRepair,
            Self::ToolRepair => ModelRecoveryDisposition::ToolRepair,
            Self::ContextRepair => ModelRecoveryDisposition::ContextRepair,
            Self::TransportRetry => ModelRecoveryDisposition::TransportRetry,
            Self::Truncated
            | Self::Refused
            | Self::Paused
            | Self::TransportIncomplete
            | Self::UnknownStop
            | Self::Unclassified => ModelRecoveryDisposition::Terminal,
        }
    }

    /// 本分类归一化后的停止原因；repair/transport/unknown 没有可声明的停止。
    pub const fn stop_reason(self) -> ModelStopReason {
        match self {
            Self::Truncated => ModelStopReason::Length,
            Self::Refused => ModelStopReason::Refusal,
            Self::Paused => ModelStopReason::Pause,
            Self::TransportIncomplete => ModelStopReason::Incomplete,
            Self::UnknownStop
            | Self::FormatRepair
            | Self::ToolRepair
            | Self::ContextRepair
            | Self::TransportRetry
            | Self::Unclassified => ModelStopReason::Unknown,
        }
    }
}

/// 一次模型失败的 wire/CLI 分类结果。
///
/// 该 DTO 只保存分类与策略投影，不包含 provider body/header、prompt 或原始错误文本；
/// `request_sent`/`side_effect_state` 为 `None` 表示该 surface 不携带请求证据
/// （由错误码字符串构造时），不能反推副作用已发生或未发生。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFailureSurface {
    /// surface 合同名。
    pub schema: String,
    /// 闭合失败分类。
    pub failure: ModelFailureClass,
    /// 稳定的公开失败码；repair/stop 分类使用自有码，transport 使用源错误码。
    pub wire_code: String,
    /// 分类归一化后的停止原因。
    pub stop_reason: ModelStopReason,
    /// 显式恢复 disposition；未接线的 repair 仍保留 repair 语义。
    pub disposition: ModelRecoveryDisposition,
    /// 显式重试类别；分类自身不授权自动重试。
    pub retry_class: ModelRetryClass,
    /// 是否已确认发出模型请求；`None` 表示该 surface 没有请求证据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_sent: Option<bool>,
    /// 已观察到的副作用状态；`None` 表示该 surface 没有副作用证据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_effect_state: Option<ModelSideEffectState>,
    /// CLI/HTTP 策略类别，来自既有 `CapabilityErrorCode` 表。
    pub capability_error: CapabilityErrorCode,
    /// `capability_error` 策略给出的 CLI 退出码（数据，不是进程动作）。
    pub cli_exit: u8,
    /// `capability_error` 策略给出的 HTTP 状态码（数据，不是网络动作）。
    pub http_status: u16,
    /// 是否允许自动重试；当前所有映射均为 `false`。
    pub retryable: bool,
    /// 是否要求新的授权决定。
    pub requires_new_authorization: bool,
    /// 是否要求先对账未知副作用。
    pub requires_reconciliation: bool,
}

impl ModelFailureSurface {
    fn new(
        failure: ModelFailureClass,
        wire_code: String,
        retry_class: ModelRetryClass,
        request_sent: Option<bool>,
        side_effect_state: Option<ModelSideEffectState>,
    ) -> Self {
        // 策略单一来源于既有错误表；没有自有码的 transport/unknown 码保守落到
        // `ExecutionFailed`，绝不会因为 surface 的存在而获得重试或更宽权限。
        let capability_error = CapabilityErrorCode::from_reason(&wire_code);
        let policy = capability_error.policy();
        Self {
            schema: MODEL_FAILURE_SCHEMA.to_owned(),
            failure,
            wire_code,
            stop_reason: failure.stop_reason(),
            disposition: failure.disposition(),
            retry_class,
            request_sent,
            side_effect_state,
            capability_error,
            cli_exit: policy.cli_exit,
            http_status: policy.http_status,
            retryable: policy.retryable,
            requires_new_authorization: policy.requires_new_authorization,
            requires_reconciliation: policy.requires_reconciliation,
        }
    }

    /// 由自有码分类构造 surface；stop/repair 分类固定 `Terminal`/`Never`（repair 除外）。
    fn classified(
        failure: ModelFailureClass,
        request_sent: Option<bool>,
        side_effect_state: Option<ModelSideEffectState>,
    ) -> Self {
        Self::new(
            failure,
            failure
                .stable_code()
                .unwrap_or(MODEL_FAILURE_UNCLASSIFIED_CODE)
                .to_owned(),
            ModelRetryClass::Never,
            request_sent,
            side_effect_state,
        )
    }

    /// 显式兜底：未知分类固定 `Terminal` + `Never` + 通用码，绝不授权成功或重试。
    pub fn unclassified() -> Self {
        Self::new(
            ModelFailureClass::Unclassified,
            MODEL_FAILURE_UNCLASSIFIED_CODE.to_owned(),
            ModelRetryClass::Never,
            None,
            None,
        )
    }

    /// 仅识别已知模型失败错误码；未识别返回 `None`，让调用方保留通用错误路径。
    pub fn try_from_code(code: &str) -> Option<Self> {
        let failure = ModelFailureClass::from_code(code)?;
        Some(Self::classified(failure, None, None))
    }

    /// 由错误码构造 surface；未识别码一律落到 [`Self::unclassified`]（deny-first）。
    pub fn from_code(code: &str) -> Self {
        Self::try_from_code(code).unwrap_or_else(Self::unclassified)
    }

    /// 由 bounded `ModelOutcome` 投影失败 surface。
    ///
    /// 返回 `None` 仅代表「校验通过的成功 outcome」（`end_turn`/`tool_use`，无错误码、
    /// 无恢复声明）；任何校验失败、未知码或矛盾组合都返回 `Some(unclassified)`。
    /// repair disposition 优先于源错误码：自动修复路径未接线时，公开码是
    /// `*_repair_unavailable`，而不是内部 producer 码。
    pub fn from_outcome(outcome: &ModelOutcome) -> Option<Self> {
        if outcome.validate().is_err() {
            return Some(Self::unclassified());
        }
        let has_error = outcome
            .error_code
            .as_deref()
            .is_some_and(|code| !code.trim().is_empty());
        let stop_claim = matches!(
            outcome.stop_reason,
            ModelStopReason::Length
                | ModelStopReason::Refusal
                | ModelStopReason::Pause
                | ModelStopReason::Incomplete
        );
        // 校验通过、无错误码、无恢复声明且不是 unknown stop 的 outcome 是成功路径。
        if outcome.recovery_disposition == ModelRecoveryDisposition::Terminal
            && !has_error
            && !stop_claim
            && outcome.stop_reason != ModelStopReason::Unknown
        {
            return None;
        }
        if let Some(repair) =
            ModelFailureClass::from_repair_disposition(outcome.recovery_disposition)
        {
            return Some(Self::classified(
                repair,
                Some(outcome.request_sent),
                Some(outcome.side_effect_state),
            ));
        }
        if outcome.recovery_disposition == ModelRecoveryDisposition::TransportRetry {
            // 可重试传输尝试与「已收到响应的停止失败」互相矛盾，必须 fail closed。
            if stop_claim {
                return Some(Self::unclassified());
            }
            let wire_code = outcome
                .error_code
                .clone()
                .filter(|code| !code.trim().is_empty())
                .unwrap_or_else(|| MODEL_FAILURE_UNCLASSIFIED_CODE.to_owned());
            return Some(Self::new(
                ModelFailureClass::TransportRetry,
                wire_code,
                outcome.retry_class,
                Some(outcome.request_sent),
                Some(outcome.side_effect_state),
            ));
        }
        if let Some(code) = outcome
            .error_code
            .as_deref()
            .filter(|code| !code.trim().is_empty())
        {
            if let Some(class) = ModelFailureClass::from_code(code) {
                return Some(Self::classified(
                    class,
                    Some(outcome.request_sent),
                    Some(outcome.side_effect_state),
                ));
            }
        }
        if let Some(class) = ModelFailureClass::from_stop_reason(outcome.stop_reason) {
            return Some(Self::classified(
                class,
                Some(outcome.request_sent),
                Some(outcome.side_effect_state),
            ));
        }
        Some(Self::unclassified())
    }

    /// 校验 surface 自洽：schema、码、分类、disposition 与策略投影必须一致。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_FAILURE_SCHEMA {
            return Err("model_failure_schema_invalid".to_owned());
        }
        if self.wire_code.is_empty()
            || self.wire_code.trim() != self.wire_code
            || self.wire_code.len() > 128
            || self.wire_code.contains('\0')
            || self.wire_code.contains('\n')
        {
            return Err("model_failure_code_invalid".to_owned());
        }
        if let Some(owned) = self.failure.stable_code() {
            if self.wire_code != owned || self.disposition != self.failure.disposition() {
                return Err("model_failure_classification_mismatch".to_owned());
            }
        }
        match self.failure {
            ModelFailureClass::Unclassified => {
                if self.wire_code != MODEL_FAILURE_UNCLASSIFIED_CODE
                    || self.disposition != ModelRecoveryDisposition::Terminal
                    || self.retry_class != ModelRetryClass::Never
                {
                    return Err("model_failure_unclassified_invalid".to_owned());
                }
            }
            ModelFailureClass::TransportRetry => {
                if self.disposition != ModelRecoveryDisposition::TransportRetry
                    || self.retry_class == ModelRetryClass::Never
                {
                    return Err("model_failure_transport_retry_invalid".to_owned());
                }
            }
            // stop 与 repair 分类都是确定性结果，不携带重试类别。
            _ => {
                if self.retry_class != ModelRetryClass::Never {
                    return Err("model_failure_classification_mismatch".to_owned());
                }
            }
        }
        let policy = CapabilityErrorCode::from_reason(&self.wire_code).policy();
        if self.cli_exit != policy.cli_exit
            || self.http_status != policy.http_status
            || self.retryable != policy.retryable
            || self.requires_new_authorization != policy.requires_new_authorization
            || self.requires_reconciliation != policy.requires_reconciliation
        {
            return Err("model_failure_policy_mismatch".to_owned());
        }
        Ok(())
    }
}

impl ResponseEnvelope {
    /// 把 `Failed` 响应的错误码投影为 H05 模型失败 surface（additive wire 方法）。
    ///
    /// 只有 `Failed` 状态且错误码是已知模型停止/修复码时返回 `Some`；其他状态保留既有
    /// [`ResponseEnvelope::failure_code()`] 通用分类路径。生命周期保持权威：
    /// `ResultUnknown`/`Cancelled`/`AwaitingApproval` 不会被诊断文本改写成模型失败，
    /// 本方法也不会让任何响应变成成功。
    pub fn model_failure(&self) -> Option<ModelFailureSurface> {
        if self.status != ExecutionStatus::Failed {
            return None;
        }
        ModelFailureSurface::try_from_code(self.error.as_deref()?)
    }
}

/// 与 `CapabilityErrorCode::from_reason` 相同的受控前缀归一化，不做子串猜测。
fn normalized_failure_code(reason: &str) -> &str {
    let mut reason = reason.trim();
    loop {
        let Some((prefix, detail)) = reason.split_once(':') else {
            break;
        };
        if matches!(prefix, "port_failed" | "port_conflict" | "port_unavailable") {
            reason = detail.trim();
        } else {
            break;
        }
    }
    reason.split(':').next().unwrap_or_default()
}
