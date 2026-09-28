//! DEP-25: what to do about an external effect whose real result is not known.
//!
//! The whole card is about one uncomfortable fact: a request went out, and nobody knows whether it
//! landed. Retrying it might send it twice. Declaring it failed might be a lie. Declaring it
//! succeeded might be a lie. This module refuses to make that decision on its own and instead
//! decides **which of four honest moves is admissible**, given an observation that someone else
//! actually made.
//!
//! ```text
//! EffectObservation { ConfirmedSuccess | ConfirmedFailure | NoEffect | Unknown }
//!        ↓  reconcile_effect(request)
//! EffectResolution  { Reconciled | RetryWithoutEffect | Abandoned | Compensated }
//!        ↓  sealed receipt (its own digest, bound to the observation)
//! ```
//!
//! # The rule the card leads with
//!
//! An `Unknown` may **not** be resolved by retrying. `RetryWithoutEffect` is admissible only from
//! `NoEffect` — that is, only when somebody proved nothing happened. Retrying an unknown is how a
//! timeout becomes a double charge, and this module refuses it at the point of decision rather
//! than leaving it to discipline.
//!
//! # Why four resolutions and not one status
//!
//! `Reconciled`, `RetryWithoutEffect`, `Abandoned` and `Compensated` have genuinely different
//! preconditions, and each carries its own receipt. Collapsing them into "resolved" would lose
//! exactly the distinction that matters: whether a compensating action was taken is a fact an
//! auditor needs, and it is not the same fact as "we stopped caring".
//!
//! # This module decides, it does not act
//!
//! It performs no lookup, calls no provider, appends no event and issues no approval. The
//! observation is supplied; this decides whether the proposed resolution may be recorded against
//! it, and re-derives that decision in `validate_against` so an edited receipt cannot publish
//! itself.

use kiana_domain::{json_digest, redact_text, scan_secret_sentinels, EffectObservation, EffectObservationState, SchemaVersion, SecretScanChannel};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const EFFECT_RECONCILIATION_SCHEMA: &str = "kiana.effect-reconciliation.v1";
pub const EFFECT_RECONCILIATION_RECEIPT_SCHEMA: &str = "kiana.effect-reconciliation-receipt.v1";
pub const EFFECT_RECONCILIATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EFFECT_RECONCILIATION_TEXT: usize = 256;

/// The four admissible moves. Each has its own receipt and its own preconditions.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectResolution {
    /// The external system told us what happened; record that.
    Reconciled,
    /// Nothing happened, so the same request may be sent again.
    RetryWithoutEffect,
    /// There will never be an effect, so there is nothing left to wait for.
    Abandoned,
    /// Something may have happened and a compensating action was taken.
    Compensated,
}

impl EffectResolution {
    pub const ALL: [Self; 4] = [
        Self::Reconciled,
        Self::RetryWithoutEffect,
        Self::Abandoned,
        Self::Compensated,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reconciled => "reconciled",
            Self::RetryWithoutEffect => "retry_without_effect",
            Self::Abandoned => "abandoned",
            Self::Compensated => "compensated",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "reconciled" => Ok(Self::Reconciled),
            "retry_without_effect" => Ok(Self::RetryWithoutEffect),
            "abandoned" => Ok(Self::Abandoned),
            "compensated" => Ok(Self::Compensated),
            _ => Err("effect_resolution_invalid".to_owned()),
        }
    }

    /// Does this resolution assert that an effect is now known to have happened?
    const fn asserts_effect(self) -> bool {
        matches!(self, Self::Reconciled | Self::Compensated)
    }
}

/// A reference to the outside world's own record, when there is one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalReceiptRef {
    /// The provider's own identifier for the call. Opaque on purpose: this module does not know
    /// any provider's id format and must not grow a parser for one.
    pub external_ref: String,
    pub receipt_digest: String,
    pub observed_at_unix_ms: u64,
}

