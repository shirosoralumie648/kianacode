//! BQ-21 ControlPlane-side restart/recovery admission.
//!
//! The Domain module owns the single decision rule for restart, continue, replay and
//! reconciliation. This file is the authority boundary a restarted caller must pass before it may
//! touch an in-flight reservation, an uncertain attempt or a settled attempt:
//!
//! ```text
//! committed attempt snapshot + server-owned boot fence
//!        -> caller assertion (BillingRecoveryRequest)
//!        -> 【this file】BillingRecoveryAdmission::authorize_*
//!        -> one accepted BillingRecoveryPlan / reconciliation plan
//!        -> caller persists it through the existing ControlPlane/EventLog transaction
//! ```
//!
//! The admission never re-implements the epoch, cursor, accumulation or settlement rules: it
//! delegates to `kiana-domain` and only classifies the refusal so an entrypoint can report a
//! stable reason. It never dispatches a provider call, appends an EventLog fact, releases a
//! reservation or mutates a billing ledger; `admit` records a decision in the caller-owned
//! append-only `BillingRecoveryLedger`, which refuses a second settlement for one attempt.

use kiana_domain::{
    plan_billing_recovery as plan_domain_billing_recovery,
    reconcile_attempt as reconcile_domain_attempt,
    validate_billing_recovery as validate_domain_billing_recovery, BillingAttemptSnapshot,
    BillingBootFence, BillingReconciliation, BillingRecoveryApplyOutcome, BillingRecoveryFact,
    BillingRecoveryLedger, BillingRecoveryPlan, BillingRecoveryRequest,
};

/// Re-export the domain recovery validator under the crate-facing name `kiana-core` exposes.
pub use kiana_domain::validate_billing_recovery;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
/// Stable refusal classification for restart/recovery admission.
///
/// `SnapshotInvalid`/`FenceInvalid` are shape failures of server-owned inputs, `Refused` carries
/// the Domain policy code (stale epoch, terminal attempt, accumulation reset, missing evidence,
/// ...) and `ReplayConflict` groups the ledger rules that keep one attempt settled at most once.
pub enum BillingRecoveryAdmissionError {
    #[error("billing_recovery_fact_invalid:{0}")]
    FactInvalid(String),
    #[error("billing_recovery_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
    #[error("billing_recovery_boot_fence_invalid:{0}")]
    FenceInvalid(String),
    #[error("billing_recovery_refused:{0}")]
    Refused(String),
    #[error("billing_recovery_replay_conflict:{0}")]
    ReplayConflict(String),
}

/// Stateless BQ-21 admission facade.
///
/// It keeps the decision in Core/ControlPlane instead of letting an entrypoint or adapter decide
/// on its own whether a restarted attempt may continue, be re-dispatched or be settled again.
pub struct BillingRecoveryAdmission;

impl BillingRecoveryAdmission {
    /// Authorize a restart, continue or replay assertion against the committed snapshot and the
    /// current boot fence.
    ///
    /// A completed attempt is never re-dispatched, a `Continue` never resets accumulated usage and
    /// an unknown attempt never becomes a success: the accepted plan is `Paused` or
    /// `NeedsReconciliation` (or a `Recovered` replay of an already committed settlement).
    pub fn authorize_restart(
        snapshot: &BillingAttemptSnapshot,
        fence: &BillingBootFence,
        request: &BillingRecoveryRequest,
    ) -> Result<BillingRecoveryPlan, BillingRecoveryAdmissionError> {
        plan_domain_billing_recovery(snapshot, fence, request).map_err(Self::classify)
    }

    /// Authorize the single, evidence-backed settlement of an unknown attempt.
    ///
    /// The evidence must be bound to the committed snapshot and the current fence, and usage must
    /// be known; a reconciliation that cannot say whether usage happened is refused. The returned
    /// plan records exactly one settlement and is refused on a second application.
    pub fn authorize_reconciliation(
        snapshot: &BillingAttemptSnapshot,
        fence: &BillingBootFence,
        reconciliation: &BillingReconciliation,
    ) -> Result<BillingRecoveryPlan, BillingRecoveryAdmissionError> {
        reconcile_domain_attempt(snapshot, fence, reconciliation).map_err(Self::classify)
    }

    /// Record one accepted decision in the caller-owned append-only recovery ledger.
    ///
    /// An identical plan is replayed without a second settlement; a second settlement, a skipped
    /// reconciliation or a downgrade of a recovered attempt is refused.
    pub fn admit(
        ledger: &mut BillingRecoveryLedger,
        plan: BillingRecoveryPlan,
    ) -> Result<BillingRecoveryApplyOutcome, BillingRecoveryAdmissionError> {
        ledger.apply(plan).map_err(Self::classify)
    }

    /// Validate the committed BQ-21 recovery fact through the same admission boundary.
    pub fn check_fact(fact: &BillingRecoveryFact) -> Result<(), BillingRecoveryAdmissionError> {
        validate_domain_billing_recovery(fact)
            .map_err(|code| BillingRecoveryAdmissionError::FactInvalid(code.to_owned()))
    }

    fn classify(code: String) -> BillingRecoveryAdmissionError {
        match code.as_str() {
            "billing_recovery_snapshot_invalid" => {
                BillingRecoveryAdmissionError::SnapshotInvalid(code)
            }
            "billing_recovery_boot_fence_invalid" => {
                BillingRecoveryAdmissionError::FenceInvalid(code)
            }
            "billing_recovery_second_settlement"
            | "billing_recovery_replay_conflict"
            | "billing_recovery_reconciliation_skipped"
            | "billing_recovery_decision_downgrade" => {
                BillingRecoveryAdmissionError::ReplayConflict(code)
            }
            _ => BillingRecoveryAdmissionError::Refused(code),
        }
    }
}

impl super::ControlPlane {
    /// Expose the BQ-21 restart/continue/replay admission as a `ControlPlane` method.
    ///
    /// This is an authorization boundary, not an execution entry: it validates the caller
    /// assertion against the committed snapshot and boot fence and returns the accepted plan.
    /// The caller still has to persist that plan through the normal event transaction, and no
    /// provider call, reservation release or second execution loop is started here.
    pub fn admit_billing_recovery_restart(
        &self,
        snapshot: &BillingAttemptSnapshot,
        fence: &BillingBootFence,
        request: &BillingRecoveryRequest,
    ) -> Result<BillingRecoveryPlan, BillingRecoveryAdmissionError> {
        BillingRecoveryAdmission::authorize_restart(snapshot, fence, request)
    }

    /// Expose the explicit reconciliation admission as a `ControlPlane` method.
    ///
    /// Only an unknown attempt with evidence-bound, known usage may be settled, and the returned
    /// plan carries the single settlement. Nothing is appended and no invoice is touched here.
    pub fn admit_billing_recovery_reconciliation(
        &self,
        snapshot: &BillingAttemptSnapshot,
        fence: &BillingBootFence,
        reconciliation: &BillingReconciliation,
    ) -> Result<BillingRecoveryPlan, BillingRecoveryAdmissionError> {
        BillingRecoveryAdmission::authorize_reconciliation(snapshot, fence, reconciliation)
    }

    /// Record an accepted recovery decision in the caller-owned append-only ledger.
    pub fn admit_billing_recovery_plan(
        &self,
        ledger: &mut BillingRecoveryLedger,
        plan: BillingRecoveryPlan,
    ) -> Result<BillingRecoveryApplyOutcome, BillingRecoveryAdmissionError> {
        BillingRecoveryAdmission::admit(ledger, plan)
    }
}
