//! SC-36 protocol parity across CLI, Workbench, Web, Desktop and the daemon.
//!
//! UI-31 already compares four UI surfaces on command, cursor, disposition, retry and receipt
//! identity. SC-36 asks a different question, and the difference is the whole point of this
//! module: **a surface that renders correctly while deciding for itself is still broken.** Three
//! failures in particular are invisible to a trace comparison that only looks at agreement:
//!
//! - a surface that offers an approve button and applies it without a server-issued approval
//!   reference, so the approval exists only in that surface;
//! - a surface that computes allow/deny locally instead of rendering the server's decision;
//! - a surface that redacts differently, so the same secret is visible in one and hidden in another.
//!
//! A fourth is the one this repository keeps meeting: a disposition of `unknown` rendered as
//! "denied" in one surface and "done" in another. Both are lies, and they are different lies.
//!
//! # Why this is a new enum and not a new variant on `ParitySurface`
//!
//! `ParitySurface` has four variants and `REQUIRED_SURFACES` is a fixed array; adding a fifth
//! would silently change what UI-31 accepts, and a parity check that changes meaning when you are
//! not looking is exactly the failure this card is about. The daemon is also not a peer surface —
//! it is the one surface that *issued* the decision the others render — so it is modelled as the
//! reference the others are compared against rather than as a fifth equal.
//!
//! # Read-only
//!
//! No transport, no command, no effect. It compares supplied observations and refuses the ones
//! that describe a surface acting on its own authority.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const PROTOCOL_PARITY_SCHEMA: &str = "kiana.protocol-parity-observation.v1";
pub const PROTOCOL_PARITY_REPORT_SCHEMA: &str = "kiana.protocol-parity-report.v1";
pub const MAX_PROTOCOL_PARITY_LIMITATIONS: usize = 32;

/// The four UI surfaces plus the daemon.
///
/// `Daemon` is the reference. Every other surface must agree with it, and the report says which
/// direction the comparison ran, so a reader cannot mistake "the daemon agrees with the UI" for
/// "the UI agrees with the daemon".
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolSurface {
    Cli,
    Workbench,
    Web,
    Desktop,
    Daemon,
}

impl ProtocolSurface {
    pub const ALL: [Self; 5] = [
        Self::Cli,
        Self::Workbench,
        Self::Web,
        Self::Desktop,
        Self::Daemon,
    ];

    pub const fn is_daemon(self) -> bool {
        matches!(self, Self::Daemon)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Workbench => "workbench",
            Self::Web => "web",
            Self::Desktop => "desktop",
            Self::Daemon => "daemon",
        }
    }
}

/// Where a surface's allow/deny came from.
///
/// This is the field that makes "locally computed permission" detectable at all. A trace that only
/// records the outcome cannot tell a surface that rendered the server's answer from one that worked
/// it out itself, and the second one is the bug.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    /// The decision came from the server and the surface rendered it. The only acceptable value.
    ServerRendered,
    /// The surface decided for itself. Refused: a UI that computes permission is a second policy
    /// engine, and it will disagree with the first one eventually.
    LocallyComputed,
}

/// How a surface presented a disposition.
///
/// Only `NotApplicable` and `RenderedAsIs` are acceptable. The other two exist so that a fixture
/// can *say* what a broken surface did, which is what makes the refusal assertable instead of
/// merely imaginable.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPresentation {
    /// This surface shows no approval control. Correct unless the daemon issued an approval.
    NotApplicable,
    /// The control is shown, disabled, waiting on a decision. Correct while an approval is pending.
    RenderedPending,
    /// The control was applied in this surface without a server-issued reference. Refused.
    AppliedLocally,
    /// The control is shown as already decided. Refused unless it names the server's reference.
    RenderedDecided,
}

/// One surface's observation of one command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolObservation {
    pub schema: String,
    pub surface: ProtocolSurface,
    pub command_id: String,
    pub operation: String,
    /// `accepted` | `applied` | `rejected` | `unknown` — the same vocabulary UI-31 uses, kept so a
    /// fixture can be compared across both checks.
    pub disposition: String,
    pub retry: String,
    pub cursor_epoch: String,
    pub cursor_sequence: u64,
    pub revision: u64,
    pub receipt_digest: Option<String>,
    pub error_code: Option<String>,
    pub decision_source: DecisionSource,
    pub approval: ApprovalPresentation,
    /// The server-issued approval reference, when there is one. Required whenever `approval` is
    /// `AppliedLocally` or `RenderedDecided` — which is exactly what makes those two refusable.
    pub approval_reference: Option<String>,
    /// Digest of the redaction profile this surface applied. Must be identical everywhere.
    pub redaction_profile_digest: String,
    /// How many sensitive fields this surface still shows. Must be zero.
    pub sensitive_field_count: u32,
    pub limitations: Vec<String>,
}