impl ExternalReceiptRef {
    pub fn new(
        external_ref: impl Into<String>,
        receipt_digest: impl Into<String>,
        observed_at_unix_ms: u64,
    ) -> Self {
        Self {
            external_ref: external_ref.into(),
            receipt_digest: receipt_digest.into(),
            observed_at_unix_ms,
        }
    }

    fn validate(&self) -> Result<(), String> {
        safe_text(&self.external_ref, "effect_external_ref")?;
        valid_digest(&self.receipt_digest, "effect_external_receipt_digest")?;
        if self.observed_at_unix_ms == 0 {
            return Err("effect_external_receipt_timestamp_required".to_owned());
        }
        Ok(())
    }
}

/// The proposal: somebody wants to record a resolution against an observation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectReconciliationRequest {
    pub schema: String,
    pub version: SchemaVersion,
    /// The observation this proposal is about. Supplied, not fetched.
    pub observation: EffectObservation,
    pub resolution: EffectResolution,
    /// Present exactly when the resolution asserts an effect happened.
    pub external_receipt: Option<ExternalReceiptRef>,
    /// Present exactly when the resolution is `Compensated`.
    pub compensation_ref: Option<String>,
    /// Present exactly when the resolution is `Abandoned`.
    pub abandon_reason: Option<String>,
    /// The approval that authorised this reconciliation.
    pub approval_ref: String,
    pub authority_epoch: u64,
    pub fence_token: String,
    /// Approvals already spent. Reusing one is refused: an approval is single-use, and a second
    /// reconciliation under the same approval is how one human sign-off becomes two effects.
    pub consumed_approvals: Vec<String>,
    /// The key that makes this proposal replay-safe.
    pub idempotency_key: String,
    /// The payload the key was first used with. A replay that changes it is a conflict.
    pub request_digest: String,
    pub request_digest_seal: String,
}

impl EffectReconciliationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        observation: EffectObservation,
        resolution: EffectResolution,
        external_receipt: Option<ExternalReceiptRef>,
        compensation_ref: Option<String>,
        abandon_reason: Option<String>,
        approval_ref: impl Into<String>,
        authority_epoch: u64,
        fence_token: impl Into<String>,
        consumed_approvals: Vec<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: EFFECT_RECONCILIATION_SCHEMA.to_owned(),
            version: EFFECT_RECONCILIATION_VERSION,
            observation,
            resolution,
            external_receipt,
            compensation_ref,
            abandon_reason,
            approval_ref: approval_ref.into(),
            authority_epoch,
            fence_token: fence_token.into(),
            consumed_approvals,
            idempotency_key: idempotency_key.into(),
            request_digest: String::new(),
            request_digest_seal: String::new(),
        };
        value.request_digest = value.digest();
        value.request_digest_seal = json_digest(&json!({ "request": value.request_digest }));
        value.validate()?;
        Ok(value)
    }

    /// The seal over everything except the seal itself.
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "observation_digest": self.observation.observation_digest,
            "resolution": self.resolution,
            "external_receipt": self.external_receipt,
            "compensation_ref": self.compensation_ref,
            "abandon_reason": self.abandon_reason,
            "approval_ref": self.approval_ref,
            "authority_epoch": self.authority_epoch,
            "fence_token": self.fence_token,
            "consumed_approvals": self.consumed_approvals,
            "idempotency_key": self.idempotency_key,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EFFECT_RECONCILIATION_SCHEMA
            || !self.version.is_compatible_with(&EFFECT_RECONCILIATION_VERSION)
        {
            return Err("effect_reconcile_header_invalid".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("effect_reconcile_request_digest_mismatch".to_owned());
        }
        safe_text(&self.approval_ref, "effect_approval_ref")?;
        if self.authority_epoch == 0 {
            return Err("effect_approval_epoch_required".to_owned());
        }
        safe_text(&self.fence_token, "effect_fence_token")?;
        safe_text(&self.idempotency_key, "effect_idempotency_key")?;
        if self
            .consumed_approvals
            .iter()
            .any(|approval| approval == &self.approval_ref)
        {
            return Err("effect_approval_reused".to_owned());
        }
        for approval in &self.consumed_approvals {
            safe_text(approval, "effect_consumed_approval")?;
        }
        if let Some(receipt) = &self.external_receipt {
            receipt.validate()?;
        }
        if let Some(reason) = &self.abandon_reason {
            safe_text(reason, "effect_abandon_reason")?;
        }
        if let Some(reference) = &self.compensation_ref {
            safe_text(reference, "effect_compensation_ref")?;
        }
        decide(self).map(|_| ())
    }
}

