//! PD-23 Core restore verification facade.
//!
//! This module is the only Core-side authority that turns an already-sealed external-effect
//! reconciliation receipt into restore evidence, and the only place that combines a restore fact
//! with an `ActivationState` readiness gate.  It performs no I/O, copies no root, appends no event
//! and mints no lease: activation stays in [`crate::restore_activation`].

use kiana_domain::{
    require_verified_restore_verification, validate_restore_verification, EffectObservation,
    EffectObservationState, RestoreEffectResolution, RestoreExternalEffectClaim,
    RestoreVerificationFact,
};

use crate::{
    admit_command_after_activation, ActivationState, EffectReconciliationReceipt,
    EffectReconciliationRequest, EffectResolution,
};

pub fn validate_restore_verification_fact(
    fact: &RestoreVerificationFact,
) -> Result<(), &'static str> {
    validate_restore_verification(fact)
}

/// The activation readiness gate for a restore fact.
///
/// Two things must hold before a restored root may take commands: the verification fact must be
/// `Verified` (not merely structurally valid, and not `Blocked`/`NeedsReconciliation`), and the
/// activation state must be `Activated`.  Either failing is a stable refusal rather than a
/// caller's judgement call.
pub fn require_restore_ready(
    fact: &RestoreVerificationFact,
    state: &ActivationState,
) -> Result<(), &'static str> {
    require_verified_restore_verification(fact)?;
    state
        .validate()
        .map_err(|_| "restore_activation_state_invalid")?;
    if !state.ready() {
        return Err("restore_activation_not_ready");
    }
    Ok(())
}

/// The single admissible command gate after a restore.
///
/// This is the production-shaped entry: it refuses a non-`Verified` fact, a non-ready activation
/// state, the replaced root, and any command whose instance/fence/epochs do not match the
/// activated root.  It delegates to [`admit_command_after_activation`] rather than re-deriving
/// the fence rules here, so there is exactly one readiness implementation.
pub fn admit_restored_command(
    fact: &RestoreVerificationFact,
    state: &ActivationState,
    command_root: &kiana_domain::StorageRootId,
    command_instance_id: &kiana_domain::InstanceId,
    command_fence_token: kiana_domain::FenceTokenId,
    command_authority_epoch: u64,
    command_data_epoch: u64,
) -> Result<(), String> {
    require_restore_ready(fact, state).map_err(str::to_owned)?;
    admit_command_after_activation(
        state,
        command_root,
        command_instance_id,
        command_fence_token,
        command_authority_epoch,
        command_data_epoch,
    )
}

/// Derive restore evidence from one sealed reconciliation receipt and the observation it decided.
///
/// The receipt is re-validated against its request, and the observation is re-validated in full,
/// so neither an edited receipt nor a mismatched observation can become restore evidence.  An
/// `Unknown` observation is refused here before the claim constructor sees it.
pub fn restore_effect_claim_from_receipt(
    request: &EffectReconciliationRequest,
    receipt: &EffectReconciliationReceipt,
    observation: &EffectObservation,
    authority_epoch: u64,
) -> Result<RestoreExternalEffectClaim, String> {
    // Re-derive the decision from the request; a receipt that no longer matches is refused.
    receipt.validate_against(request)?;
    observation.validate()?;
    if !receipt.admitted() {
        return Err("restore_effect_receipt_not_admitted".to_owned());
    }
    if observation.observation_digest != request.observation.observation_digest {
        return Err("restore_effect_observation_binding_invalid".to_owned());
    }
    if request.authority_epoch != authority_epoch {
        return Err("restore_effect_authority_epoch_mismatch".to_owned());
    }
    if observation.state == EffectObservationState::Unknown {
        return Err("restore_effect_claim_unknown".to_owned());
    }
    let resolution = match receipt.resolution {
        EffectResolution::Reconciled => RestoreEffectResolution::Reconciled,
        EffectResolution::RetryWithoutEffect => RestoreEffectResolution::RetryWithoutEffect,
        EffectResolution::Abandoned => RestoreEffectResolution::Abandoned,
        EffectResolution::Compensated => RestoreEffectResolution::Compensated,
    };
    RestoreExternalEffectClaim::new(
        observation.observation_digest.clone(),
        observation.state,
        resolution,
        receipt.receipt_digest.clone(),
        receipt.approval_ref.clone(),
        receipt.authority_epoch,
    )
}

/// Why a restore fact's external-effect evidence was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreEffectClaimError {
    /// The claim is not sealed, or its resolution contradicts its observed state.
    Invalid,
    /// The claim's `Unknown` state can never be reconciled.
    Unknown,
    /// No receipt was supplied for a claim.
    MissingReceipt,
    /// A receipt does not re-derive to the claim it is supposed to support.
    ReceiptMismatch,
    /// The same observation appears under two claims.
    Duplicate,
}

impl RestoreEffectClaimError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Invalid => "restore_effect_claim_invalid",
            Self::Unknown => "restore_effect_claim_unknown",
            Self::MissingReceipt => "restore_effect_claim_receipt_missing",
            Self::ReceiptMismatch => "restore_effect_claim_receipt_mismatch",
            Self::Duplicate => "restore_effect_claim_duplicate",
        }
    }
}

/// Cross-check every claim in the fact against the reconciliation receipts and observations that
/// are supposed to back it.
///
/// The fact alone cannot prove an effect was reconciled: `claim_digest` only proves the claim is
/// self-consistent.  This function re-derives each claim from a caller-supplied receipt and
/// observation and refuses any claim that does not match, any `Unknown` observation, and any
/// duplicate observation.  A fact with no claims passes trivially — which is correct, because
/// such a fact cannot be `Verified` when `unknown_effect_count` is non-zero.
pub fn verify_restore_effect_claims(
    fact: &RestoreVerificationFact,
    evidence: &[(
        &EffectReconciliationRequest,
        &EffectReconciliationReceipt,
        &EffectObservation,
    )],
) -> Result<(), RestoreEffectClaimError> {
    if fact.external_effect_claims.len() > kiana_domain::MAX_RESTORE_EFFECT_CLAIMS {
        return Err(RestoreEffectClaimError::Invalid);
    }
    let mut seen = std::collections::BTreeSet::new();
    for claim in &fact.external_effect_claims {
        claim.validate().map_err(|code| match code {
            "restore_effect_claim_unknown" => RestoreEffectClaimError::Unknown,
            _ => RestoreEffectClaimError::Invalid,
        })?;
        if !seen.insert(claim.observation_digest.as_str()) {
            return Err(RestoreEffectClaimError::Duplicate);
        }
        let Some((request, receipt, observation)) = evidence
            .iter()
            .find(|(_, _, observation)| observation.observation_digest == claim.observation_digest)
            .copied()
        else {
            return Err(RestoreEffectClaimError::MissingReceipt);
        };
        let derived =
            restore_effect_claim_from_receipt(request, receipt, observation, claim.authority_epoch)
                .map_err(|code| match code.as_str() {
                    "restore_effect_claim_unknown" => RestoreEffectClaimError::Unknown,
                    _ => RestoreEffectClaimError::ReceiptMismatch,
                })?;
        if derived.observation_digest != claim.observation_digest
            || derived.observed_state != claim.observed_state
            || derived.resolution != claim.resolution
            || derived.receipt_digest != claim.receipt_digest
            || derived.approval_ref != claim.approval_ref
            || derived.authority_epoch != claim.authority_epoch
        {
            return Err(RestoreEffectClaimError::ReceiptMismatch);
        }
    }
    Ok(())
}
