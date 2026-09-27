//! SC-32 compare-and-swap, monotonic cursor and rebuild-equivalence gate for the audit projection.
//!
//! A projector is a cache of audit facts. A cache that can move backwards is worse than no cache,
//! because it un-answers questions that have already been answered: a caller re-reads a window,
//! sees an audit row disappear, and cannot tell a deletion from a rollback. This module therefore
//! admits exactly three outcomes for a publish attempt, and nothing else:
//!
//! ```text
//! Advanced     a new head with generation + 1 and a strictly greater source cursor
//! ExactReplay  the claim's target is already the published head, so it changes nothing
//! Rejected     every other shape, each with one stable reason
//! ```
//!
//! The CAS rule is the load-bearing one: a claim carries the generation it read, and only a claim
//! whose `expected_generation` is the *currently published* generation may advance it. Two
//! projectors that read the same head therefore cannot both win, and the loser is told the
//! generation it lost to rather than silently overwriting a newer head.
//!
//! This is a read-only decision over supplied digests and counters. It appends nothing, stores
//! nothing, calls no adapter and no port, and it cannot tell a truthful projector from a lying
//! one: it proves that a head moved the way the claim says it moved, not that the folded state
//! behind the digest is correct. The reducer that produces the state, and the rebuild that proves
//! two paths agree about history, live in `kiana-query/src/audit_projector.rs`.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const AUDIT_PROJECTION_HEAD_SCHEMA: &str = "kiana.audit-projection-head.v1";
pub const AUDIT_PROJECTION_CLAIM_SCHEMA: &str = "kiana.audit-projection-claim.v1";
pub const AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA: &str = "kiana.audit-projection-commit-report.v1";
pub const AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA: &str = "kiana.audit-projection-rebuild-proof.v1";
pub const AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA: &str = "kiana.audit-projection-rebuild-report.v1";
pub const AUDIT_PROJECTION_COMMIT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_AUDIT_PROJECTION_TEXT: usize = 256;

/// The published head of one audit projection.
///
/// The head is the whole durable claim: which projector owns it, which generation it is, how far
/// into the EventLog it has consumed, and the digest of exactly that state. Everything else about
/// the read model is derivable, so nothing is duplicated here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionHead {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    pub head_digest: String,
}

impl AuditProjectionHead {
    pub fn new(
        projector: impl Into<String>,
        generation: u64,
        source_cursor: EventCursor,
        state_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut head = Self {
            schema: AUDIT_PROJECTION_HEAD_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: projector.into(),
            generation,
            source_cursor,
            state_digest: state_digest.into(),
            head_digest: String::new(),
        };
        head.head_digest = head.digest();
        head.validate()?;
        Ok(head)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTION_HEAD_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.generation == 0
            || self.source_cursor == 0
        {
            return Err("audit_projection_head_header_invalid".to_owned());
        }
        safe_text(&self.projector, "audit_projection_head_projector")?;
        valid_digest(&self.state_digest, "audit_projection_head_state_digest")?;
        valid_digest(&self.head_digest, "audit_projection_head_digest")?;
        if self.head_digest != self.digest() {
            return Err("audit_projection_head_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
        }))
    }
}

/// One attempt to publish a new head for an audit projection.
///
/// `expected_generation` is the CAS token: it is the generation the projector read, not the one it
/// wants. `previous_state_digest` is the same idea for the folded state, so a projector that was
/// working from a stale or foreign state cannot publish on top of a head it never actually saw.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionClaim {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub expected_generation: u64,
    pub new_generation: u64,
    pub previous_source_cursor: EventCursor,
    pub new_source_cursor: EventCursor,
    pub previous_state_digest: String,
    pub new_state_digest: String,
    /// `None` for an ordinary contiguous advance. `Some(previous_source_cursor)` when the
    /// projector re-folded from the published head to close a gap. `Some(0)` for a full rebuild
    /// from the beginning of the log. Any other origin is a claim about a history nobody verified.
    pub rebuild_from_cursor: Option<EventCursor>,
    pub claim_digest: String,
}

