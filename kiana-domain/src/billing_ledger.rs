//! Append-only cost ledger and approval-bound correction contracts.
//!
//! BQ-13 produces immutable per-attempt cost observations.  This module gives those observations
//! a ledger identity without turning a receipt or model transcript into an authority.  Original
//! entries can only be appended.  A correction is a separate fact that carries the exact target
//! digest, run/attempt/source cursor/revision fence, bounded evidence references and a committed
//! approval.  The in-memory reducer is intentionally deterministic and side-effect free; callers
//! append its facts through the existing ControlPlane/EventLog transaction.

use crate::{
    json_digest, ApprovalId, AttemptId, CostBreakdown, CostBreakdownKind, EventId, LedgerEntryId,
    Money, ProviderReceiptRef, QuotaReservationId, RateCardId, RequestId, RunId, SchemaVersion,
    UsageId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COST_LEDGER_ENTRY_SCHEMA: &str = "kiana.cost-ledger-entry.v1";
pub const COST_CORRECTION_SCHEMA: &str = "kiana.cost-correction.v1";
pub const COST_CORRECTION_COMMAND_SCHEMA: &str = "kiana.cost-correction-command.v1";
pub const COST_CORRECTION_APPROVAL_SCHEMA: &str = "kiana.cost-correction-approval.v1";
pub const COST_LEDGER_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const COST_LEDGER_ENTRY_EVENT: &str = "cost.ledger_entry";
pub const COST_CORRECTION_EVENT: &str = "cost.corrected";
pub const COST_CORRECTION_COMMAND: &str = "cost.correction";
pub const MAX_COST_CORRECTION_EVIDENCE: usize = 64;
pub const MAX_COST_CORRECTION_REASON: usize = 1_024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .bytes()
            .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn currency(value: &str, field: &str) -> Result<(), String> {
    if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_idempotency(value: &str) -> Result<(), String> {
    required(value, "cost_idempotency_key", 256)
}

fn validate_evidence_refs(refs: &[String]) -> Result<(), String> {
    if refs.is_empty() || refs.len() > MAX_COST_CORRECTION_EVIDENCE {
        return Err("cost_correction_evidence_required".to_owned());
    }
    let mut seen = std::collections::BTreeSet::new();
    for reference in refs {
        required(reference, "cost_correction_evidence_ref", 512)?;
        if !seen.insert(reference) {
            return Err("cost_correction_evidence_duplicate".to_owned());
        }
    }
    Ok(())
}

/// The only kinds of immutable facts that may enter the cost ledger.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostLedgerEntryKind {
    Reservation,
    Consumption,
    Release,
    Correction,
}

impl CostLedgerEntryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reservation => "reservation",
            Self::Consumption => "consumption",
            Self::Release => "release",
            Self::Correction => "correction",
        }
    }

    pub const fn is_original(self) -> bool {
        !matches!(self, Self::Correction)
    }
}

/// One immutable billing fact.  The struct intentionally stores both estimate and measured
/// fields: an estimate can later be superseded by a separate measured entry or correction, but
/// no adapter may mutate this record in place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostLedgerEntry {
    pub schema: String,
    pub version: SchemaVersion,
    pub entry_id: LedgerEntryId,
    pub kind: CostLedgerEntryKind,
    pub run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<AttemptId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_ref: Option<UsageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_ref: Option<QuotaReservationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Money>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost: Option<Money>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured_cost: Option<Money>,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_id: Option<RateCardId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_receipt_ref: Option<ProviderReceiptRef>,
    pub created_at_unix_ms: u64,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub revision: u64,
    pub source_digest: String,
    pub entry_digest: String,
}