/// The sealed decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectReconciliationReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub observation_digest: String,
    pub resolution: EffectResolution,
    /// The observation state that was actually in force. Carried so the receipt says what it was
    /// reasoning from, not only what it concluded.
    pub observed_state: EffectObservationState,
    pub external_receipt: Option<ExternalReceiptRef>,
    pub compensation_ref: Option<String>,
    pub approval_ref: String,
    pub authority_epoch: u64,
    pub idempotency_key: String,
    pub request_digest: String,
    pub reason: String,
    pub receipt_digest: String,
}

impl EffectReconciliationReceipt {
    pub fn admitted(&self) -> bool {
        self.reason.is_empty()
    }

    /// Re-derive the decision from the same request.
    ///
    /// This is what makes the receipt unpublishable on its own terms: any field edited after the
    /// fact stops matching the derivation and the receipt is refused.
    pub fn validate_against(&self, request: &EffectReconciliationRequest) -> Result<(), String> {
        request.validate()?;
        let (reason, observed_state) = decide(request)?;
        if self.schema != EFFECT_RECONCILIATION_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&EFFECT_RECONCILIATION_VERSION)
            || self.observation_digest != request.observation.observation_digest
            || self.resolution != request.resolution
            || self.observed_state != observed_state
            || self.external_receipt != request.external_receipt
            || self.compensation_ref != request.compensation_ref
            || self.approval_ref != request.approval_ref
            || self.authority_epoch != request.authority_epoch
            || self.idempotency_key != request.idempotency_key
            || self.request_digest != request.request_digest
            || self.reason != reason
        {
            return Err("effect_reconcile_receipt_binding_invalid".to_owned());
        }
        if self.receipt_digest != self.digest() {
            return Err("effect_reconcile_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "observation_digest": self.observation_digest,
            "resolution": self.resolution,
            "observed_state": self.observed_state,
            "external_receipt": self.external_receipt,
            "compensation_ref": self.compensation_ref,
            "approval_ref": self.approval_ref,
            "authority_epoch": self.authority_epoch,
            "idempotency_key": self.idempotency_key,
            "request_digest": self.request_digest,
            "reason": self.reason,
        }))
    }
}

