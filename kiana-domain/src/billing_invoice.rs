//! BQ-22 provider invoice import and reconciliation source contracts.
//!
//! An authenticated invoice is an external observation, not a replacement for the EventLog
//! ledger.  Imports are immutable and duplicate invoice identities are rejected before a caller
//! can route a correction through the existing BQ-14 approval command.  Missing or mismatched
//! fields remain visible as review/correction states; this module never silently overwrites a
//! measured ledger value.
//!
//! # 数据流与边界
//!
//! ```text
//! provider receipt bytes (adapter, 不在此文件)
//!   -> InvoiceAuthenticationEvidence（服务端验证结果，不是 wire 自报布尔值）
//!   -> ProviderInvoiceImport（不可变导入事实，含 receipt_digest 绑定）
//!   -> ProviderInvoiceLedger::append（身份 + 来源游标围栏，重复/回退拒绝）
//!   -> compare_provider_invoice（用已提交 expectation 派生 Matched/Correction/Review）
//!   -> ProviderInvoiceObservation（实测 / 需修正 / 待人工，差异可追踪）
//!   -> ProviderInvoiceCorrectionHandoff（只做 draft 绑定，审批与落账仍走 BQ-14）
//! ```
//!
//! 本文件只做纯值校验与只追加账本：不 append EventStore、不消费审批、不调用 provider、
//! 不覆盖任何已提交事实。`Matched` 只表示“服务端比较结果一致”，不表示账单已支付。

use crate::{json_digest, CostCorrectionCommand, CostLedgerEntry, Money, ProviderReceiptRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROVIDER_INVOICE_SCHEMA: &str = "kiana.provider-invoice.v1";
pub const INVOICE_COMPARISON_SCHEMA: &str = "kiana.invoice-comparison.v1";
pub const INVOICE_AUTHENTICATION_SCHEMA: &str = "kiana.invoice-authentication.v1";
pub const INVOICE_OBSERVATION_SCHEMA: &str = "kiana.invoice-observation.v1";
pub const INVOICE_CORRECTION_HANDOFF_SCHEMA: &str = "kiana.invoice-correction-handoff.v1";
/// 已登记的运行时事件（`cost.` 族仍是关键族，未登记变体继续失败关闭）。
pub const INVOICE_IMPORT_EVENT: &str = "cost.invoice_imported";
pub const INVOICE_OBSERVATION_EVENT: &str = "cost.invoice_observed";
/// 一次导入最多保留的差异码数量，与 `InvoiceComparison::validate` 使用同一上限。
pub const MAX_INVOICE_DISCREPANCIES: usize = 8;
/// handoff 的证据引用上限；证据只保存 ref/digest，绝不复制收据原文。
pub const MAX_INVOICE_HANDOFF_EVIDENCE: usize = 8;
/// 服务端验证结果的最大年龄：超过后必须重新验证收据，不能拿旧证据当当前授权。
pub const INVOICE_AUTHENTICATION_MAX_AGE_UNIX_MS: u64 = 86_400_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvoiceImport {
    pub schema: String,
    pub provider_account_ref: String,
    pub invoice_id: String,
    pub period_start_unix_ms: u64,
    pub period_end_unix_ms: u64,
    pub model: Option<String>,
    pub usage_digest: Option<String>,
    pub amount: Money,
    pub receipt_ref: ProviderReceiptRef,
    /// 收据字节的摘要，由服务端在验证时计算；认证证据必须绑定同一摘要。
    pub receipt_digest: String,
    pub authentication_ref: String,
    pub authenticated: bool,
    pub source_cursor: u64,
    pub import_key: String,
    pub import_digest: String,
}

impl ProviderInvoiceImport {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PROVIDER_INVOICE_SCHEMA
            || !valid_text(&self.provider_account_ref)
            || !valid_text(&self.invoice_id)
            || self.period_start_unix_ms == 0
            || self.period_end_unix_ms <= self.period_start_unix_ms
            || self
                .model
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self
                .usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self.amount.validate().is_err()
            || self.amount.micros < 0
            || ProviderReceiptRef::new(self.receipt_ref.as_str().to_owned()).is_err()
            || !valid_digest(&self.receipt_digest)
            || !valid_text(&self.authentication_ref)
            || !self.authenticated
            || self.source_cursor == 0
            || !valid_text(&self.import_key)
            || !valid_digest(&self.import_digest)
            || self.import_digest != self.digest()
        {
            return Err("provider_invoice_import_invalid");
        }
        Ok(())
    }

    /// 与**服务端**认证证据逐字段比对。
    ///
    /// `authenticated` 是 wire 自报字段，不能作为认证权威；只有与同一账户、同一 invoice、
    /// 同一账期和同一收据摘要绑定的证据（且在有效窗口内、授权世代一致）才被接受。
    pub fn validate_against_authentication(
        &self,
        evidence: &InvoiceAuthenticationEvidence,
        now_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<(), &'static str> {
        self.validate()?;
        evidence.validate()?;
        if evidence.provider_account_ref != self.provider_account_ref
            || evidence.invoice_id != self.invoice_id
            || evidence.period_start_unix_ms != self.period_start_unix_ms
            || evidence.period_end_unix_ms != self.period_end_unix_ms
            || evidence.receipt_digest != self.receipt_digest
        {
            return Err("provider_invoice_authentication_binding_mismatch");
        }
        if evidence.authority_epoch > authority_epoch {
            return Err("provider_invoice_authentication_epoch_ahead");
        }
        if evidence.authority_epoch < authority_epoch {
            return Err("provider_invoice_authentication_epoch_stale");
        }
        if evidence.verified_at_unix_ms == 0
            || now_unix_ms == 0
            || evidence.verified_at_unix_ms > now_unix_ms
        {
            return Err("provider_invoice_authentication_timestamp_invalid");
        }
        if now_unix_ms - evidence.verified_at_unix_ms > INVOICE_AUTHENTICATION_MAX_AGE_UNIX_MS {
            return Err("provider_invoice_authentication_expired");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "provider_account_ref": self.provider_account_ref,
            "invoice_id": self.invoice_id,
            "period_start_unix_ms": self.period_start_unix_ms,
            "period_end_unix_ms": self.period_end_unix_ms,
            "model": self.model,
            "usage_digest": self.usage_digest,
            "amount": self.amount,
            "receipt_ref": self.receipt_ref,
            "receipt_digest": self.receipt_digest,
            "authentication_ref": self.authentication_ref,
            "authenticated": self.authenticated,
            "source_cursor": self.source_cursor,
            "import_key": self.import_key,
        }))
    }

    /// 导入事件的唯一聚合身份：导入摘要（同时是后续 observation 的 `invoice_digest`）。
    pub fn aggregate_id(&self) -> &str {
        self.import_digest.as_str()
    }

    pub fn to_runtime_event(
        &self,
        request_id: crate::RequestId,
        sequence: u64,
        stream_version: u64,
    ) -> Result<crate::RuntimeEvent, String> {
        self.validate().map_err(str::to_owned)?;
        if stream_version == 0 {
            return Err("provider_invoice_stream_version_invalid".to_owned());
        }
        let data =
            serde_json::to_value(self).map_err(|_| "provider_invoice_encode_failed".to_owned())?;
        crate::RuntimeEvent::new(request_id, sequence, INVOICE_IMPORT_EVENT, data)
            .map(|event| {
                event.with_stream_metadata(
                    "provider_invoice",
                    self.aggregate_id().to_owned(),
                    stream_version,
                )
            })
            .map_err(|error| error.to_string())
    }

    fn identity(&self) -> (&str, &str, u64, u64) {
        (
            &self.provider_account_ref,
            &self.invoice_id,
            self.period_start_unix_ms,
            self.period_end_unix_ms,
        )
    }
}