impl CostLedgerEntry {
    /// Build a fact with the minimum server-owned source fence. Optional fields can be added with
    /// the `with_*` builders; every builder re-seals the digest before returning.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        entry_id: LedgerEntryId,
        kind: CostLedgerEntryKind,
        run_id: RunId,
        attempt_id: Option<AttemptId>,
        amount: Option<Money>,
        currency: impl Into<String>,
        created_at_unix_ms: u64,
        source_event_id: EventId,
        source_cursor: u64,
        revision: u64,
        source_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut entry = Self {
            schema: COST_LEDGER_ENTRY_SCHEMA.to_owned(),
            version: COST_LEDGER_VERSION,
            entry_id,
            kind,
            run_id,
            attempt_id,
            usage_ref: None,
            reservation_ref: None,
            allocation_ref: None,
            amount,
            estimated_cost: None,
            measured_cost: None,
            currency: currency.into(),
            rate_card_id: None,
            rate_card_version: None,
            provider_receipt_ref: None,
            created_at_unix_ms,
            source_event_id,
            source_cursor,
            revision,
            source_digest: source_digest.into(),
            entry_digest: String::new(),
        };
        entry.reseal();
        entry.validate()?;
        Ok(entry)
    }

    /// Create an immutable ledger fact from the BQ-13 observation. The source digest is the
    /// observation digest and no provider body or model text is copied into the ledger.
    pub fn from_cost_breakdown(
        entry_id: LedgerEntryId,
        kind: CostLedgerEntryKind,
        breakdown: &CostBreakdown,
        created_at_unix_ms: u64,
        source_event_id: EventId,
        source_cursor: u64,
        revision: u64,
    ) -> Result<Self, String> {
        breakdown.validate()?;
        let attempt_id = breakdown.attempt_id;
        let (amount, estimated_cost, measured_cost, rate_card_id, rate_card_version, receipt) =
            match &breakdown.kind {
                CostBreakdownKind::Estimated { estimate } => (
                    estimate.amount.clone(),
                    estimate.amount.clone(),
                    None,
                    Some(estimate.rate_card_id),
                    Some(estimate.rate_card_version),
                    None,
                ),
                CostBreakdownKind::Measured {
                    amount,
                    provider_receipt,
                } => (
                    Some(amount.clone()),
                    None,
                    Some(amount.clone()),
                    breakdown.rate_card_id,
                    breakdown.rate_card_version,
                    Some(provider_receipt.clone()),
                ),
                CostBreakdownKind::Unknown { .. } => (
                    None,
                    None,
                    None,
                    breakdown.rate_card_id,
                    breakdown.rate_card_version,
                    None,
                ),
            };
        let currency = amount
            .as_ref()
            .or(estimated_cost.as_ref())
            .or(measured_cost.as_ref())
            .map(|money| money.currency.clone())
            .unwrap_or_else(|| "UNK".to_owned());
        let mut entry = Self::new(
            entry_id,
            kind,
            breakdown.run_id,
            attempt_id,
            amount,
            currency,
            created_at_unix_ms,
            source_event_id,
            source_cursor,
            revision,
            breakdown.breakdown_digest.clone(),
        )?;
        entry.estimated_cost = estimated_cost;
        entry.measured_cost = measured_cost;
        entry.rate_card_id = rate_card_id;
        entry.rate_card_version = rate_card_version;
        entry.provider_receipt_ref = receipt;
        entry.reseal();
        entry.validate()?;
        Ok(entry)
    }

    pub fn with_usage_ref(mut self, usage_ref: UsageId) -> Result<Self, String> {
        self.usage_ref = Some(usage_ref);
        self.reseal_and_validate()
    }

    pub fn with_reservation_ref(
        mut self,
        reservation_ref: QuotaReservationId,
    ) -> Result<Self, String> {
        self.reservation_ref = Some(reservation_ref);
        self.reseal_and_validate()
    }

    pub fn with_allocation_ref(
        mut self,
        allocation_ref: impl Into<String>,
    ) -> Result<Self, String> {
        self.allocation_ref = Some(allocation_ref.into());
        self.reseal_and_validate()
    }

    pub fn with_estimated_cost(mut self, estimated_cost: Money) -> Result<Self, String> {
        self.estimated_cost = Some(estimated_cost);
        self.reseal_and_validate()
    }

    pub fn with_measured_cost(
        mut self,
        measured_cost: Money,
        provider_receipt_ref: ProviderReceiptRef,
    ) -> Result<Self, String> {
        self.measured_cost = Some(measured_cost);
        self.provider_receipt_ref = Some(provider_receipt_ref);
        self.reseal_and_validate()
    }

    pub fn with_rate_card(
        mut self,
        rate_card_id: RateCardId,
        rate_card_version: u64,
    ) -> Result<Self, String> {
        self.rate_card_id = Some(rate_card_id);
        self.rate_card_version = Some(rate_card_version);
        self.reseal_and_validate()
    }

    fn reseal_and_validate(mut self) -> Result<Self, String> {
        self.reseal();
        self.validate()?;
        Ok(self)
    }

    fn reseal(&mut self) {
        self.entry_digest = self.digest();
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_LEDGER_ENTRY_SCHEMA
            || !self.version.is_compatible_with(&COST_LEDGER_VERSION)
            || self.entry_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.created_at_unix_ms == 0
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.revision == 0
        {
            return Err("cost_ledger_entry_header_invalid".to_owned());
        }
        currency(&self.currency, "cost_ledger_entry_currency")?;
        digest(&self.source_digest, "cost_ledger_entry_source_digest")?;
        digest(&self.entry_digest, "cost_ledger_entry_digest")?;
        if self.entry_digest != self.digest() {
            return Err("cost_ledger_entry_digest_mismatch".to_owned());
        }
        if self.kind.is_original() && self.attempt_id.is_none() {
            return Err("cost_ledger_entry_attempt_required".to_owned());
        }
        if let Some(allocation_ref) = &self.allocation_ref {
            required(allocation_ref, "cost_ledger_entry_allocation_ref", 512)?;
        }
        for money in [
            self.amount.as_ref(),
            self.estimated_cost.as_ref(),
            self.measured_cost.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            money.validate()?;
            if money.currency != self.currency {
                return Err("cost_ledger_entry_currency_mismatch".to_owned());
            }
        }
        if self.estimated_cost.is_some()
            && (self.rate_card_id.is_none() || self.rate_card_version.is_none())
        {
            return Err("cost_ledger_entry_rate_card_required".to_owned());
        }
        if self.rate_card_id.is_some() != self.rate_card_version.is_some()
            || self.rate_card_version == Some(0)
        {
            return Err("cost_ledger_entry_rate_card_invalid".to_owned());
        }
        if self.measured_cost.is_some() {
            if self.provider_receipt_ref.is_none() {
                return Err("cost_ledger_entry_provider_receipt_required".to_owned());
            }
            let receipt = self.provider_receipt_ref.as_ref().expect("checked above");
            ProviderReceiptRef::new(receipt.as_str().to_owned())
                .map(|_| ())
                .map_err(|_| "cost_ledger_entry_provider_receipt_invalid".to_owned())?;
        } else if self.provider_receipt_ref.is_some() {
            return Err("cost_ledger_entry_provider_receipt_without_measured".to_owned());
        }
        if self.kind == CostLedgerEntryKind::Correction
            && self.estimated_cost.is_none()
            && self.measured_cost.is_none()
        {
            return Err("cost_ledger_entry_correction_amount_required".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "entry_id": self.entry_id,
            "kind": self.kind,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "usage_ref": self.usage_ref,
            "reservation_ref": self.reservation_ref,
            "allocation_ref": self.allocation_ref,
            "amount": self.amount,
            "estimated_cost": self.estimated_cost,
            "measured_cost": self.measured_cost,
            "currency": self.currency,
            "rate_card_id": self.rate_card_id,
            "rate_card_version": self.rate_card_version,
            "provider_receipt_ref": self.provider_receipt_ref,
            "created_at_unix_ms": self.created_at_unix_ms,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "revision": self.revision,
            "source_digest": self.source_digest,
        }))
    }
}