impl AuditProjectionClaim {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        projector: &str,
        expected_generation: u64,
        new_generation: u64,
        previous_source_cursor: EventCursor,
        new_source_cursor: EventCursor,
        previous_state_digest: &str,
        new_state_digest: &str,
        rebuild_from_cursor: Option<EventCursor>,
    ) -> Result<Self, String> {
        let mut claim = Self {
            schema: AUDIT_PROJECTION_CLAIM_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: projector.to_owned(),
            expected_generation,
            new_generation,
            previous_source_cursor,
            new_source_cursor,
            previous_state_digest: previous_state_digest.to_owned(),
            new_state_digest: new_state_digest.to_owned(),
            rebuild_from_cursor,
            claim_digest: String::new(),
        };
        claim.claim_digest = claim.digest();
        claim.validate()?;
        Ok(claim)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTION_CLAIM_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.expected_generation == 0
            || self.new_generation == 0
            || self.previous_source_cursor == 0
            || self.new_source_cursor == 0
        {
            return Err("audit_projection_claim_header_invalid".to_owned());
        }
        safe_text(&self.projector, "audit_projection_claim_projector")?;
        valid_digest(
            &self.previous_state_digest,
            "audit_projection_claim_previous_state_digest",
        )?;
        valid_digest(
            &self.new_state_digest,
            "audit_projection_claim_new_state_digest",
        )?;
        valid_digest(&self.claim_digest, "audit_projection_claim_digest")?;
        if self.claim_digest != self.digest() {
            return Err("audit_projection_claim_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "expected_generation": self.expected_generation,
            "new_generation": self.new_generation,
            "previous_source_cursor": self.previous_source_cursor,
            "new_source_cursor": self.new_source_cursor,
            "previous_state_digest": self.previous_state_digest,
            "new_state_digest": self.new_state_digest,
            "rebuild_from_cursor": self.rebuild_from_cursor,
        }))
    }
}

/// The outcome of one publish attempt. `ExactReplay` is the only shape in which the source cursor
/// is allowed not to strictly increase, and it is a no-op rather than an advance.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditProjectionCommitStatus {
    Advanced,
    ExactReplay,
    Rejected,
}

impl AuditProjectionCommitStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Advanced => "advanced",
            Self::ExactReplay => "exact_replay",
            Self::Rejected => "rejected",
        }
    }
}

/// The ordered decision for one publish attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionCommitReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub status: AuditProjectionCommitStatus,
    /// The generation this attempt leaves behind: the claim target on `Advanced`, the published
    /// generation on `ExactReplay` and on `Rejected`.
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    /// Stable denial code. Empty unless the status is `Rejected`.
    pub reason: String,
    pub remediation: String,
    pub head_digest: String,
    pub report_digest: String,
}