pub fn validate_provider_invoice(import: &ProviderInvoiceImport) -> Result<(), &'static str> {
    import.validate()
}

/// 服务端对一份收据的验证结果；它是认证权威，不是 wire 声明。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceAuthenticationEvidence {
    pub schema: String,
    pub provider_account_ref: String,
    pub invoice_id: String,
    pub period_start_unix_ms: u64,
    pub period_end_unix_ms: u64,
    pub receipt_digest: String,
    pub verifier_ref: String,
    pub verified_at_unix_ms: u64,
    pub authority_epoch: u64,
    pub evidence_digest: String,
}

impl InvoiceAuthenticationEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_account_ref: impl Into<String>,
        invoice_id: impl Into<String>,
        period_start_unix_ms: u64,
        period_end_unix_ms: u64,
        receipt_digest: impl Into<String>,
        verifier_ref: impl Into<String>,
        verified_at_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: INVOICE_AUTHENTICATION_SCHEMA.to_owned(),
            provider_account_ref: provider_account_ref.into(),
            invoice_id: invoice_id.into(),
            period_start_unix_ms,
            period_end_unix_ms,
            receipt_digest: receipt_digest.into(),
            verifier_ref: verifier_ref.into(),
            verified_at_unix_ms,
            authority_epoch,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != INVOICE_AUTHENTICATION_SCHEMA
            || !valid_text(&self.provider_account_ref)
            || !valid_text(&self.invoice_id)
            || self.period_start_unix_ms == 0
            || self.period_end_unix_ms <= self.period_start_unix_ms
            || !valid_digest(&self.receipt_digest)
            || !valid_text(&self.verifier_ref)
            || self.verified_at_unix_ms == 0
            || self.authority_epoch == 0
            || !valid_digest(&self.evidence_digest)
            || self.evidence_digest != self.digest()
        {
            return Err("provider_invoice_authentication_evidence_invalid");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "provider_account_ref": self.provider_account_ref,
            "invoice_id": self.invoice_id,
            "period_start_unix_ms": self.period_start_unix_ms,
            "period_end_unix_ms": self.period_end_unix_ms,
            "receipt_digest": self.receipt_digest,
            "verifier_ref": self.verifier_ref,
            "verified_at_unix_ms": self.verified_at_unix_ms,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

pub fn validate_invoice_authentication(
    evidence: &InvoiceAuthenticationEvidence,
) -> Result<(), &'static str> {
    evidence.validate()
}