/// Approval proof for one correction command.  It is a digest-only subject binding; approval is
/// never inferred from a model response or from a caller-provided boolean.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostCorrectionApproval {
    pub schema: String,
    pub version: SchemaVersion,
    pub approval_id: ApprovalId,
    pub command_id: RequestId,
    pub command_digest: String,
    pub target_entry_digest: String,
    pub approver_id: String,
    pub approved_at_unix_ms: u64,
    pub approval_digest: String,
}

impl CostCorrectionApproval {
    pub fn new(
        approval_id: ApprovalId,
        command_id: RequestId,
        command_digest: impl Into<String>,
        target_entry_digest: impl Into<String>,
        approver_id: impl Into<String>,
        approved_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut approval = Self {
            schema: COST_CORRECTION_APPROVAL_SCHEMA.to_owned(),
            version: COST_LEDGER_VERSION,
            approval_id,
            command_id,
            command_digest: command_digest.into(),
            target_entry_digest: target_entry_digest.into(),
            approver_id: approver_id.into(),
            approved_at_unix_ms,
            approval_digest: String::new(),
        };
        approval.approval_digest = approval.digest();
        approval.validate()?;
        Ok(approval)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_CORRECTION_APPROVAL_SCHEMA
            || !self.version.is_compatible_with(&COST_LEDGER_VERSION)
            || self.approval_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.approved_at_unix_ms == 0
        {
            return Err("cost_correction_approval_header_invalid".to_owned());
        }
        digest(
            &self.command_digest,
            "cost_correction_approval_command_digest",
        )?;
        digest(
            &self.target_entry_digest,
            "cost_correction_approval_target_digest",
        )?;
        required(&self.approver_id, "cost_correction_approval_approver", 256)?;
        digest(&self.approval_digest, "cost_correction_approval_digest")?;
        if self.approval_digest != self.digest() {
            return Err("cost_correction_approval_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "approval_id": self.approval_id,
            "command_id": self.command_id,
            "command_digest": self.command_digest,
            "target_entry_digest": self.target_entry_digest,
            "approver_id": self.approver_id,
            "approved_at_unix_ms": self.approved_at_unix_ms,
        }))
    }
}

