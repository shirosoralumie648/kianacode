//! BQ-22 ControlPlane admission for provider invoice imports and reconciliation facts.
//!
//! # 职责与边界
//!
//! ```text
//! adapter（收据读取，不在此文件）
//!   -> InvoiceAuthenticationEvidence（服务端验证结果）
//!   -> 【当前文件】import 准入：形状 + 认证绑定 + 账本追加（只追加）
//!   -> compare_provider_invoice（服务端派生状态，拒绝 wire 自报的 Matched）
//!   -> ProviderInvoiceObservation（measured / correction / review）
//!   -> 【当前文件】admit_invoice_correction：handoff + BQ-14 批准命令 → CostCorrection
//! ```
//!
//! 这里的函数只做内存校验与事件构造：不 append EventStore、不消费审批、不调用 provider、
//! 不覆盖已提交账本。`Ok` 只表示“当前输入可以进入正常事件提交阶段”，不表示账单已支付、
//! 已 durable 或已经产生财务影响。
//!
//! `authenticated` 布尔值、wire 自报的比较状态、模型文本都不是授权：认证必须由服务端证据
//! 绑定，比较状态由 `compare_provider_invoice` 从已提交期望重算，任何漂移在事实成形前拒绝。

use crate::{CostCorrectionAdmission, CostCorrectionAdmissionError};
use kiana_domain::{
    compare_provider_invoice, validate_invoice_comparison as validate_domain_comparison,
    validate_provider_invoice as validate_domain_invoice, CostCorrection, CostCorrectionCommand,
    CostLedgerEntry, InvoiceAuthenticationEvidence, InvoiceComparison, InvoiceImportOutcome,
    InvoiceLedgerExpectation, InvoiceObservationOutcome, ProviderInvoiceCorrectionHandoff,
    ProviderInvoiceImport, ProviderInvoiceLedger, ProviderInvoiceObservation, RuntimeEvent,
    INVOICE_IMPORT_EVENT, INVOICE_OBSERVATION_EVENT,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
/// 账单导入准入失败的稳定错误分类。
///
/// 分类区分了导入形状、服务端认证、比较漂移、账本围栏与事件编码失败，便于上层给出可理解
/// 的拒绝原因；本模块不把所有拒绝压成通用 I/O 错误，也不自动重试或改用备用金额。
pub enum ProviderInvoiceAdmissionError {
    /// 导入事实自身形状/摘要非法。
    #[error("provider_invoice_import_invalid:{0}")]
    ImportInvalid(String),
    /// 服务端认证证据缺失、漂移、过期或世代不符。
    #[error("provider_invoice_authentication_invalid:{0}")]
    AuthenticationInvalid(String),
    /// 服务端比较结果与 wire 自报值不一致或比较事实自身非法。
    #[error("provider_invoice_comparison_invalid:{0}")]
    ComparisonInvalid(String),
    /// 只追加账本围栏：重复 invoice、游标回退、未知导入或观测重放冲突。
    #[error("provider_invoice_ledger_invalid:{0}")]
    LedgerInvalid(String),
    /// 观测事实非法，或编码为 RuntimeEvent/流元数据失败。
    #[error("provider_invoice_event_invalid:{0}")]
    EventInvalid(String),
    /// 修正闭环：handoff 与批准后的 BQ-14 命令不能成立（未批准、绑定漂移、期望非权威）。
    #[error("provider_invoice_correction_invalid:{0}")]
    CorrectionInvalid(String),
}

/// 把 Domain 拒绝码映射到 Core 分类；分类只做归类，不改写 Domain 规则。
fn classify_import(code: String) -> ProviderInvoiceAdmissionError {
    if code.starts_with("provider_invoice_authentication") {
        ProviderInvoiceAdmissionError::AuthenticationInvalid(code)
    } else {
        ProviderInvoiceAdmissionError::ImportInvalid(code)
    }
}

fn classify_comparison(code: String) -> ProviderInvoiceAdmissionError {
    ProviderInvoiceAdmissionError::ComparisonInvalid(code)
}

fn classify_ledger(code: String) -> ProviderInvoiceAdmissionError {
    ProviderInvoiceAdmissionError::LedgerInvalid(code)
}

fn classify_observation(code: String) -> ProviderInvoiceAdmissionError {
    ProviderInvoiceAdmissionError::EventInvalid(code)
}

/// BQ-22 账单导入准入的无状态门面。
///
/// 与 BQ-19/BQ-21 的准入门面一致：它不持有 EventStore、不落账、不派发，只让相同的 Domain
/// 规则在 CI 夹具与正常提交路径之间复用，避免入口或 adapter 自己判断“这张账单算不算数”。
pub struct ProviderInvoiceAdmission;

impl ProviderInvoiceAdmission {
    /// 校验一份导入是否可以进入提交阶段。
    ///
    /// 【检查顺序】
    /// 1. 导入事实形状与自证摘要（Domain `validate`）；
    /// 2. 服务端认证证据的形状与自证摘要；
    /// 3. 证据与导入逐字段绑定（账户/invoice/账期/收据摘要）；
    /// 4. 授权世代一致（超前/落后都拒绝）；
    /// 5. 验证时刻非零、不来自未来、不超过最大有效期。
    ///
    /// 【失败】任一项不通过都在事实成形前拒绝；不会把 `authenticated=true` 当成证据，
    /// 也不会修改导入、选择备用证据或重试。
    pub fn authorize_import(
        import: &ProviderInvoiceImport,
        evidence: &InvoiceAuthenticationEvidence,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), ProviderInvoiceAdmissionError> {
        import
            .validate_against_authentication(evidence, now_unix_ms, authority_epoch)
            .map_err(|code| classify_import(code.to_owned()))
    }

    /// 用已提交的账本期望重算比较，并要求 wire 自报值逐字段一致。
    ///
    /// wire 只提交事实字段，状态（Matched/CorrectionRequired/ReviewRequired）与差异码由服务端
    /// 派生；重算结果与自报值（含摘要）不同即 `provider_invoice_comparison_reported_drift`。
    /// 因此调用方无法把 period/model/usage/amount 不匹配的账单标成 Matched，也无法省略差异码。
    pub fn authorize_comparison(
        import: &ProviderInvoiceImport,
        expected: &InvoiceLedgerExpectation,
        correction_ref: Option<&str>,
        reported: &InvoiceComparison,
    ) -> Result<InvoiceComparison, ProviderInvoiceAdmissionError> {
        validate_domain_comparison(reported)
            .map_err(|code| classify_comparison(code.to_owned()))?;
        let recomputed = compare_provider_invoice(import, expected, correction_ref)
            .map_err(classify_comparison)?;
        if recomputed != *reported {
            return Err(ProviderInvoiceAdmissionError::ComparisonInvalid(
                "provider_invoice_comparison_reported_drift".to_owned(),
            ));
        }
        Ok(recomputed)
    }

    /// 重算比较并派生观测事实（measured / correction / review）。
    ///
    /// `review_ref` 只在服务端重算结果是 `ReviewRequired` 时允许出现；其它状态带了复核引用
    /// 会在 Domain 构造时拒绝，不存在“未知顺手当成实测”的路径。
    pub fn authorize_observation(
        import: &ProviderInvoiceImport,
        expected: &InvoiceLedgerExpectation,
        correction_ref: Option<&str>,
        review_ref: Option<&str>,
        reported: &InvoiceComparison,
    ) -> Result<ProviderInvoiceObservation, ProviderInvoiceAdmissionError> {
        let verified = Self::authorize_comparison(import, expected, correction_ref, reported)?;
        ProviderInvoiceObservation::from_import_comparison(import, &verified, review_ref)
            .map_err(classify_observation)
    }

    /// 只追加地把导入写入调用方账本；重复/回退由 Domain 账本围栏拒绝。
    pub fn record_import(
        ledger: &mut ProviderInvoiceLedger,
        import: &ProviderInvoiceImport,
    ) -> Result<InvoiceImportOutcome, ProviderInvoiceAdmissionError> {
        ledger.append(import).map_err(classify_ledger)
    }

    /// 只追加地记录观测；一张 invoice 只能有一条观测事实。
    pub fn record_observation(
        ledger: &mut ProviderInvoiceLedger,
        observation: &ProviderInvoiceObservation,
    ) -> Result<InvoiceObservationOutcome, ProviderInvoiceAdmissionError> {
        ledger
            .record_observation(observation)
            .map_err(classify_ledger)
    }

    /// 构造导入事件。事件不在此处追加；调用方仍走既有 ControlPlane/EventStore 事务。
    pub fn import_event(
        request_id: kiana_domain::RequestId,
        sequence: u64,
        stream_version: u64,
        import: &ProviderInvoiceImport,
    ) -> Result<RuntimeEvent, ProviderInvoiceAdmissionError> {
        import
            .to_runtime_event(request_id, sequence, stream_version)
            .map_err(classify_import)
    }

    /// 构造观测事件；`stream_version` 必须非零且沿同一 `provider_invoice` 流单调。
    pub fn observation_event(
        request_id: kiana_domain::RequestId,
        sequence: u64,
        stream_version: u64,
        observation: &ProviderInvoiceObservation,
    ) -> Result<RuntimeEvent, ProviderInvoiceAdmissionError> {
        observation
            .to_runtime_event(request_id, sequence, stream_version)
            .map_err(classify_observation)
    }

    /// 供只读检查器/测试查看事件 kind，不执行任何写入。
    pub const fn import_event_kind() -> &'static str {
        INVOICE_IMPORT_EVENT
    }

    /// 供只读检查器/测试查看事件 kind，不执行任何写入。
    pub const fn observation_event_kind() -> &'static str {
        INVOICE_OBSERVATION_EVENT
    }

    /// BQ-22 correction loop: hand an invoice discrepancy to the BQ-14 approval path.
    ///
    /// This is the missing caller the domain `ProviderInvoiceCorrectionHandoff` never had.  It
    /// does *not* create a second correction ledger and it does not approve anything: it proves
    /// the handoff's target digest is the committed ledger entry's digest, that the command is
    /// still the unapproved draft the handoff binds, and then delegates the approved command to
    /// [`CostCorrectionAdmission::authorize`].  An unapproved draft is refused with
    /// `cost_correction_approval_required`; an approved but drifted command is refused by the
    /// same admission helper's binding/digest checks.
    ///
    /// `Ok(CostCorrection)` is a value the caller must still append through the existing
    /// ControlPlane/EventStore transaction; no event is written here.
    pub fn admit_invoice_correction(
        handoff: &ProviderInvoiceCorrectionHandoff,
        target: &CostLedgerEntry,
        command: &CostCorrectionCommand,
    ) -> Result<CostCorrection, ProviderInvoiceAdmissionError> {
        handoff
            .validate()
            .map_err(|code| ProviderInvoiceAdmissionError::CorrectionInvalid(code.to_owned()))?;
        target
            .validate()
            .map_err(ProviderInvoiceAdmissionError::CorrectionInvalid)?;
        if target.entry_digest != handoff.ledger_entry_digest {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "provider_invoice_handoff_target_digest_mismatch".to_owned(),
            ));
        }
        // The handoff only binds the *draft*.  Refuse an unapproved command before asking BQ-14,
        // so an operator sees the stable invoice-side code rather than a generic command error.
        if command.approval.is_none() {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "cost_correction_approval_required".to_owned(),
            ));
        }
        // `validate_draft_command` rejects approved commands by design (it is the handoff's
        // draft-only binding), so compare the fields it would have compared directly.
        if command.target_entry_digest != handoff.ledger_entry_digest {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "provider_invoice_handoff_command_binding_mismatch".to_owned(),
            ));
        }
        if command.correction_id.to_string() != handoff.correction_ref {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "provider_invoice_handoff_command_reference_mismatch".to_owned(),
            ));
        }
        if command.delta_estimated.is_some()
            || command.delta_measured.as_ref() != Some(&handoff.delta_measured)
        {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "provider_invoice_handoff_command_delta_mismatch".to_owned(),
            ));
        }
        if handoff
            .evidence_refs
            .iter()
            .any(|reference| !command.evidence_refs.contains(reference))
        {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "provider_invoice_handoff_command_evidence_missing".to_owned(),
            ));
        }
        if handoff
            .discrepancy_codes
            .iter()
            .any(|code| !command.reason.contains(code.as_str()))
        {
            return Err(ProviderInvoiceAdmissionError::CorrectionInvalid(
                "provider_invoice_handoff_command_reason_missing".to_owned(),
            ));
        }
        CostCorrectionAdmission::authorize(target, command).map_err(|error| match error {
            CostCorrectionAdmissionError::TargetInvalid(code) => {
                ProviderInvoiceAdmissionError::CorrectionInvalid(code)
            }
            CostCorrectionAdmissionError::CommandInvalid(code) => {
                ProviderInvoiceAdmissionError::CorrectionInvalid(code)
            }
            CostCorrectionAdmissionError::EventInvalid(code) => {
                ProviderInvoiceAdmissionError::CorrectionInvalid(code)
            }
        })
    }
}