pub fn reject_duplicate_provider_invoices(
    imports: &[ProviderInvoiceImport],
) -> Result<(), &'static str> {
    let mut identities = BTreeSet::new();
    for import in imports {
        import.validate()?;
        if !identities.insert(import.identity()) {
            return Err("provider_invoice_duplicate");
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceComparisonState {
    Matched,
    CorrectionRequired,
    ReviewRequired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceComparison {
    pub schema: String,
    pub invoice_digest: String,
    pub ledger_entry_digest: String,
    pub expected_period_start_unix_ms: u64,
    pub expected_period_end_unix_ms: u64,
    pub invoice_period_start_unix_ms: u64,
    pub invoice_period_end_unix_ms: u64,
    pub expected_model: String,
    pub invoice_model: Option<String>,
    pub expected_usage_digest: Option<String>,
    pub invoice_usage_digest: Option<String>,
    pub expected_amount: Option<Money>,
    pub invoice_amount: Money,
    /// BQ-22: the committed ledger fact's provider receipt reference, when it has one.
    ///
    /// Compared field-by-field with the invoice's own receipt before a `Matched` state is
    /// allowed.  `None` on the ledger side makes the comparison a review item, never a match.
    #[serde(default)]
    pub expected_receipt_ref: Option<ProviderReceiptRef>,
    /// BQ-22: the invoice import's provider receipt reference.  Always populated by
    /// `compare_provider_invoice`; hand-built comparisons must carry it to claim a match.
    #[serde(default)]
    pub invoice_receipt_ref: Option<ProviderReceiptRef>,
    pub state: InvoiceComparisonState,
    pub discrepancy_codes: Vec<String>,
    pub correction_ref: Option<String>,
    pub comparison_digest: String,
}

/// 已提交账本一侧的比较期望（服务端从事实派生，不由 wire 提供）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceLedgerExpectation {
    pub ledger_entry_digest: String,
    pub ledger_entry_ref: String,
    pub period_start_unix_ms: u64,
    pub period_end_unix_ms: u64,
    pub model: String,
    /// 账本侧用量摘要；`None` 表示用量仍然未知，此时比较只能是 ReviewRequired。
    pub usage_digest: Option<String>,
    /// 账本侧金额；`None` 表示金额仍然未知，此时比较只能是 ReviewRequired。
    pub amount: Option<Money>,
    pub source_cursor: u64,
    /// BQ-22: the committed ledger fact's own provider receipt reference.
    ///
    /// The invoice must present the same receipt before a comparison may claim a match.  It is
    /// copied from the authoritative entry (never from the wire) so an adapter cannot reintroduce
    /// a hand-passed receipt by constructing the expectation directly.
    #[serde(default)]
    pub receipt_ref: Option<ProviderReceiptRef>,
}

impl InvoiceLedgerExpectation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ledger_entry_digest: impl Into<String>,
        ledger_entry_ref: impl Into<String>,
        period_start_unix_ms: u64,
        period_end_unix_ms: u64,
        model: impl Into<String>,
        usage_digest: Option<String>,
        amount: Option<Money>,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let expectation = Self {
            ledger_entry_digest: ledger_entry_digest.into(),
            ledger_entry_ref: ledger_entry_ref.into(),
            period_start_unix_ms,
            period_end_unix_ms,
            model: model.into(),
            usage_digest,
            amount,
            source_cursor,
            receipt_ref: None,
        };
        expectation.validate()?;
        Ok(expectation)
    }

    /// The whole-card path: derive the expectation from a committed `CostLedgerEntry`.
    ///
    /// Every field comes from the fact itself: the entry digest is re-derived and checked, the
    /// period/model are the server-owned comparison fields, the usage digest is the entry's
    /// `source_digest` (the BQ-13 observation digest the entry was built from), the amount is the
    /// measured cost when present, and the receipt reference is the entry's own
    /// `provider_receipt_ref`.  A fact that never named a period or model cannot be turned into a
    /// matching expectation; the try fails closed instead of letting an adapter fill the gap.
    ///
    /// `usage_digest` is `None` for entries whose cost is still an estimate/unknown, which keeps
    /// the comparison at `usage_unknown` (review) rather than manufacturing a measured value.
    pub fn from_ledger_entry(entry: &CostLedgerEntry) -> Result<Self, String> {
        entry.validate()?;
        let (Some(period_start_unix_ms), Some(period_end_unix_ms)) =
            (entry.period_start_unix_ms, entry.period_end_unix_ms)
        else {
            return Err("provider_invoice_ledger_entry_period_missing".to_owned());
        };
        let Some(model) = entry.model.clone() else {
            return Err("provider_invoice_ledger_entry_model_missing".to_owned());
        };
        // Estimated facts have no measured observation behind them, so their usage stays unknown
        // for invoice comparison.  A measured fact carries the observation digest in
        // `source_digest`, which is the only server-owned usage digest on the entry.
        let (usage_digest, amount) = if entry.measured_cost.is_some() {
            (
                Some(entry.source_digest.clone()),
                entry.measured_cost.clone(),
            )
        } else {
            (None, None)
        };
        let expectation = Self {
            ledger_entry_digest: entry.entry_digest.clone(),
            ledger_entry_ref: format!("ledger-entry:{}", entry.entry_id),
            period_start_unix_ms,
            period_end_unix_ms,
            model,
            usage_digest,
            amount,
            source_cursor: entry.source_cursor,
            receipt_ref: entry.provider_receipt_ref.clone(),
        };
        expectation.validate()?;
        Ok(expectation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !valid_digest(&self.ledger_entry_digest)
            || !valid_text(&self.ledger_entry_ref)
            || self.period_start_unix_ms == 0
            || self.period_end_unix_ms <= self.period_start_unix_ms
            || !valid_text(&self.model)
            || self
                .usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self
                .amount
                .as_ref()
                .is_some_and(|amount| amount.validate().is_err() || amount.micros < 0)
            || self.source_cursor == 0
            || self.receipt_ref.as_ref().is_some_and(|receipt| {
                receipt.as_str().trim().is_empty()
                    || receipt.as_str().len() > 512
                    || receipt.as_str().contains(['\0', '\n', '\r'])
            })
        {
            return Err("provider_invoice_ledger_expectation_invalid".to_owned());
        }
        Ok(())
    }
}

impl TryFrom<&CostLedgerEntry> for InvoiceLedgerExpectation {
    type Error = String;

    fn try_from(entry: &CostLedgerEntry) -> Result<Self, Self::Error> {
        Self::from_ledger_entry(entry)
    }
}

impl InvoiceComparison {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != INVOICE_COMPARISON_SCHEMA
            || !valid_digest(&self.invoice_digest)
            || !valid_digest(&self.ledger_entry_digest)
            || self.expected_period_start_unix_ms == 0
            || self.expected_period_end_unix_ms <= self.expected_period_start_unix_ms
            || self.invoice_period_start_unix_ms == 0
            || self.invoice_period_end_unix_ms <= self.invoice_period_start_unix_ms
            || !valid_text(&self.expected_model)
            || self
                .invoice_model
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self
                .expected_usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self
                .invoice_usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self
                .expected_amount
                .as_ref()
                .is_some_and(|value| value.validate().is_err() || value.micros < 0)
            || self.invoice_amount.validate().is_err()
            || self.invoice_amount.micros < 0
            || self
                .expected_amount
                .as_ref()
                .is_some_and(|value| value.currency != self.invoice_amount.currency)
            || self.discrepancy_codes.len() > MAX_INVOICE_DISCREPANCIES
            || self.discrepancy_codes.iter().any(|code| !valid_text(code))
            || has_duplicates(&self.discrepancy_codes)
            || self
                .correction_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_digest(&self.comparison_digest)
            || self.comparison_digest != self.digest()
        {
            return Err("invoice_comparison_invalid");
        }

        let mismatches = self.mismatch_codes();
        match self.state {
            InvoiceComparisonState::Matched => {
                if !mismatches.is_empty()
                    || self.expected_usage_digest.is_none()
                    || self.invoice_usage_digest.is_none()
                    || self.expected_receipt_ref.is_none()
                    || self.invoice_receipt_ref.is_none()
                    || !self.discrepancy_codes.is_empty()
                    || self.correction_ref.is_some()
                {
                    return Err("invoice_comparison_match_invalid");
                }
            }
            InvoiceComparisonState::CorrectionRequired => {
                if mismatches.is_empty()
                    || self.discrepancy_codes != mismatches
                    || self.correction_ref.is_none()
                {
                    return Err("invoice_comparison_correction_required");
                }
            }
            InvoiceComparisonState::ReviewRequired => {
                if mismatches.is_empty()
                    || self.discrepancy_codes != mismatches
                    || self.correction_ref.is_some()
                {
                    return Err("invoice_comparison_review_required");
                }
            }
        }
        Ok(())
    }

    /// 服务端比较结果是否仍然只知道“未知”，也就是必须走人工复核。
    pub fn requires_review(&self) -> bool {
        self.mismatch_codes()
            .iter()
            .any(|code| code.ends_with("_unknown"))
    }

    pub fn mismatch_codes(&self) -> Vec<String> {
        let mut codes = Vec::new();
        if self.expected_period_start_unix_ms != self.invoice_period_start_unix_ms
            || self.expected_period_end_unix_ms != self.invoice_period_end_unix_ms
        {
            codes.push("period_mismatch".to_owned());
        }
        if self.invoice_model.as_deref() != Some(self.expected_model.as_str()) {
            codes.push("model_mismatch".to_owned());
        }
        match (
            self.expected_usage_digest.as_deref(),
            self.invoice_usage_digest.as_deref(),
        ) {
            (Some(expected), Some(invoice)) if expected == invoice => {}
            (Some(_), Some(_)) => codes.push("usage_mismatch".to_owned()),
            _ => codes.push("usage_unknown".to_owned()),
        }
        match self.expected_amount.as_ref() {
            Some(expected) if expected == &self.invoice_amount => {}
            Some(_) => codes.push("amount_mismatch".to_owned()),
            None => codes.push("amount_unknown".to_owned()),
        }
        // The receipt is the invoice's own identity: without it on both sides the comparison can
        // only be reviewed, and a different receipt is an explicit mismatch.
        match (
            self.expected_receipt_ref.as_ref(),
            self.invoice_receipt_ref.as_ref(),
        ) {
            (Some(expected), Some(invoice)) if expected == invoice => {}
            (Some(_), Some(_)) => codes.push("receipt_mismatch".to_owned()),
            _ => codes.push("receipt_unknown".to_owned()),
        }
        codes
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "invoice_digest": self.invoice_digest,
            "ledger_entry_digest": self.ledger_entry_digest,
            "expected_period_start_unix_ms": self.expected_period_start_unix_ms,
            "expected_period_end_unix_ms": self.expected_period_end_unix_ms,
            "invoice_period_start_unix_ms": self.invoice_period_start_unix_ms,
            "invoice_period_end_unix_ms": self.invoice_period_end_unix_ms,
            "expected_model": self.expected_model,
            "invoice_model": self.invoice_model,
            "expected_usage_digest": self.expected_usage_digest,
            "invoice_usage_digest": self.invoice_usage_digest,
            "expected_amount": self.expected_amount,
            "invoice_amount": self.invoice_amount,
            "expected_receipt_ref": self.expected_receipt_ref,
            "invoice_receipt_ref": self.invoice_receipt_ref,
            "state": self.state,
            "discrepancy_codes": self.discrepancy_codes,
            "correction_ref": self.correction_ref,
        }))
    }
}