impl AuditProjectionCommitReport {
    pub fn evaluate(
        head: &AuditProjectionHead,
        claim: &AuditProjectionClaim,
    ) -> Result<Self, String> {
        head.validate()?;
        claim.validate()?;
        let (status, generation, source_cursor, state_digest, reason, remediation) =
            derive(head, claim);
        let mut report = Self {
            schema: AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: claim.projector.clone(),
            status,
            generation,
            source_cursor,
            state_digest,
            reason,
            remediation,
            head_digest: head.head_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(head, claim)?;
        Ok(report)
    }

    /// Re-derive every field from the same head and claim, so an edited status, generation, cursor,
    /// digest or reason cannot be published as if it had been decided here.
    pub fn validate_against(
        &self,
        head: &AuditProjectionHead,
        claim: &AuditProjectionClaim,
    ) -> Result<(), String> {
        head.validate()?;
        claim.validate()?;
        let (status, generation, source_cursor, state_digest, reason, remediation) =
            derive(head, claim);
        if self.schema != AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.projector != claim.projector
            || self.status != status
            || self.generation != generation
            || self.source_cursor != source_cursor
            || self.state_digest != state_digest
            || self.reason != reason
            || self.remediation != remediation
            || self.head_digest != head.head_digest
        {
            return Err("audit_projection_commit_binding_invalid".to_owned());
        }
        // Only a rejection carries a reason, and every rejection must carry one. A "rejected"
        // report with an empty reason would hide which rule fired.
        if (self.status == AuditProjectionCommitStatus::Rejected) != !self.reason.is_empty() {
            return Err("audit_projection_commit_reason_state_mismatch".to_owned());
        }
        if self.status != AuditProjectionCommitStatus::Rejected && !self.remediation.is_empty() {
            return Err("audit_projection_commit_remediation_with_outcome".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "audit_projection_commit_reason")?;
        }
        if !self.remediation.is_empty() {
            safe_text(&self.remediation, "audit_projection_commit_remediation")?;
        }
        valid_digest(&self.state_digest, "audit_projection_commit_state_digest")?;
        valid_digest(&self.head_digest, "audit_projection_commit_head_digest")?;
        valid_digest(&self.report_digest, "audit_projection_commit_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("audit_projection_commit_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether the published head may now be considered moved.
    pub fn advanced(&self) -> bool {
        self.status == AuditProjectionCommitStatus::Advanced
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "status": self.status,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
            "reason": self.reason,
            "remediation": self.remediation,
            "head_digest": self.head_digest,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore the
/// same for the same head and claim.
///
/// The order is not arbitrary. An exact replay is checked first because it is the one shape that
/// is allowed to re-state a cursor without moving it, and a claim that re-states a *different*
/// cursor does not match it and falls through to the monotonicity rules. The projector identity
/// comes next, because a claim filed against another projector's head is not a race at all. The
/// CAS token then settles the race. The state digest is compared before the cursors, because a
/// projector that was folding a state the head never published has nothing to say about which
/// cursor it reached. Finally the cursor rules run, so "moved backwards" and "jumped forwards"
/// can never both be reported for the same claim.
fn derive(
    head: &AuditProjectionHead,
    claim: &AuditProjectionClaim,
) -> (
    AuditProjectionCommitStatus,
    u64,
    EventCursor,
    String,
    String,
    String,
) {
    // Exact replay: the claim's target is already the published head. Narrow on purpose - the
    // generation, the cursor and the folded state must all match, so a claim that would move the
    // head backwards or to a different state can never pass as a replay.
    if claim.new_generation == head.generation
        && claim.new_source_cursor == head.source_cursor
        && claim.new_state_digest == head.state_digest
    {
        return (
            AuditProjectionCommitStatus::ExactReplay,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            String::new(),
            String::new(),
        );
    }
    if claim.projector != head.projector {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_projector_mismatch".to_owned(),
            "publish to the head that owns this projector name, not to another projector's head"
                .to_owned(),
        );
    }
    if claim.expected_generation != head.generation {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_generation_cas_conflict".to_owned(),
            format!(
                "re-read the head: it is at generation {}, this attempt expected {}",
                head.generation, claim.expected_generation
            ),
        );
    }
    if claim.new_generation != head.generation.saturating_add(1) {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_generation_not_monotonic".to_owned(),
            format!(
                "advance the head to exactly generation {}, not {}",
                head.generation.saturating_add(1),
                claim.new_generation
            ),
        );
    }
    if claim.previous_state_digest != head.state_digest {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_state_digest_drift".to_owned(),
            "fold from the published state digest; a claim built on another state cannot be committed"
                .to_owned(),
        );
    }
    if claim.new_source_cursor < claim.previous_source_cursor {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_cursor_regression".to_owned(),
            format!(
                "a published projection may not move back from cursor {} to {}",
                claim.previous_source_cursor, claim.new_source_cursor
            ),
        );
    }
    if claim.new_source_cursor == claim.previous_source_cursor {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_cursor_not_advanced".to_owned(),
            format!(
                "cursor {} was already published with a different state; re-read the tail instead of re-stating it",
                claim.previous_source_cursor
            ),
        );
    }
    if let Some(origin) = claim.rebuild_from_cursor {
        if origin != 0 && origin != claim.previous_source_cursor {
            return (
                AuditProjectionCommitStatus::Rejected,
                head.generation,
                head.source_cursor,
                head.state_digest.clone(),
                "audit_projection_rebuild_origin_mismatch".to_owned(),
                format!(
                    "a rebuild starts at cursor 0 or at the published cursor {}, not {}",
                    claim.previous_source_cursor, origin
                ),
            );
        }
    }
    let contiguous = claim.new_source_cursor == claim.previous_source_cursor.saturating_add(1);
    if !contiguous && claim.rebuild_from_cursor.is_none() {
        return (
            AuditProjectionCommitStatus::Rejected,
            head.generation,
            head.source_cursor,
            head.state_digest.clone(),
            "audit_projection_cursor_gap".to_owned(),
            format!(
                "cursor {} to {} skips committed facts; re-fold the missing range or declare a rebuild",
                claim.previous_source_cursor, claim.new_source_cursor
            ),
        );
    }
    (
        AuditProjectionCommitStatus::Advanced,
        claim.new_generation,
        claim.new_source_cursor,
        claim.new_state_digest.clone(),
        String::new(),
        String::new(),
    )
}