impl ProtocolObservation {
    pub fn validate(&self) -> Result<(), ProtocolParityError> {
        if self.schema != PROTOCOL_PARITY_SCHEMA {
            return Err(ProtocolParityError::ObservationInvalid("schema"));
        }
        if self.command_id.trim().is_empty() || self.command_id.len() > 256 {
            return Err(ProtocolParityError::ObservationInvalid("command_id"));
        }
        if self.operation.trim().is_empty() || self.operation.len() > 256 {
            return Err(ProtocolParityError::ObservationInvalid("operation"));
        }
        if !matches!(
            self.disposition.as_str(),
            "accepted" | "applied" | "rejected" | "unknown"
        ) {
            return Err(ProtocolParityError::ObservationInvalid("disposition"));
        }
        if !matches!(
            self.retry.as_str(),
            "query_original" | "safe_retry" | "do_not_retry"
        ) {
            return Err(ProtocolParityError::ObservationInvalid("retry"));
        }
        if self.cursor_epoch.trim().is_empty()
            || self.cursor_epoch.len() > 256
            || self.cursor_sequence == 0
        {
            return Err(ProtocolParityError::ObservationInvalid("cursor"));
        }
        if self.revision == 0 {
            return Err(ProtocolParityError::ObservationInvalid("revision"));
        }
        if self.limitations.len() > MAX_PROTOCOL_PARITY_LIMITATIONS {
            return Err(ProtocolParityError::ObservationInvalid("limitations"));
        }
        if self.sensitive_field_count > 0 {
            return Err(ProtocolParityError::SensitiveFieldVisible);
        }
        if !valid_digest(&self.redaction_profile_digest) {
            return Err(ProtocolParityError::ObservationInvalid("redaction_profile"));
        }
        if self
            .receipt_digest
            .as_deref()
            .is_some_and(|value| !valid_digest(value))
        {
            return Err(ProtocolParityError::ObservationInvalid("receipt"));
        }
        // Applied without a receipt is the same failure UI-31 refuses, restated here so this
        // comparator is usable on its own.
        if self.disposition == "applied"
            && self
                .receipt_digest
                .as_deref()
                .is_none_or(|value| !valid_digest(value))
        {
            return Err(ProtocolParityError::ReceiptMissing);
        }
        if self.error_code.as_deref().is_some_and(|value| {
            value.trim().is_empty() || value.len() > 128 || value.contains('\0')
        }) {
            return Err(ProtocolParityError::ObservationInvalid("error_code"));
        }
        // The three authority rules, checked per surface before anything is compared. Deciding
        // them here rather than in the comparator means a bad surface is refused even when it
        // happens to agree with the daemon on every other field.
        if self.decision_source == DecisionSource::LocallyComputed {
            return Err(ProtocolParityError::LocallyComputedDecision);
        }
        if self.approval == ApprovalPresentation::AppliedLocally {
            return Err(ProtocolParityError::ApprovalAppliedLocally);
        }
        if self.approval == ApprovalPresentation::RenderedDecided
            && self
                .approval_reference
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ProtocolParityError::ApprovalReferenceMissing);
        }
        // An `unknown` rendered as a settled outcome is a lie in whichever direction it points.
        if self.disposition == "unknown"
            && self.receipt_digest.is_some()
            && self.approval != ApprovalPresentation::NotApplicable
        {
            return Err(ProtocolParityError::UnknownRenderedAsSettled);
        }
        Ok(())
    }
}