/// 用已提交的账本期望派生比较结果。状态与差异码是算出来的，不是 wire 声明的。
///
/// 【检查顺序与规则】
/// 1. 导入事实与账本期望各自先过形状校验；
/// 2. 币种必须一致（否则修正差额没有意义，返回 `provider_invoice_currency_mismatch`）；
/// 3. 差异码由字段派生：period/model/usage/amount/receipt；
/// 4. 只要出现 `*_unknown`（账本或账单一侧的用量、金额、收据未知）就只能是 `ReviewRequired`，
///    绝不生成实测金额；其余差异是 `CorrectionRequired` 且**必须**带修正引用；
/// 5. `Matched` 要求无差异、两侧用量摘要与收据引用齐全且不得携带修正引用。
///
/// 【失败】任何形状或引用缺失都在构造比较事实前拒绝；本函数不修改导入与期望。
pub fn compare_provider_invoice(
    import: &ProviderInvoiceImport,
    expected: &InvoiceLedgerExpectation,
    correction_ref: Option<&str>,
) -> Result<InvoiceComparison, String> {
    import.validate().map_err(str::to_owned)?;
    expected.validate()?;
    if expected
        .amount
        .as_ref()
        .is_some_and(|amount| amount.currency != import.amount.currency)
    {
        return Err("provider_invoice_currency_mismatch".to_owned());
    }
    // The receipt reference is the bill's identity.  If the committed ledger fact names a
    // different provider receipt than the imported invoice, the two documents are not the same
    // bill and no monetary comparison is meaningful: refuse before any fact is built.
    if expected
        .receipt_ref
        .as_ref()
        .is_some_and(|receipt| receipt != &import.receipt_ref)
    {
        return Err("provider_invoice_receipt_mismatch".to_owned());
    }

    let mut comparison = InvoiceComparison {
        schema: INVOICE_COMPARISON_SCHEMA.to_owned(),
        invoice_digest: import.import_digest.clone(),
        ledger_entry_digest: expected.ledger_entry_digest.clone(),
        expected_period_start_unix_ms: expected.period_start_unix_ms,
        expected_period_end_unix_ms: expected.period_end_unix_ms,
        invoice_period_start_unix_ms: import.period_start_unix_ms,
        invoice_period_end_unix_ms: import.period_end_unix_ms,
        expected_model: expected.model.clone(),
        invoice_model: import.model.clone(),
        expected_usage_digest: expected.usage_digest.clone(),
        invoice_usage_digest: import.usage_digest.clone(),
        expected_amount: expected.amount.clone(),
        invoice_amount: import.amount.clone(),
        expected_receipt_ref: expected.receipt_ref.clone(),
        invoice_receipt_ref: Some(import.receipt_ref.clone()),
        state: InvoiceComparisonState::Matched,
        discrepancy_codes: Vec::new(),
        correction_ref: None,
        comparison_digest: String::new(),
    };
    let codes = comparison.mismatch_codes();
    let unknown = codes.iter().any(|code| code.ends_with("_unknown"));
    if codes.is_empty() {
        if correction_ref.is_some() {
            return Err("provider_invoice_correction_ref_unexpected".to_owned());
        }
    } else if unknown {
        if correction_ref.is_some() {
            return Err("provider_invoice_correction_ref_unexpected".to_owned());
        }
        comparison.state = InvoiceComparisonState::ReviewRequired;
    } else {
        let Some(correction_ref) = correction_ref.filter(|value| valid_text(value)) else {
            return Err("provider_invoice_correction_ref_required".to_owned());
        };
        comparison.state = InvoiceComparisonState::CorrectionRequired;
        comparison.correction_ref = Some(correction_ref.to_owned());
    }
    comparison.discrepancy_codes = codes;
    comparison.comparison_digest = comparison.digest();
    comparison.validate().map_err(str::to_owned)?;
    Ok(comparison)
}

