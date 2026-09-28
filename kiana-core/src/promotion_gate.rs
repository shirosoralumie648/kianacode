//! BQ-30: the gate a claim passes before it is allowed to be called something stronger than it is.
//!
//! Three failures, in the card's own words:
//!
//! - **a type, a unit test or an estimate cannot be called billing live.** The gap between "the
//!   code compiles" and "money moved" is the whole gap the proof ladder exists to describe.
//! - **no provider receipt means nothing may be called measured.** An estimate computed locally is
//!   an estimate, and rendering it as a measurement is how a budget page starts lying.
//! - **unrecorded limitations block the promotion.** A claim with an empty limitations list has
//!   not been reviewed, because reviewing is what produces them.
//!
//! # Offline durable and opt-in live are two different doors
//!
//! The card insists they stay separate, and the module keeps them as two separate claims rather
//! than one "production" level. They are separated because they answer different questions and
//! have different preconditions: *durable* asks whether the fact survives a restart, and *live*
//! asks whether a real external system was actually exercised under an explicit opt-in. A claim
//! that satisfied one has said nothing about the other, and collapsing them is how an offline
//! rehearsal ends up presented as a live run.
//!
//! # The gate consumes the evidence contract, it does not define one
//!
//! The input is an `EvidenceManifest` from the SC-35 contract. Reusing it is the point: a second
//! notion of "what counts as evidence" is a second answer to the question this gate exists to ask.
//!
//! This module decides. It promotes nothing, starts nothing and reads no file.

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::evidence_manifest::{EvidenceManifest, EvidenceProofLevel};

pub const PROMOTION_GATE_REQUEST_SCHEMA: &str = "kiana.promotion-gate-request.v1";
pub const PROMOTION_GATE_DECISION_SCHEMA: &str = "kiana.promotion-gate-decision.v1";
pub const PROMOTION_GATE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// What a claimant is asking the claim to be called.
///
/// The ordering is the ladder, and it is an ordering rather than a set of labels because the gate's
/// whole job is to compare a claim against the evidence beneath it.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimedLevel {
    /// A type exists. Nothing has run.
    TypeOnly,
    /// A unit test passed. One case, in one process, on one machine.
    UnitTest,
    /// A number was computed locally from inputs nobody external confirmed.
    Estimated,
    /// The fact survives a restart, proven offline against committed evidence.
    OfflineDurable,
    /// A real external system was exercised, under an explicit opt-in.
    OptInLive,
}

impl ClaimedLevel {
    pub const ALL: [Self; 5] = [
        Self::TypeOnly,
        Self::UnitTest,
        Self::Estimated,
        Self::OfflineDurable,
        Self::OptInLive,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TypeOnly => "type_only",
            Self::UnitTest => "unit_test",
            Self::Estimated => "estimated",
            Self::OfflineDurable => "offline_durable",
            Self::OptInLive => "opt_in_live",
        }
    }

    /// Does this level mean a number was *measured* rather than computed?
    ///
    /// `OfflineDurable` does not, on its own: a fact can be durable and still be an estimate. Only
    /// `OptInLive` implies somebody outside this system produced the number.
    const fn implies_measured(self) -> bool {
        matches!(self, Self::OptInLive)
    }
}

/// The request: a claim, and the evidence offered for it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionGateRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub claim_id: String,
    pub claimed_level: ClaimedLevel,
    /// The evidence contract, sealed by SC-35.
    pub evidence: EvidenceManifest,
    /// The provider's own receipt for the run, when one exists. Required to claim *measured*.
    pub provider_receipt_ref: Option<String>,
    /// The typed opt-in that authorises a live run. Required to claim *live*.
    pub opt_in_ref: Option<String>,
    /// Evidence that the fact was read back after a restart. Required to claim *durable*.
    pub restart_replay_ref: Option<String>,
    pub request_digest: String,
}

impl PromotionGateRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        claim_id: impl Into<String>,
        claimed_level: ClaimedLevel,
        evidence: EvidenceManifest,
        provider_receipt_ref: Option<String>,
        opt_in_ref: Option<String>,
        restart_replay_ref: Option<String>,
    ) -> Self {
        let mut value = Self {
            schema: PROMOTION_GATE_REQUEST_SCHEMA.to_owned(),
            version: PROMOTION_GATE_VERSION,
            claim_id: claim_id.into(),
            claimed_level,
            evidence,
            provider_receipt_ref,
            opt_in_ref,
            restart_replay_ref,
            request_digest: String::new(),
        };
        value.request_digest = value.digest();
        value
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "claim_id": self.claim_id,
            "claimed_level": self.claimed_level,
            "evidence_digest": self.evidence.manifest_digest,
            "provider_receipt_ref": self.provider_receipt_ref,
            "opt_in_ref": self.opt_in_ref,
            "restart_replay_ref": self.restart_replay_ref,
        }))
    }
}