/// Versioned command submitted to ControlPlane for approval.  Raw model text is not part of this
/// type; a caller can only name a target and provide bounded digest/evidence references.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostCorrectionCommand {
    pub schema: String,
    pub version: SchemaVersion,
    pub command_id: RequestId,
    pub correction_id: crate::CostCorrectionId,
    pub target_entry_id: LedgerEntryId,
    pub target_entry_digest: String,
    pub target_run_id: RunId,
    pub target_attempt_id: AttemptId,
    pub target_source_cursor: u64,
    pub target_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_estimated: Option<Money>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_measured: Option<Money>,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub requested_by: String,
    pub idempotency_key: String,
    pub created_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<CostCorrectionApproval>,
    pub command_digest: String,
}

impl CostCorrectionCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn draft(
        command_id: RequestId,
        correction_id: crate::CostCorrectionId,
        target: &CostLedgerEntry,
        delta_estimated: Option<Money>,
        delta_measured: Option<Money>,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
        requested_by: impl Into<String>,
        idempotency_key: impl Into<String>,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        target.validate()?;
        let mut command = Self {
            schema: COST_CORRECTION_COMMAND_SCHEMA.to_owned(),
            version: COST_LEDGER_VERSION,
            command_id,
            correction_id,
            target_entry_id: target.entry_id,
            target_entry_digest: target.entry_digest.clone(),
            target_run_id: target.run_id,
            target_attempt_id: target
                .attempt_id
                .ok_or_else(|| "cost_correction_target_attempt_required".to_owned())?,
            target_source_cursor: target.source_cursor,
            target_revision: target.revision,
            delta_estimated,
            delta_measured,
            reason: reason.into(),
            evidence_refs,
            requested_by: requested_by.into(),
            idempotency_key: idempotency_key.into(),
            created_at_unix_ms,
            approval: None,
            command_digest: String::new(),
        };
        command.command_digest = command.digest();
        command.validate_draft_against(target)?;
        Ok(command)
    }

    /// Alias used by command adapters that call the unapproved state a request.
    pub fn new(
        command_id: RequestId,
        correction_id: crate::CostCorrectionId,
        target: &CostLedgerEntry,
        delta_estimated: Option<Money>,
        delta_measured: Option<Money>,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
        requested_by: impl Into<String>,
        idempotency_key: impl Into<String>,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::draft(
            command_id,
            correction_id,
            target,
            delta_estimated,
            delta_measured,
            reason,
            evidence_refs,
            requested_by,
            idempotency_key,
            created_at_unix_ms,
        )
    }

    pub fn with_approval(mut self, approval: CostCorrectionApproval) -> Result<Self, String> {
        approval.validate()?;
        if approval.command_id != self.command_id
            || approval.command_digest != self.command_digest
            || approval.target_entry_digest != self.target_entry_digest
        {
            return Err("cost_correction_approval_binding_mismatch".to_owned());
        }
        self.approval = Some(approval);
        self.validate_approved()?;
        Ok(self)
    }

    pub fn validate_draft_against(&self, target: &CostLedgerEntry) -> Result<(), String> {
        self.validate_common()?;
        if self.target_entry_id != target.entry_id
            || self.target_entry_digest != target.entry_digest
            || self.target_run_id != target.run_id
            || Some(self.target_attempt_id) != target.attempt_id
            || self.target_source_cursor != target.source_cursor
            || self.target_revision != target.revision
        {
            return Err("cost_correction_target_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_approved(&self) -> Result<(), String> {
        self.validate_common()?;
        let approval = self
            .approval
            .as_ref()
            .ok_or_else(|| "cost_correction_approval_required".to_owned())?;
        approval.validate()?;
        if approval.command_id != self.command_id
            || approval.command_digest != self.command_digest
            || approval.target_entry_digest != self.target_entry_digest
        {
            return Err("cost_correction_approval_binding_mismatch".to_owned());
        }
        if approval.approver_id == self.requested_by {
            return Err("cost_correction_conflicted_approver".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_approved()
    }

    fn validate_common(&self) -> Result<(), String> {
        if self.schema != COST_CORRECTION_COMMAND_SCHEMA
            || !self.version.is_compatible_with(&COST_LEDGER_VERSION)
            || self.command_id.as_uuid().is_nil()
            || self.correction_id.as_uuid().is_nil()
            || self.target_entry_id.as_uuid().is_nil()
            || self.target_run_id.as_uuid().is_nil()
            || self.target_attempt_id.as_uuid().is_nil()
            || self.target_source_cursor == 0
            || self.target_revision == 0
            || self.created_at_unix_ms == 0
        {
            return Err("cost_correction_command_header_invalid".to_owned());
        }
        digest(&self.target_entry_digest, "cost_correction_target_digest")?;
        required(
            &self.reason,
            "cost_correction_reason",
            MAX_COST_CORRECTION_REASON,
        )?;
        validate_evidence_refs(&self.evidence_refs)?;
        required(&self.requested_by, "cost_correction_requested_by", 256)?;
        validate_idempotency(&self.idempotency_key)?;
        validate_delta_pair(self.delta_estimated.as_ref(), self.delta_measured.as_ref())?;
        digest(&self.command_digest, "cost_correction_command_digest")?;
        if self.command_digest != self.digest() {
            return Err("cost_correction_command_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Digest excludes the approval so the approval can bind a stable command subject before it
    /// is attached. Approval itself is still included in the appended CostCorrection fact.
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "command_id": self.command_id,
            "correction_id": self.correction_id,
            "target_entry_id": self.target_entry_id,
            "target_entry_digest": self.target_entry_digest,
            "target_run_id": self.target_run_id,
            "target_attempt_id": self.target_attempt_id,
            "target_source_cursor": self.target_source_cursor,
            "target_revision": self.target_revision,
            "delta_estimated": self.delta_estimated,
            "delta_measured": self.delta_measured,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "requested_by": self.requested_by,
            "idempotency_key": self.idempotency_key,
            "created_at_unix_ms": self.created_at_unix_ms,
        }))
    }
}

fn validate_delta_pair(
    delta_estimated: Option<&Money>,
    delta_measured: Option<&Money>,
) -> Result<(), String> {
    if delta_estimated.is_none() && delta_measured.is_none() {
        return Err("cost_correction_delta_required".to_owned());
    }
    if let Some(estimated) = delta_estimated {
        estimated.validate()?;
    }
    if let Some(measured) = delta_measured {
        measured.validate()?;
    }
    if let (Some(estimated), Some(measured)) = (delta_estimated, delta_measured) {
        if estimated.currency != measured.currency {
            return Err("cost_correction_currency_mismatch".to_owned());
        }
        if estimated.micros == 0 && measured.micros == 0 {
            return Err("cost_correction_zero_delta".to_owned());
        }
    } else if delta_estimated
        .or(delta_measured)
        .is_some_and(|money| money.micros == 0)
    {
        return Err("cost_correction_zero_delta".to_owned());
    }
    Ok(())
}

/// Immutable correction fact emitted only after a command passed the ControlPlane approval gate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostCorrection {
    pub schema: String,
    pub version: SchemaVersion,
    pub correction_id: crate::CostCorrectionId,
    pub command_id: RequestId,
    pub command_digest: String,
    pub target_entry_id: LedgerEntryId,
    pub target_entry_digest: String,
    /// Server-owned run identity repeated for event routing; it must equal `target_run_id`.
    pub run_id: RunId,
    pub target_run_id: RunId,
    pub target_attempt_id: AttemptId,
    pub target_source_cursor: u64,
    pub target_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_estimated: Option<Money>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_measured: Option<Money>,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub requested_by: String,
    pub idempotency_key: String,
    pub approval_id: ApprovalId,
    pub approver_id: String,
    pub approved_at_unix_ms: u64,
    pub approval_digest: String,
    pub created_at_unix_ms: u64,
    pub correction_digest: String,
}

impl CostCorrection {
    pub fn from_command(
        command: &CostCorrectionCommand,
        target: &CostLedgerEntry,
    ) -> Result<Self, String> {
        command.validate_draft_against(target)?;
        command.validate_approved()?;
        let approval = command.approval.as_ref().expect("validated approval");
        let mut correction = Self {
            schema: COST_CORRECTION_SCHEMA.to_owned(),
            version: COST_LEDGER_VERSION,
            correction_id: command.correction_id,
            command_id: command.command_id,
            command_digest: command.command_digest.clone(),
            target_entry_id: command.target_entry_id,
            target_entry_digest: command.target_entry_digest.clone(),
            run_id: command.target_run_id,
            target_run_id: command.target_run_id,
            target_attempt_id: command.target_attempt_id,
            target_source_cursor: command.target_source_cursor,
            target_revision: command.target_revision,
            delta_estimated: command.delta_estimated.clone(),
            delta_measured: command.delta_measured.clone(),
            reason: command.reason.clone(),
            evidence_refs: command.evidence_refs.clone(),
            requested_by: command.requested_by.clone(),
            idempotency_key: command.idempotency_key.clone(),
            approval_id: approval.approval_id,
            approver_id: approval.approver_id.clone(),
            approved_at_unix_ms: approval.approved_at_unix_ms,
            approval_digest: approval.approval_digest.clone(),
            created_at_unix_ms: command.created_at_unix_ms,
            correction_digest: String::new(),
        };
        correction.correction_digest = correction.digest();
        correction.validate_against(target)?;
        Ok(correction)
    }

    pub fn validate_against(&self, target: &CostLedgerEntry) -> Result<(), String> {
        target.validate()?;
        self.validate()?;
        if self.target_entry_id != target.entry_id
            || self.target_entry_digest != target.entry_digest
            || self.target_run_id != target.run_id
            || Some(self.target_attempt_id) != target.attempt_id
            || self.target_source_cursor != target.source_cursor
            || self.target_revision != target.revision
        {
            return Err("cost_correction_target_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_CORRECTION_SCHEMA
            || !self.version.is_compatible_with(&COST_LEDGER_VERSION)
            || self.correction_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.target_entry_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.target_run_id.as_uuid().is_nil()
            || self.target_attempt_id.as_uuid().is_nil()
            || self.target_source_cursor == 0
            || self.target_revision == 0
            || self.approval_id.as_uuid().is_nil()
            || self.approved_at_unix_ms == 0
            || self.created_at_unix_ms == 0
        {
            return Err("cost_correction_header_invalid".to_owned());
        }
        if self.run_id != self.target_run_id {
            return Err("cost_correction_run_binding_mismatch".to_owned());
        }
        digest(&self.command_digest, "cost_correction_command_digest")?;
        digest(&self.target_entry_digest, "cost_correction_target_digest")?;
        digest(&self.approval_digest, "cost_correction_approval_digest")?;
        digest(&self.correction_digest, "cost_correction_digest")?;
        required(
            &self.reason,
            "cost_correction_reason",
            MAX_COST_CORRECTION_REASON,
        )?;
        required(&self.requested_by, "cost_correction_requested_by", 256)?;
        validate_idempotency(&self.idempotency_key)?;
        required(&self.approver_id, "cost_correction_approver", 256)?;
        if self.requested_by == self.approver_id {
            return Err("cost_correction_conflicted_approver".to_owned());
        }
        validate_evidence_refs(&self.evidence_refs)?;
        validate_delta_pair(self.delta_estimated.as_ref(), self.delta_measured.as_ref())?;
        if self.correction_digest != self.digest() {
            return Err("cost_correction_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "correction_id": self.correction_id,
            "command_id": self.command_id,
            "command_digest": self.command_digest,
            "target_entry_id": self.target_entry_id,
            "target_entry_digest": self.target_entry_digest,
            "run_id": self.run_id,
            "target_run_id": self.target_run_id,
            "target_attempt_id": self.target_attempt_id,
            "target_source_cursor": self.target_source_cursor,
            "target_revision": self.target_revision,
            "delta_estimated": self.delta_estimated,
            "delta_measured": self.delta_measured,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "requested_by": self.requested_by,
            "idempotency_key": self.idempotency_key,
            "approval_id": self.approval_id,
            "approver_id": self.approver_id,
            "approved_at_unix_ms": self.approved_at_unix_ms,
            "approval_digest": self.approval_digest,
            "created_at_unix_ms": self.created_at_unix_ms,
        }))
    }
}

/// Deterministic result of appending a correction command. Replay never mutates the target fact.
#[derive(Clone, Debug, PartialEq)]
pub enum CostCorrectionAppendOutcome {
    Committed(CostCorrection),
    Replayed(CostCorrection),
}

/// Read model formed by folding immutable originals with appended corrections. The original entry
/// and every correction remain available for audit; only this derived view contains totals.
#[derive(Clone, Debug, PartialEq)]
pub struct CostLedgerView {
    pub original: CostLedgerEntry,
    pub corrections: Vec<CostCorrection>,
    pub estimated_cost: Option<Money>,
    pub measured_cost: Option<Money>,
}

impl CostLedgerView {
    pub fn from_parts(
        original: CostLedgerEntry,
        mut corrections: Vec<CostCorrection>,
    ) -> Result<Self, String> {
        original.validate()?;
        corrections
            .sort_by_key(|correction| (correction.target_revision, correction.correction_id));
        let mut estimated = original.estimated_cost.clone();
        let mut measured = original.measured_cost.clone();
        for correction in &corrections {
            correction.validate_against(&original)?;
            estimated = add_delta(estimated, correction.delta_estimated.as_ref())?;
            measured = add_delta(measured, correction.delta_measured.as_ref())?;
        }
        Ok(Self {
            original,
            corrections,
            estimated_cost: estimated,
            measured_cost: measured,
        })
    }
}

fn add_delta(base: Option<Money>, delta: Option<&Money>) -> Result<Option<Money>, String> {
    let Some(delta) = delta else { return Ok(base) };
    match base {
        Some(base) => base.checked_add(delta).map(Some),
        None => Ok(Some(delta.clone())),
    }
}

/// Small append-only reducer used by query adapters and CI fixtures. It stores no mutable version
/// of an entry and exposes no update/delete operation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostLedger {
    entries: BTreeMap<LedgerEntryId, CostLedgerEntry>,
    corrections: BTreeMap<crate::CostCorrectionId, CostCorrection>,
    commands: BTreeMap<RequestId, (String, crate::CostCorrectionId)>,
    idempotency_keys: BTreeMap<String, (String, crate::CostCorrectionId)>,
}

impl CostLedger {
    pub fn append_entry(&mut self, entry: CostLedgerEntry) -> Result<(), String> {
        entry.validate()?;
        if self.entries.contains_key(&entry.entry_id) {
            return Err("cost_ledger_entry_duplicate".to_owned());
        }
        if self
            .entries
            .values()
            .any(|existing| existing.source_event_id == entry.source_event_id)
        {
            return Err("cost_ledger_source_event_duplicate".to_owned());
        }
        self.entries.insert(entry.entry_id, entry);
        Ok(())
    }

    pub fn append_correction(
        &mut self,
        command: CostCorrectionCommand,
    ) -> Result<CostCorrectionAppendOutcome, String> {
        let target = self
            .entries
            .get(&command.target_entry_id)
            .ok_or_else(|| "cost_correction_target_missing".to_owned())?;
        command.validate_draft_against(target)?;
        command.validate_approved()?;
        if let Some((digest, correction_id)) = self.commands.get(&command.command_id) {
            if digest != &command.command_digest {
                return Err("cost_correction_command_replay_conflict".to_owned());
            }
            return self
                .corrections
                .get(correction_id)
                .cloned()
                .map(CostCorrectionAppendOutcome::Replayed)
                .ok_or_else(|| "cost_correction_replay_missing_fact".to_owned());
        }
        if let Some((digest, correction_id)) = self.idempotency_keys.get(&command.idempotency_key) {
            if digest != &command.command_digest {
                return Err("cost_correction_idempotency_conflict".to_owned());
            }
            return self
                .corrections
                .get(correction_id)
                .cloned()
                .map(CostCorrectionAppendOutcome::Replayed)
                .ok_or_else(|| "cost_correction_replay_missing_fact".to_owned());
        }
        if self.corrections.contains_key(&command.correction_id) {
            return Err("cost_correction_id_duplicate".to_owned());
        }
        let correction = CostCorrection::from_command(&command, target)?;
        self.commands.insert(
            command.command_id,
            (command.command_digest.clone(), command.correction_id),
        );
        self.idempotency_keys.insert(
            command.idempotency_key.clone(),
            (command.command_digest.clone(), command.correction_id),
        );
        self.corrections
            .insert(command.correction_id, correction.clone());
        Ok(CostCorrectionAppendOutcome::Committed(correction))
    }

    pub fn entry(&self, entry_id: LedgerEntryId) -> Option<&CostLedgerEntry> {
        self.entries.get(&entry_id)
    }

    pub fn correction(&self, correction_id: crate::CostCorrectionId) -> Option<&CostCorrection> {
        self.corrections.get(&correction_id)
    }

    pub fn entries(&self) -> impl Iterator<Item = &CostLedgerEntry> {
        self.entries.values()
    }

    pub fn corrections(&self) -> impl Iterator<Item = &CostCorrection> {
        self.corrections.values()
    }

    pub fn view(&self, entry_id: LedgerEntryId) -> Result<CostLedgerView, String> {
        let original = self
            .entries
            .get(&entry_id)
            .cloned()
            .ok_or_else(|| "cost_ledger_entry_missing".to_owned())?;
        let corrections = self
            .corrections
            .values()
            .filter(|correction| correction.target_entry_id == entry_id)
            .cloned()
            .collect();
        CostLedgerView::from_parts(original, corrections)
    }
}