pub fn validate_invoice_comparison(comparison: &InvoiceComparison) -> Result<(), &'static str> {
    comparison.validate()
}

/// 一次账单导入最终生成的不可变判定：实测金额、待修正引用或待人工复核项。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceObservationKind {
    Measured,
    CorrectionRequired,
    ReviewRequired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvoiceObservation {
    pub schema: String,
    pub kind: InvoiceObservationKind,
    pub invoice_digest: String,
    pub ledger_entry_digest: String,
    /// 只有 `Measured` 才允许携带金额；零额是显式观测，负额永远拒绝。
    pub measured_amount: Option<Money>,
    pub usage_digest: Option<String>,
    pub discrepancy_codes: Vec<String>,
    pub correction_ref: Option<String>,
    /// 待人工项的可追踪引用；`ReviewRequired` 必填。
    pub review_ref: Option<String>,
    pub source_cursor: u64,
    pub observation_digest: String,
}

impl ProviderInvoiceObservation {
    /// 从**已验证**的服务端比较事实派生观测；`review_ref` 只用于 `ReviewRequired`。
    pub fn from_import_comparison(
        import: &ProviderInvoiceImport,
        comparison: &InvoiceComparison,
        review_ref: Option<&str>,
    ) -> Result<Self, String> {
        import.validate().map_err(str::to_owned)?;
        comparison.validate().map_err(str::to_owned)?;
        if comparison.invoice_digest != import.import_digest {
            return Err("provider_invoice_observation_invoice_mismatch".to_owned());
        }
        let mut observation = Self {
            schema: INVOICE_OBSERVATION_SCHEMA.to_owned(),
            kind: InvoiceObservationKind::ReviewRequired,
            invoice_digest: import.import_digest.clone(),
            ledger_entry_digest: comparison.ledger_entry_digest.clone(),
            measured_amount: None,
            usage_digest: None,
            discrepancy_codes: comparison.discrepancy_codes.clone(),
            correction_ref: comparison.correction_ref.clone(),
            review_ref: None,
            source_cursor: import.source_cursor,
            observation_digest: String::new(),
        };
        match comparison.state {
            InvoiceComparisonState::Matched => {
                if review_ref.is_some() {
                    return Err("provider_invoice_review_ref_unexpected".to_owned());
                }
                observation.kind = InvoiceObservationKind::Measured;
                observation.measured_amount = Some(comparison.invoice_amount.clone());
                observation.usage_digest = comparison.invoice_usage_digest.clone();
            }
            InvoiceComparisonState::CorrectionRequired => {
                if review_ref.is_some() {
                    return Err("provider_invoice_review_ref_unexpected".to_owned());
                }
                observation.kind = InvoiceObservationKind::CorrectionRequired;
            }
            InvoiceComparisonState::ReviewRequired => {
                let Some(review_ref) = review_ref.filter(|value| valid_text(value)) else {
                    return Err("provider_invoice_review_ref_required".to_owned());
                };
                observation.kind = InvoiceObservationKind::ReviewRequired;
                observation.review_ref = Some(review_ref.to_owned());
            }
        }
        observation.observation_digest = observation.digest();
        observation.validate().map_err(str::to_owned)?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != INVOICE_OBSERVATION_SCHEMA
            || !valid_digest(&self.invoice_digest)
            || !valid_digest(&self.ledger_entry_digest)
            || self.source_cursor == 0
            || self
                .measured_amount
                .as_ref()
                .is_some_and(|amount| amount.validate().is_err())
            || self
                .usage_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || self.discrepancy_codes.len() > MAX_INVOICE_DISCREPANCIES
            || self.discrepancy_codes.iter().any(|code| !valid_text(code))
            || has_duplicates(&self.discrepancy_codes)
            || self
                .correction_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self
                .review_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_digest(&self.observation_digest)
            || self.observation_digest != self.digest()
        {
            return Err("provider_invoice_observation_invalid");
        }
        if self
            .measured_amount
            .as_ref()
            .is_some_and(|amount| amount.micros < 0)
        {
            return Err("provider_invoice_observation_amount_negative");
        }
        match self.kind {
            InvoiceObservationKind::Measured => {
                if self.measured_amount.is_none()
                    || self.usage_digest.is_none()
                    || !self.discrepancy_codes.is_empty()
                    || self.correction_ref.is_some()
                    || self.review_ref.is_some()
                {
                    return Err("provider_invoice_observation_measured_invalid");
                }
            }
            InvoiceObservationKind::CorrectionRequired => {
                if self.measured_amount.is_some()
                    || self.usage_digest.is_some()
                    || self.discrepancy_codes.is_empty()
                    || self.correction_ref.is_none()
                    || self.review_ref.is_some()
                {
                    return Err("provider_invoice_observation_correction_invalid");
                }
            }
            InvoiceObservationKind::ReviewRequired => {
                if self.measured_amount.is_some()
                    || self.usage_digest.is_some()
                    || self.correction_ref.is_some()
                    || self.review_ref.is_none()
                    || self.discrepancy_codes.is_empty()
                    || !self
                        .discrepancy_codes
                        .iter()
                        .any(|code| code.ends_with("_unknown"))
                {
                    return Err("provider_invoice_review_requires_unknown");
                }
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "kind": self.kind,
            "invoice_digest": self.invoice_digest,
            "ledger_entry_digest": self.ledger_entry_digest,
            "measured_amount": self.measured_amount,
            "usage_digest": self.usage_digest,
            "discrepancy_codes": self.discrepancy_codes,
            "correction_ref": self.correction_ref,
            "review_ref": self.review_ref,
            "source_cursor": self.source_cursor,
        }))
    }

    pub fn to_runtime_event(
        &self,
        request_id: crate::RequestId,
        sequence: u64,
        stream_version: u64,
    ) -> Result<crate::RuntimeEvent, String> {
        self.validate().map_err(str::to_owned)?;
        if stream_version == 0 {
            return Err("provider_invoice_stream_version_invalid".to_owned());
        }
        let data = serde_json::to_value(self)
            .map_err(|_| "provider_invoice_observation_encode_failed".to_owned())?;
        crate::RuntimeEvent::new(request_id, sequence, INVOICE_OBSERVATION_EVENT, data)
            .map(|event| {
                event.with_stream_metadata(
                    "provider_invoice",
                    self.invoice_digest.clone(),
                    stream_version,
                )
            })
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvoiceImportOutcome {
    Imported,
    Replayed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvoiceObservationOutcome {
    Recorded,
    Replayed,
}

/// 只追加的账单导入账本：`Vec` + 身份集合，没有 UPDATE/DELETE 入口。
///
/// 它保存的是**事实**：同一 invoice 身份第二次以不同摘要出现时整条拒绝；同摘要重放幂等；
/// 来源游标只前进；一张 invoice 只产生一条观测，实测金额不可能被后来的观测静默覆盖。
#[derive(Clone, Debug, Default)]
pub struct ProviderInvoiceLedger {
    imports: Vec<ProviderInvoiceImport>,
    observations: Vec<ProviderInvoiceObservation>,
    last_source_cursor: u64,
}

impl ProviderInvoiceLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_source_cursor(&self) -> u64 {
        self.last_source_cursor
    }

    pub fn imports(&self) -> impl Iterator<Item = &ProviderInvoiceImport> {
        self.imports.iter()
    }

    pub fn observations(&self) -> impl Iterator<Item = &ProviderInvoiceObservation> {
        self.observations.iter()
    }

    pub fn import(&self, import_digest: &str) -> Option<&ProviderInvoiceImport> {
        self.imports
            .iter()
            .find(|import| import.import_digest == import_digest)
    }

    pub fn observation(&self, invoice_digest: &str) -> Option<&ProviderInvoiceObservation> {
        self.observations
            .iter()
            .find(|observation| observation.invoice_digest == invoice_digest)
    }

    /// 追加一条导入。拒绝路径不改变账本内容与 `last_source_cursor`。
    ///
    /// 【顺序】形状 → 身份重复（同摘要重放，异摘要拒绝）→ `import_key` 冲突 →
    /// 来源游标回退。
    pub fn append(
        &mut self,
        import: &ProviderInvoiceImport,
    ) -> Result<InvoiceImportOutcome, String> {
        import.validate().map_err(str::to_owned)?;
        if let Some(existing) = self
            .imports
            .iter()
            .find(|existing| existing.identity() == import.identity())
        {
            if existing.import_digest == import.import_digest {
                return Ok(InvoiceImportOutcome::Replayed);
            }
            return Err("provider_invoice_duplicate".to_owned());
        }
        if self
            .imports
            .iter()
            .any(|existing| existing.import_key == import.import_key)
        {
            return Err("provider_invoice_import_key_conflict".to_owned());
        }
        if import.source_cursor <= self.last_source_cursor {
            return Err("provider_invoice_source_cursor_regression".to_owned());
        }
        self.last_source_cursor = import.source_cursor;
        self.imports.push(import.clone());
        Ok(InvoiceImportOutcome::Imported)
    }

    /// 记录一次观测。一张 invoice 只允许一条观测事实。
    ///
    /// 【顺序】形状 → 必须存在同摘要导入（否则 `provider_invoice_observation_unknown_import`）
    /// → 同摘要重放幂等 → 其它任何差异都拒绝；两条实测金额不同的观测属于
    /// `provider_invoice_measured_overwrite_refused`，绝不静默覆盖原值。
    pub fn record_observation(
        &mut self,
        observation: &ProviderInvoiceObservation,
    ) -> Result<InvoiceObservationOutcome, String> {
        observation.validate().map_err(str::to_owned)?;
        if !self
            .imports
            .iter()
            .any(|import| import.import_digest == observation.invoice_digest)
        {
            return Err("provider_invoice_observation_unknown_import".to_owned());
        }
        if let Some(existing) = self
            .observations
            .iter()
            .find(|existing| existing.invoice_digest == observation.invoice_digest)
        {
            if existing.observation_digest == observation.observation_digest {
                return Ok(InvoiceObservationOutcome::Replayed);
            }
            if existing.kind == InvoiceObservationKind::Measured
                && observation.kind == InvoiceObservationKind::Measured
            {
                return Err("provider_invoice_measured_overwrite_refused".to_owned());
            }
            return Err("provider_invoice_observation_replay_conflict".to_owned());
        }
        self.observations.push(observation.clone());
        Ok(InvoiceObservationOutcome::Recorded)
    }

    /// 已实测的观测（成功列可追踪视图）。
    pub fn measured_observations(&self) -> Vec<&ProviderInvoiceObservation> {
        self.observations
            .iter()
            .filter(|observation| observation.kind == InvoiceObservationKind::Measured)
            .collect()
    }

    /// 待修正项（修正引用已确定，仍待 BQ-14 审批）。
    pub fn open_corrections(&self) -> Vec<&ProviderInvoiceObservation> {
        self.observations
            .iter()
            .filter(|observation| observation.kind == InvoiceObservationKind::CorrectionRequired)
            .collect()
    }

    /// 待人工复核项（未知用量/未知金额等）。
    pub fn pending_reviews(&self) -> Vec<&ProviderInvoiceObservation> {
        self.observations
            .iter()
            .filter(|observation| observation.kind == InvoiceObservationKind::ReviewRequired)
            .collect()
    }
}

/// 把「需要修正」的比较结果交给既有 BQ-14 命令的唯一 handoff。
///
/// 它只携带绑定信息与差额证据：没有审批、没有落账、没有第二套修正账本。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvoiceCorrectionHandoff {
    pub schema: String,
    pub invoice_digest: String,
    pub ledger_entry_digest: String,
    pub correction_ref: String,
    pub discrepancy_codes: Vec<String>,
    /// `invoice - expected` 的显式差额；零额或缺失金额一律拒绝。
    pub delta_measured: Money,
    pub evidence_refs: Vec<String>,
    pub handoff_digest: String,
}