/// The sealed decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionGateDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub claim_id: String,
    pub claimed_level: ClaimedLevel,
    /// The level the evidence actually reaches. Not a demotion, a fact.
    pub evidence_reaches: ClaimedLevel,
    pub promoted: bool,
    pub reason: String,
    pub evidence_digest: String,
    pub limitations: Vec<String>,
    pub decision_digest: String,
}

impl PromotionGateDecision {
    pub fn validate_against(&self, request: &PromotionGateRequest) -> Result<(), String> {
        if self.schema != PROMOTION_GATE_DECISION_SCHEMA
            || !self.version.is_compatible_with(&PROMOTION_GATE_VERSION)
            || self.claim_id != request.claim_id
            || self.claimed_level != request.claimed_level
            || self.evidence_digest != request.evidence.manifest_digest
        {
            return Err("promotion_decision_binding_invalid".to_owned());
        }
        if self.decision_digest != self.digest() {
            return Err("promotion_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "claim_id": self.claim_id,
            "claimed_level": self.claimed_level,
            "evidence_reaches": self.evidence_reaches,
            "promoted": self.promoted,
            "reason": self.reason,
            "evidence_digest": self.evidence_digest,
            "limitations": self.limitations,
        }))
    }
}

/// The highest level this evidence can carry.
///
/// Derived from the manifest's own proof level, so the two can never disagree.
fn evidence_reaches(manifest: &EvidenceManifest) -> ClaimedLevel {
    match manifest.proof_level {
        EvidenceProofLevel::Source => ClaimedLevel::TypeOnly,
        EvidenceProofLevel::LocalBehavior => ClaimedLevel::UnitTest,
        EvidenceProofLevel::Durable => ClaimedLevel::OfflineDurable,
        EvidenceProofLevel::Live => ClaimedLevel::OptInLive,
        // There is no physical rung on this ladder. A manifest that says `physical` is not
        // claiming a stronger version of the same thing; it is claiming a different one, and the
        // gate cannot represent it, so it is treated as reaching the top rather than being rounded
        // down silently.
        EvidenceProofLevel::Physical => ClaimedLevel::OptInLive,
    }
}

/// Decide one promotion.
///
/// The order is the card's order. The manifest is validated first because everything else is a
/// statement *about* it; then the recorded limitations, because a review that produced none did
/// not happen; then the evidence ceiling, which is the general rule; then the three specific
/// requirements, each of which is a precondition its own level carries and no other.
pub fn evaluate_promotion(
    request: &PromotionGateRequest,
) -> Result<PromotionGateDecision, String> {
    if request.schema != PROMOTION_GATE_REQUEST_SCHEMA
        || !request.version.is_compatible_with(&PROMOTION_GATE_VERSION)
    {
        return Err("promotion_request_header_invalid".to_owned());
    }
    if request.request_digest != request.digest() {
        return Err("promotion_request_digest_mismatch".to_owned());
    }
    if request.claim_id.trim().is_empty() {
        return Err("promotion_claim_id_required".to_owned());
    }
    // The SC-35 contract is the input, and it refuses its own malformed records first.
    request.evidence.validate()?;

    // A claim with no recorded limitation has not been reviewed.
    if request.evidence.limitations.is_empty() {
        return Err("promotion_limitations_unrecorded".to_owned());
    }

    let reaches = evidence_reaches(&request.evidence);
    if request.claimed_level > reaches {
        return Err("promotion_above_evidence_ceiling".to_owned());
    }

    // Each level carries its own precondition, and none of them is satisfied by another level.
    if request.claimed_level == ClaimedLevel::OptInLive {
        if request.provider_receipt_ref.is_none() {
            return Err("promotion_live_requires_provider_receipt".to_owned());
        }
        if request.opt_in_ref.is_none() {
            return Err("promotion_live_requires_opt_in".to_owned());
        }
    }
    if request.claimed_level.implies_measured() && request.provider_receipt_ref.is_none() {
        return Err("promotion_measured_requires_provider_receipt".to_owned());
    }
    if request.claimed_level == ClaimedLevel::OfflineDurable
        && request.restart_replay_ref.is_none()
    {
        return Err("promotion_durable_requires_restart_replay".to_owned());
    }

    let mut decision = PromotionGateDecision {
        schema: PROMOTION_GATE_DECISION_SCHEMA.to_owned(),
        version: PROMOTION_GATE_VERSION,
        claim_id: request.claim_id.clone(),
        claimed_level: request.claimed_level,
        evidence_reaches: reaches,
        promoted: true,
        reason: String::new(),
        evidence_digest: request.evidence.manifest_digest.clone(),
        limitations: request.evidence.limitations.clone(),
        decision_digest: String::new(),
    };
    decision.decision_digest = decision.digest();
    decision.validate_against(request)?;
    Ok(decision)
}