/// The comparison result. It names the reference surface so the direction of the comparison is
/// never ambiguous.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolParityReport {
    pub schema: String,
    pub command_id: String,
    pub operation: String,
    pub disposition: String,
    pub retry: String,
    pub cursor_epoch: String,
    pub cursor_sequence: u64,
    pub revision: u64,
    pub receipt_digest: Option<String>,
    /// Always `daemon`. Recorded rather than implied.
    pub reference_surface: ProtocolSurface,
    pub surfaces: Vec<ProtocolSurface>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ProtocolParityError {
    #[error("protocol_parity_observation_invalid:{0}")]
    ObservationInvalid(&'static str),
    #[error("protocol_parity_surfaces_missing")]
    SurfacesMissing,
    #[error("protocol_parity_duplicate_surface")]
    DuplicateSurface,
    #[error("protocol_parity_reference_missing")]
    ReferenceMissing,
    #[error("protocol_parity_mismatch:{0}")]
    Mismatch(&'static str),
    #[error("protocol_parity_locally_computed_decision")]
    LocallyComputedDecision,
    #[error("protocol_parity_approval_applied_locally")]
    ApprovalAppliedLocally,
    #[error("protocol_parity_approval_reference_missing")]
    ApprovalReferenceMissing,
    #[error("protocol_parity_approval_reference_mismatch")]
    ApprovalReferenceMismatch,
    #[error("protocol_parity_redaction_profile_mismatch")]
    RedactionProfileMismatch,
    #[error("protocol_parity_unknown_rendered_as_settled")]
    UnknownRenderedAsSettled,
    #[error("protocol_parity_sensitive_field_visible")]
    SensitiveFieldVisible,
    #[error("protocol_parity_receipt_missing")]
    ReceiptMissing,
    #[error("protocol_parity_limitations_required")]
    LimitationsRequired,
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Compare four UI surfaces against the daemon's own record of the same command.
///
/// 【作用】
/// Refuse any surface that decided for itself, applied an approval locally, redacted differently,
/// or turned an `unknown` into a settled outcome — and then require the survivors to agree with
/// the daemon on disposition, retry, cursor, revision and receipt.
///
/// 【调用者】
/// A conformance or release gate. It is not wired into a transport, and it starts nothing.
///
/// 【输入】
/// - `observations`: every surface's record of one command, including the daemon's.
///
/// 【输出】
/// `Ok(report)` naming the surfaces that agreed, or the first rule that was broken.
///
/// 【核心流程】
/// 1. Validate each observation. This is where the three authority rules fire, per surface.
/// 2. Require all five surfaces, exactly once each. Four is a coverage gap, six is a fabrication.
/// 3. Take the daemon observation as the reference.
/// 4. Require the redaction profile digest to be identical everywhere. A surface that redacts
///    less is not "more useful", it is leaking.
/// 5. Require the settled fields to match the reference, one field at a time so the error names
///    which one drifted.
/// 6. Require at least one limitation on the report itself. A parity check with no limitations
///    is claiming it saw everything.
pub fn compare_protocol_parity(
    observations: &[ProtocolObservation],
) -> Result<ProtocolParityReport, ProtocolParityError> {
    for observation in observations {
        observation.validate()?;
    }
    let mut seen = BTreeSet::new();
    for observation in observations {
        if !seen.insert(observation.surface) {
            return Err(ProtocolParityError::DuplicateSurface);
        }
    }
    for required in ProtocolSurface::ALL {
        if !seen.contains(&required) {
            return Err(ProtocolParityError::SurfacesMissing);
        }
    }
    let reference = observations
        .iter()
        .find(|observation| observation.surface.is_daemon())
        .ok_or(ProtocolParityError::ReferenceMissing)?;

    let redaction = &reference.redaction_profile_digest;
    for observation in observations {
        if &observation.redaction_profile_digest != redaction {
            return Err(ProtocolParityError::RedactionProfileMismatch);
        }
    }
    for observation in observations {
        if observation.surface.is_daemon() {
            continue;
        }
        if observation.disposition != reference.disposition {
            return Err(ProtocolParityError::Mismatch("disposition"));
        }
        if observation.retry != reference.retry {
            return Err(ProtocolParityError::Mismatch("retry"));
        }
        if observation.cursor_epoch != reference.cursor_epoch
            || observation.cursor_sequence != reference.cursor_sequence
        {
            return Err(ProtocolParityError::Mismatch("cursor"));
        }
        if observation.revision != reference.revision {
            return Err(ProtocolParityError::Mismatch("revision"));
        }
        if observation.receipt_digest != reference.receipt_digest {
            return Err(ProtocolParityError::Mismatch("receipt"));
        }
        if observation.error_code != reference.error_code {
            return Err(ProtocolParityError::Mismatch("error_code"));
        }
        // Two surfaces may show the same approval only if they name the same server-issued
        // reference. Agreeing on "approved" without agreeing on *which* approval is not agreement.
        if observation.approval_reference != reference.approval_reference {
            return Err(ProtocolParityError::ApprovalReferenceMismatch);
        }
    }
    let limitations = observations
        .iter()
        .flat_map(|observation| observation.limitations.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if limitations.is_empty() {
        return Err(ProtocolParityError::LimitationsRequired);
    }
    Ok(ProtocolParityReport {
        schema: PROTOCOL_PARITY_REPORT_SCHEMA.to_owned(),
        command_id: reference.command_id.clone(),
        operation: reference.operation.clone(),
        disposition: reference.disposition.clone(),
        retry: reference.retry.clone(),
        cursor_epoch: reference.cursor_epoch.clone(),
        cursor_sequence: reference.cursor_sequence,
        revision: reference.revision,
        receipt_digest: reference.receipt_digest.clone(),
        reference_surface: reference.surface,
        surfaces: ProtocolSurface::ALL.to_vec(),
        limitations,
    })
}