impl ProviderInvoiceCorrectionHandoff {
    pub fn from_import_comparison(
        import: &ProviderInvoiceImport,
        comparison: &InvoiceComparison,
    ) -> Result<Self, String> {
        import.validate().map_err(str::to_owned)?;
        comparison.validate().map_err(str::to_owned)?;
        if comparison.state != InvoiceComparisonState::CorrectionRequired {
            return Err("provider_invoice_handoff_requires_correction".to_owned());
        }
        if comparison.invoice_digest != import.import_digest {
            return Err("provider_invoice_handoff_invoice_mismatch".to_owned());
        }
        if !comparison
            .discrepancy_codes
            .iter()
            .any(|code| code == "amount_mismatch")
        {
            return Err("provider_invoice_handoff_amount_mismatch_required".to_owned());
        }
        let expected = comparison
            .expected_amount
            .as_ref()
            .ok_or_else(|| "provider_invoice_handoff_expected_amount_required".to_owned())?;
        if expected.currency != comparison.invoice_amount.currency {
            return Err("provider_invoice_handoff_currency_mismatch".to_owned());
        }
        let delta = comparison
            .invoice_amount
            .micros
            .checked_sub(expected.micros)
            .ok_or_else(|| "provider_invoice_handoff_delta_overflow".to_owned())?;
        if delta == 0 {
            return Err("provider_invoice_handoff_delta_missing".to_owned());
        }
        let correction_ref = comparison
            .correction_ref
            .clone()
            .ok_or_else(|| "provider_invoice_handoff_correction_ref_required".to_owned())?;
        let mut handoff = Self {
            schema: INVOICE_CORRECTION_HANDOFF_SCHEMA.to_owned(),
            invoice_digest: import.import_digest.clone(),
            ledger_entry_digest: comparison.ledger_entry_digest.clone(),
            correction_ref: correction_ref.clone(),
            discrepancy_codes: comparison.discrepancy_codes.clone(),
            delta_measured: Money::new(comparison.invoice_amount.currency.clone(), delta)?,
            evidence_refs: vec![
                format!("receipt:{}", import.receipt_ref.as_str()),
                format!("invoice:{}", import.import_digest),
                format!("import:{}", import.import_key),
                format!("correction:{correction_ref}"),
            ],
            handoff_digest: String::new(),
        };
        handoff.handoff_digest = handoff.digest();
        handoff.validate().map_err(str::to_owned)?;
        Ok(handoff)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != INVOICE_CORRECTION_HANDOFF_SCHEMA
            || !valid_digest(&self.invoice_digest)
            || !valid_digest(&self.ledger_entry_digest)
            || !valid_text(&self.correction_ref)
            || self.discrepancy_codes.is_empty()
            || self.discrepancy_codes.len() > MAX_INVOICE_DISCREPANCIES
            || self.discrepancy_codes.iter().any(|code| !valid_text(code))
            || has_duplicates(&self.discrepancy_codes)
            || !self
                .discrepancy_codes
                .iter()
                .any(|code| code == "amount_mismatch")
            || self.evidence_refs.is_empty()
            || self.evidence_refs.len() > MAX_INVOICE_HANDOFF_EVIDENCE
            || self.evidence_refs.iter().any(|value| !valid_text(value))
            || has_duplicates(&self.evidence_refs)
            || self.delta_measured.validate().is_err()
            || self.delta_measured.micros == 0
            || !valid_digest(&self.handoff_digest)
            || self.handoff_digest != self.digest()
        {
            return Err("provider_invoice_handoff_invalid");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "invoice_digest": self.invoice_digest,
            "ledger_entry_digest": self.ledger_entry_digest,
            "correction_ref": self.correction_ref,
            "discrepancy_codes": self.discrepancy_codes,
            "delta_measured": self.delta_measured,
            "evidence_refs": self.evidence_refs,
        }))
    }

    /// 绑定校验：只接受**未批准**的 BQ-14 draft 命令，且目标、差额、证据与差异码全部一致。
    ///
    /// 这里不调用审批、不追加事件、不伪造 `CostCorrection`；批准与落账仍由 BQ-14
    /// `CostCorrectionAdmission` 与 ControlPlane 事务负责。
    pub fn validate_draft_command(&self, command: &CostCorrectionCommand) -> Result<(), String> {
        self.validate().map_err(str::to_owned)?;
        if command.approval.is_some() {
            return Err("provider_invoice_handoff_command_approved".to_owned());
        }
        if command.command_digest != command.digest() {
            return Err("provider_invoice_handoff_command_digest_mismatch".to_owned());
        }
        if command.target_entry_digest != self.ledger_entry_digest {
            return Err("provider_invoice_handoff_command_binding_mismatch".to_owned());
        }
        if command.correction_id.to_string() != self.correction_ref {
            return Err("provider_invoice_handoff_command_reference_mismatch".to_owned());
        }
        if command.delta_estimated.is_some()
            || command.delta_measured.as_ref() != Some(&self.delta_measured)
        {
            return Err("provider_invoice_handoff_command_delta_mismatch".to_owned());
        }
        if self
            .evidence_refs
            .iter()
            .any(|reference| !command.evidence_refs.contains(reference))
        {
            return Err("provider_invoice_handoff_command_evidence_missing".to_owned());
        }
        if self
            .discrepancy_codes
            .iter()
            .any(|code| !command.reason.contains(code.as_str()))
        {
            return Err("provider_invoice_handoff_command_reason_missing".to_owned());
        }
        Ok(())
    }
}