pub fn validate_provider_invoice(import: &ProviderInvoiceImport) -> Result<(), &'static str> {
    validate_domain_invoice(import)
}

pub fn reject_duplicate_provider_invoices(
    imports: &[ProviderInvoiceImport],
) -> Result<(), &'static str> {
    kiana_domain::reject_duplicate_provider_invoices(imports)
}

pub fn validate_invoice_comparison(comparison: &InvoiceComparison) -> Result<(), &'static str> {
    validate_domain_comparison(comparison)
}

impl super::ControlPlane {
    /// 以 `ControlPlane` 方法形式暴露 BQ-22 导入准入。
    ///
    /// 这是授权边界，不是财务写入入口：它只校验认证绑定与账本围栏，返回后调用方仍需在正常
    /// 事件事务中构造并提交 `import_event`；这里不创建第二条写入路径。
    pub fn admit_provider_invoice_import(
        &self,
        ledger: &mut ProviderInvoiceLedger,
        import: &ProviderInvoiceImport,
        evidence: &InvoiceAuthenticationEvidence,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<InvoiceImportOutcome, ProviderInvoiceAdmissionError> {
        ProviderInvoiceAdmission::authorize_import(import, evidence, now_unix_ms, authority_epoch)?;
        ProviderInvoiceAdmission::record_import(ledger, import)
    }

    /// 以 `ControlPlane` 方法形式暴露服务端比较准入；返回重算后的比较事实。
    pub fn admit_provider_invoice_comparison(
        &self,
        import: &ProviderInvoiceImport,
        expected: &InvoiceLedgerExpectation,
        correction_ref: Option<&str>,
        reported: &InvoiceComparison,
    ) -> Result<InvoiceComparison, ProviderInvoiceAdmissionError> {
        ProviderInvoiceAdmission::authorize_comparison(import, expected, correction_ref, reported)
    }

    /// 记录一条已准入的观测事实；重放/覆盖冲突由 Domain 账本拒绝。
    pub fn admit_provider_invoice_observation(
        &self,
        ledger: &mut ProviderInvoiceLedger,
        observation: &ProviderInvoiceObservation,
    ) -> Result<InvoiceObservationOutcome, ProviderInvoiceAdmissionError> {
        ProviderInvoiceAdmission::record_observation(ledger, observation)
    }

    /// 以 `ControlPlane` 方法形式暴露 BQ-22 → BQ-14 修正闭环。
    ///
    /// 与 `admit_cost_correction` 相同：只返回可追加的 `CostCorrection` 值，不 append、不消费
    /// 审批；未批准 draft 在发票侧以 `cost_correction_approval_required` 拒绝。
    pub fn admit_provider_invoice_correction(
        &self,
        handoff: &ProviderInvoiceCorrectionHandoff,
        target: &CostLedgerEntry,
        command: &CostCorrectionCommand,
    ) -> Result<CostCorrection, ProviderInvoiceAdmissionError> {
        ProviderInvoiceAdmission::admit_invoice_correction(handoff, target, command)
    }
}