/// The decision itself: an empty reason means admissible.
///
/// It returns the reason **and** the observed state rather than a bare `Result`, because the state
/// is part of what the receipt has to say and recomputing it in two places is how the two drift.
fn decide(request: &EffectReconciliationRequest) -> Result<(String, EffectObservationState), String> {
    let state = request.observation.state;
    let resolution = request.resolution;
    match resolution {
        EffectResolution::RetryWithoutEffect => {
            // The rule the card leads with. Retrying is admissible only from a *proven* absence of
            // effect. `Unknown` is exactly the case where nobody proved anything, so it is refused
            // here rather than left to the caller's discretion.
            if state != EffectObservationState::NoEffect {
                return Err("effect_reconcile_unknown_auto_retry".to_owned());
            }
            if request.external_receipt.is_some() {
                return Err("effect_reconcile_receipt_without_effect".to_owned());
            }
        }
        EffectResolution::Reconciled => {
            // Success cannot be asserted locally. Somebody has to have heard it from outside.
            if state == EffectObservationState::Unknown {
                return Err("effect_reconcile_success_without_external_ref".to_owned());
            }
            if request.external_receipt.is_none() {
                return Err("effect_reconcile_success_without_external_ref".to_owned());
            }
            if request.observation.provider_receipt_id.is_none() {
                return Err("effect_reconcile_observation_receipt_missing".to_owned());
            }
        }
        EffectResolution::Compensated => {
            if state == EffectObservationState::NoEffect {
                // Nothing happened, so there is nothing to compensate. Recording a compensation
                // here would put a phantom action into the audit trail.
                return Err("effect_reconcile_compensation_without_effect".to_owned());
            }
            if request.compensation_ref.is_none() {
                return Err("effect_reconcile_compensation_ref_required".to_owned());
            }
            if request.observation.evidence_ref_digests.is_empty() {
                return Err("effect_reconcile_compensation_evidence_required".to_owned());
            }
        }
        EffectResolution::Abandoned => {
            // Abandoning is for something that will never take effect. An unknown effect is not
            // that: abandoning it is how an unknown quietly becomes a success.
            if state != EffectObservationState::NoEffect {
                return Err("effect_reconcile_abandon_requires_no_effect".to_owned());
            }
            if request.abandon_reason.is_none() {
                return Err("effect_reconcile_abandon_reason_required".to_owned());
            }
            if request.external_receipt.is_some() || request.compensation_ref.is_some() {
                return Err("effect_reconcile_abandon_conflicting_evidence".to_owned());
            }
        }
    }
    // A field that belongs to another resolution must not travel with this one, or a receipt ends
    // up saying two things at once.
    if !resolution.asserts_effect() && request.external_receipt.is_some() {
        return Err("effect_reconcile_external_ref_not_allowed".to_owned());
    }
    if resolution != EffectResolution::Compensated && request.compensation_ref.is_some() {
        return Err("effect_reconcile_compensation_ref_not_allowed".to_owned());
    }
    if resolution != EffectResolution::Abandoned && request.abandon_reason.is_some() {
        return Err("effect_reconcile_abandon_reason_not_allowed".to_owned());
    }
    Ok((String::new(), state))
}

/// Decide a proposal and return the sealed receipt.
pub fn reconcile_effect(
    request: &EffectReconciliationRequest,
) -> Result<EffectReconciliationReceipt, String> {
    let (reason, observed_state) = decide(request)?;
    let mut receipt = EffectReconciliationReceipt {
        schema: EFFECT_RECONCILIATION_RECEIPT_SCHEMA.to_owned(),
        version: EFFECT_RECONCILIATION_VERSION,
        observation_digest: request.observation.observation_digest.clone(),
        resolution: request.resolution,
        observed_state,
        external_receipt: request.external_receipt.clone(),
        compensation_ref: request.compensation_ref.clone(),
        approval_ref: request.approval_ref.clone(),
        authority_epoch: request.authority_epoch,
        idempotency_key: request.idempotency_key.clone(),
        request_digest: request.request_digest.clone(),
        reason,
        receipt_digest: String::new(),
    };
    receipt.receipt_digest = receipt.digest();
    receipt.validate_against(request)?;
    Ok(receipt)
}

/// Check a replay: the same idempotency key must never carry a different request.
///
/// A key that is reused with different content is not a retry, it is a second decision wearing a
/// retry's name, and that is the shape a double effect takes.
pub fn check_idempotent_replay(
    prior: &EffectReconciliationRequest,
    replay: &EffectReconciliationRequest,
) -> Result<(), String> {
    prior.validate()?;
    replay.validate()?;
    if prior.idempotency_key != replay.idempotency_key {
        return Err("effect_reconcile_idempotency_key_mismatch".to_owned());
    }
    let prior_keys: BTreeSet<&String> = prior.consumed_approvals.iter().collect();
    for approval in &replay.consumed_approvals {
        if prior_keys.contains(approval) && approval != &replay.approval_ref {
            return Err("effect_reconcile_approval_reused".to_owned());
        }
    }
    if prior.request_digest != replay.request_digest {
        return Err("effect_reconcile_idempotency_conflict".to_owned());
    }
    Ok(())
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_EFFECT_RECONCILIATION_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