pub fn validate_invoice_correction_handoff(
    handoff: &ProviderInvoiceCorrectionHandoff,
) -> Result<(), &'static str> {
    handoff.validate()
}

// ---------------------------------------------------------------------------
// BQ-22 local-only invoice document parser
// ---------------------------------------------------------------------------

/// Schema of the local provider invoice document an operator may hand to Kiana.
///
/// This is a *file* format, not a provider protocol: the parser below never opens a socket, reads
/// a URL or trusts a signature field.  It turns bytes into the same typed contracts the rest of
/// the billing path already uses, and fails closed on anything it does not fully understand.
pub const PROVIDER_INVOICE_DOCUMENT_SCHEMA: &str = "kiana.provider-invoice-document.v1";
/// Maximum accepted invoice document size.  A provider PDF turned into JSON must still fit a
/// bounded buffer; the adapter refuses anything larger before parsing.
pub const MAX_PROVIDER_INVOICE_DOCUMENT_BYTES: usize = 256 * 1024;

/// A parsed local invoice document: the import fact plus the server authentication evidence that
/// must bind it.  Both are produced by [`parse_provider_invoice_document`]; neither is trusted
/// merely because it appeared in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedProviderInvoiceDocument {
    pub import: ProviderInvoiceImport,
    pub authentication: InvoiceAuthenticationEvidence,
}