/// One side-by-side comparison of an incremental replay and a from-scratch rebuild.
///
/// Both sides are reported as digests, never as state, because the question this answers is
/// whether the two paths *agree about history*, not whether either one is individually plausible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionRebuildProof {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub incremental_generation: u64,
    pub incremental_source_cursor: EventCursor,
    pub incremental_state_digest: String,
    /// Digest over the sorted, deduplicated source event identities the incremental path consumed.
    pub incremental_source_event_digest: String,
    pub rebuilt_source_cursor: EventCursor,
    pub rebuilt_state_digest: String,
    /// Digest over the sorted, deduplicated source event identities the rebuild consumed.
    pub rebuilt_source_event_digest: String,
    pub proof_digest: String,
}

impl AuditProjectionRebuildProof {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        projector: &str,
        incremental_generation: u64,
        incremental_source_cursor: EventCursor,
        incremental_state_digest: &str,
        incremental_source_event_digest: &str,
        rebuilt_source_cursor: EventCursor,
        rebuilt_state_digest: &str,
        rebuilt_source_event_digest: &str,
    ) -> Result<Self, String> {
        let mut proof = Self {
            schema: AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: projector.to_owned(),
            incremental_generation,
            incremental_source_cursor,
            incremental_state_digest: incremental_state_digest.to_owned(),
            incremental_source_event_digest: incremental_source_event_digest.to_owned(),
            rebuilt_source_cursor,
            rebuilt_state_digest: rebuilt_state_digest.to_owned(),
            rebuilt_source_event_digest: rebuilt_source_event_digest.to_owned(),
            proof_digest: String::new(),
        };
        proof.proof_digest = proof.digest();
        proof.validate()?;
        Ok(proof)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.incremental_generation == 0
            || self.incremental_source_cursor == 0
            || self.rebuilt_source_cursor == 0
        {
            return Err("audit_projection_rebuild_proof_header_invalid".to_owned());
        }
        safe_text(&self.projector, "audit_projection_rebuild_proof_projector")?;
        for (value, field) in [
            (
                &self.incremental_state_digest,
                "audit_projection_rebuild_incremental_state_digest",
            ),
            (
                &self.incremental_source_event_digest,
                "audit_projection_rebuild_incremental_source_event_digest",
            ),
            (
                &self.rebuilt_state_digest,
                "audit_projection_rebuild_state_digest",
            ),
            (
                &self.rebuilt_source_event_digest,
                "audit_projection_rebuild_source_event_digest",
            ),
        ] {
            valid_digest(value, field)?;
        }
        valid_digest(&self.proof_digest, "audit_projection_rebuild_proof_digest")?;
        if self.proof_digest != self.digest() {
            return Err("audit_projection_rebuild_proof_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "incremental_generation": self.incremental_generation,
            "incremental_source_cursor": self.incremental_source_cursor,
            "incremental_state_digest": self.incremental_state_digest,
            "incremental_source_event_digest": self.incremental_source_event_digest,
            "rebuilt_source_cursor": self.rebuilt_source_cursor,
            "rebuilt_state_digest": self.rebuilt_state_digest,
            "rebuilt_source_event_digest": self.rebuilt_source_event_digest,
        }))
    }
}

/// Whether a rebuild agrees with the incremental replay it is meant to reproduce.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditProjectionRebuildStatus {
    Converged,
    Divergent,
}

impl AuditProjectionRebuildStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Converged => "converged",
            Self::Divergent => "divergent",
        }
    }
}

/// The ordered decision for one rebuild comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionRebuildReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub status: AuditProjectionRebuildStatus,
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    pub reason: String,
    pub remediation: String,
    pub proof_digest: String,
    pub report_digest: String,
}