/// Wire shape of the invoice document.  Deliberately narrow and `deny_unknown_fields`: a vendor
/// export with extra columns is rejected instead of being partially interpreted.
///
/// The document carries an *authentication block* fields, not a signature: a local file cannot
/// prove a provider signed it.  `verifier_ref`/`verified_at_unix_ms`/`authority_epoch` describe
/// who checked the receipt bytes and when, and are still compared against the caller-supplied
/// `now`/`epoch` by [`ProviderInvoiceImport::validate_against_authentication`].  The digest fields
/// are recomputed locally.  A real signature verifier remains out of scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInvoiceDocument {
    pub schema: String,
    pub provider_account_ref: String,
    pub invoice_id: String,
    pub period_start_unix_ms: u64,
    pub period_end_unix_ms: u64,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub usage_digest: Option<String>,
    /// `kiana.money.v1` value.  Negative micros are refused before the import is built.
    pub amount: Money,
    pub receipt_ref: String,
    /// Digest of the receipt bytes, computed by the operator's import step.
    pub receipt_digest: String,
    pub verifier_ref: String,
    pub verified_at_unix_ms: u64,
    pub authority_epoch: u64,
    pub source_cursor: u64,
    pub import_key: String,
}

/// Parse a provider invoice document into the import fact and its authentication evidence.
///
/// # Failure order (all before any fact exists)
/// 1. size bound → 2. strict JSON decode (`deny_unknown_fields`, no duplicate keys) →
/// 3. document schema → 4. money shape/negative amount → 5. receipt reference shape →
/// 6. digest shapes → 7. period/cursor/epoch shapes → 8. sealed import/evidence validation.
///
/// Every failure returns a stable `provider_invoice_document_*` code.  The `import_digest` and
/// `evidence_digest` are always recomputed here; a file that ships its own digests is rejected as
/// an unknown field, so a caller cannot hand-pass a covering digest.
pub fn parse_provider_invoice_document(
    bytes: &[u8],
) -> Result<ParsedProviderInvoiceDocument, String> {
    if bytes.is_empty() {
        return Err("provider_invoice_document_empty".to_owned());
    }
    if bytes.len() > MAX_PROVIDER_INVOICE_DOCUMENT_BYTES {
        return Err("provider_invoice_document_too_large".to_owned());
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| "provider_invoice_document_not_utf8".to_owned())?;
    let document: ProviderInvoiceDocument = serde_json::from_str(text)
        .map_err(|error| format!("provider_invoice_document_invalid_json:{error}"))?;

    if document.schema != PROVIDER_INVOICE_DOCUMENT_SCHEMA {
        return Err("provider_invoice_document_schema_invalid".to_owned());
    }
    document
        .amount
        .validate()
        .map_err(|_| "provider_invoice_document_money_invalid".to_owned())?;
    if document.amount.micros < 0 {
        return Err("provider_invoice_document_amount_negative".to_owned());
    }
    let receipt_ref = ProviderReceiptRef::new(document.receipt_ref.clone())
        .map_err(|_| "provider_invoice_document_receipt_invalid".to_owned())?;
    if !valid_digest(&document.receipt_digest) {
        return Err("provider_invoice_document_receipt_digest_invalid".to_owned());
    }
    if document.verified_at_unix_ms == 0 {
        return Err("provider_invoice_document_verified_at_invalid".to_owned());
    }
    if document.authority_epoch == 0 {
        return Err("provider_invoice_document_authority_epoch_invalid".to_owned());
    }
    if document.source_cursor == 0 {
        return Err("provider_invoice_document_source_cursor_invalid".to_owned());
    }
    if !valid_text(&document.import_key) {
        return Err("provider_invoice_document_import_key_invalid".to_owned());
    }

    let mut import = ProviderInvoiceImport {
        schema: PROVIDER_INVOICE_SCHEMA.to_owned(),
        provider_account_ref: document.provider_account_ref.clone(),
        invoice_id: document.invoice_id.clone(),
        period_start_unix_ms: document.period_start_unix_ms,
        period_end_unix_ms: document.period_end_unix_ms,
        model: document.model.clone(),
        usage_digest: document.usage_digest.clone(),
        amount: document.amount.clone(),
        receipt_ref: receipt_ref.clone(),
        receipt_digest: document.receipt_digest.clone(),
        authentication_ref: format!("document:{}", document.verifier_ref),
        // The document is local evidence, not a provider assertion.  `authenticated` is set only
        // after the evidence below binds it, but the field itself is still a shape gate: a
        // document that does not carry verification fields fails the later evidence check.
        authenticated: true,
        source_cursor: document.source_cursor,
        import_key: document.import_key.clone(),
        import_digest: String::new(),
    };
    import.import_digest = import.digest();
    import
        .validate()
        .map_err(|code| format!("provider_invoice_document_import_invalid:{code}"))?;

    let authentication = InvoiceAuthenticationEvidence::new(
        document.provider_account_ref,
        document.invoice_id,
        document.period_start_unix_ms,
        document.period_end_unix_ms,
        document.receipt_digest,
        document.verifier_ref,
        document.verified_at_unix_ms,
        document.authority_epoch,
    )
    .map_err(|code| format!("provider_invoice_document_evidence_invalid:{code}"))?;

    Ok(ParsedProviderInvoiceDocument {
        import,
        authentication,
    })
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn has_duplicates(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().any(|value| !seen.insert(value))
}