impl AuditProjectionRebuildReport {
    pub fn evaluate(
        head: &AuditProjectionHead,
        proof: &AuditProjectionRebuildProof,
    ) -> Result<Self, String> {
        head.validate()?;
        proof.validate()?;
        let (status, reason, remediation) = derive_rebuild(head, proof);
        let mut report = Self {
            schema: AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA.to_owned(),
            version: AUDIT_PROJECTION_COMMIT_VERSION,
            projector: proof.projector.clone(),
            status,
            generation: head.generation,
            source_cursor: head.source_cursor,
            state_digest: head.state_digest.clone(),
            reason,
            remediation,
            proof_digest: proof.proof_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(head, proof)?;
        Ok(report)
    }

    pub fn validate_against(
        &self,
        head: &AuditProjectionHead,
        proof: &AuditProjectionRebuildProof,
    ) -> Result<(), String> {
        head.validate()?;
        proof.validate()?;
        let (status, reason, remediation) = derive_rebuild(head, proof);
        if self.schema != AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&AUDIT_PROJECTION_COMMIT_VERSION)
            || self.projector != proof.projector
            || self.status != status
            || self.generation != head.generation
            || self.source_cursor != head.source_cursor
            || self.state_digest != head.state_digest
            || self.reason != reason
            || self.remediation != remediation
            || self.proof_digest != proof.proof_digest
        {
            return Err("audit_projection_rebuild_binding_invalid".to_owned());
        }
        if (self.status == AuditProjectionRebuildStatus::Divergent) != !self.reason.is_empty() {
            return Err("audit_projection_rebuild_reason_state_mismatch".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "audit_projection_rebuild_reason")?;
        }
        if !self.remediation.is_empty() {
            safe_text(&self.remediation, "audit_projection_rebuild_remediation")?;
        }
        valid_digest(
            &self.state_digest,
            "audit_projection_rebuild_report_state_digest",
        )?;
        valid_digest(
            &self.report_digest,
            "audit_projection_rebuild_report_digest",
        )?;
        if self.report_digest != self.digest() {
            return Err("audit_projection_rebuild_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether the two paths may be treated as telling the same history.
    pub fn converged(&self) -> bool {
        self.status == AuditProjectionRebuildStatus::Converged
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "status": self.status,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
            "reason": self.reason,
            "remediation": self.remediation,
            "proof_digest": self.proof_digest,
        }))
    }
}

/// Decide in a fixed order: identity, then how the incremental side was anchored to the head, then
/// whether the rebuild consumed the same facts, then whether it reached the same point, and only
/// last whether the folded state agrees. A divergence in the folded state is the expensive case to
/// investigate, so it is never reported for a rebuild that read different facts or a different
/// head: those are cheaper and more fundamental answers.
fn derive_rebuild(
    head: &AuditProjectionHead,
    proof: &AuditProjectionRebuildProof,
) -> (AuditProjectionRebuildStatus, String, String) {
    if proof.projector != head.projector {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_projector_mismatch".to_owned(),
            "compare a rebuild against the head of the same projector, not another projector's"
                .to_owned(),
        );
    }
    if proof.incremental_generation != head.generation {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_generation_mismatch".to_owned(),
            format!(
                "the incremental side was taken at generation {}, the head is at {}",
                proof.incremental_generation, head.generation
            ),
        );
    }
    if proof.incremental_source_cursor != head.source_cursor {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_incremental_cursor_mismatch".to_owned(),
            format!(
                "the incremental side was taken at cursor {}, the head is at {}",
                proof.incremental_source_cursor, head.source_cursor
            ),
        );
    }
    if proof.incremental_state_digest != head.state_digest {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_incremental_drift".to_owned(),
            "take the incremental side from the published head; a drifted base proves nothing"
                .to_owned(),
        );
    }
    if proof.rebuilt_source_event_digest != proof.incremental_source_event_digest {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_source_identity_divergent".to_owned(),
            "rebuild over the same committed source event identities before comparing state"
                .to_owned(),
        );
    }
    if proof.rebuilt_source_cursor != head.source_cursor {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_cursor_not_reaching_head".to_owned(),
            format!(
                "rebuild through cursor {} to match the published cursor",
                head.source_cursor
            ),
        );
    }
    if proof.rebuilt_state_digest != head.state_digest {
        return (
            AuditProjectionRebuildStatus::Divergent,
            "audit_projection_rebuild_state_divergent".to_owned(),
            "quarantine the projection and re-fold both paths; they disagree about history"
                .to_owned(),
        );
    }
    (
        AuditProjectionRebuildStatus::Converged,
        String::new(),
        String::new(),
    )
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_AUDIT_PROJECTION_TEXT
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
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
