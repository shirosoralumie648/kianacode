//! BQ-21 billing restart, continue, replay and recovery contracts.
//!
//! A model attempt can outlive the process that started it. This module is the value-level
//! contract a restarted caller must satisfy before it may touch an in-flight quota reservation,
//! an uncertain model attempt or an already settled attempt:
//!
//! * `BillingAttemptSnapshot` is the server-owned committed recap of one attempt: its settlement
//!   disposition, accumulated usage, settlement count and the committed source event it was read
//!   from.
//! * `BillingBootFence` is the server-owned boot/authority/lease/cursor fence of the current
//!   process. A claim from an earlier boot generation, authority epoch or lease epoch is stale and
//!   may never continue to settle work.
//! * `BillingRecoveryRequest` is the caller's assertion. It must match the snapshot and the fence
//!   exactly; it never becomes the authority for usage, epochs or ownership.
//! * `plan_billing_recovery` refuses re-dispatch of a completed attempt, a `Continue` that resets
//!   accumulated usage, stale boot/authority/lease epochs, a regressed source cursor and every
//!   attempt to treat unknown usage as success. What it accepts is either `Paused` (the in-flight
//!   reservation is still held) or `NeedsReconciliation` (usage is unknown and still occupied).
//! * `BillingReconciliation` is the explicit, evidence-backed decision that turns a
//!   `NeedsReconciliation` snapshot into `Consumed`/`Released`; `reconcile_attempt` returns the one
//!   settlement plan and `BillingAttemptSnapshot::settled_by` returns the advanced committed fact,
//!   so a second reconcile is refused instead of settling twice.
//! * `BillingRecoveryLedger` records the accepted plans append-only. An identical plan is replayed
//!   without a second settlement; a different decision for a settled attempt is refused.
//!
//! `BillingRecoveryFact` (below) is the committed recovery record, including the reference a human
//! reconciliation is tracked under. A `BillingRecoveryPlan` is the pre-reconciliation decision, so
//! a `NeedsReconciliation` plan carries no reference until `reconcile_attempt` supplies the
//! evidence; `BillingRecoveryPlan::matches_fact` keeps both surfaces on the same one-settlement
//! rule.
//!
//! Nothing here dispatches a provider call, appends an EventLog fact, releases a reservation or
//! mutates a ledger in place. Callers persist the accepted plan through the existing
//! ControlPlane/EventLog transaction.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const BILLING_RECOVERY_SCHEMA: &str = "kiana.billing-recovery.v1";
pub const BILLING_RECOVERY_SNAPSHOT_SCHEMA: &str = "kiana.billing-recovery-snapshot.v1";
pub const BILLING_RECOVERY_FENCE_SCHEMA: &str = "kiana.billing-recovery-fence.v1";
pub const BILLING_RECOVERY_REQUEST_SCHEMA: &str = "kiana.billing-recovery-request.v1";
pub const BILLING_RECOVERY_PLAN_SCHEMA: &str = "kiana.billing-recovery-plan.v1";
pub const BILLING_RECOVERY_RECONCILIATION_SCHEMA: &str = "kiana.billing-recovery-reconciliation.v1";
pub const BILLING_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingRecoveryState {
    Paused,
    NeedsReconciliation,
    Recovered,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingRecoveryFact {
    pub schema: String,
    pub attempt_id: String,
    pub reservation_digest: String,
    pub source_cursor: u64,
    pub authority_epoch: u64,
    pub lease_epoch: u64,
    pub state: BillingRecoveryState,
    pub usage_unknown: bool,
    pub continue_resets_usage: bool,
    pub settlement_count: u8,
    pub reconciliation_ref: Option<String>,
    pub recovery_digest: String,
}

impl BillingRecoveryFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BILLING_RECOVERY_SCHEMA
            || !valid_text(&self.attempt_id)
            || !valid_digest(&self.reservation_digest)
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.lease_epoch == 0
            || self.continue_resets_usage
            || self.settlement_count > 1
            || self
                .reconciliation_ref
                .as_deref()
                .is_some_and(|v| !valid_text(v))
            || matches!(self.state, BillingRecoveryState::Recovered)
                && (self.usage_unknown
                    || self.reconciliation_ref.is_none()
                    || self.settlement_count != 1)
            || matches!(self.state, BillingRecoveryState::NeedsReconciliation)
                && (!self.usage_unknown
                    || self.reconciliation_ref.is_none()
                    || self.settlement_count != 0)
            || matches!(self.state, BillingRecoveryState::Paused) && self.settlement_count != 0
            || !valid_digest(&self.recovery_digest)
            || self.recovery_digest != self.digest()
        {
            return Err("billing_recovery_fact_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "attempt_id": self.attempt_id,
            "reservation_digest": self.reservation_digest,
            "source_cursor": self.source_cursor,
            "authority_epoch": self.authority_epoch,
            "lease_epoch": self.lease_epoch,
            "state": self.state,
            "usage_unknown": self.usage_unknown,
            "continue_resets_usage": self.continue_resets_usage,
            "settlement_count": self.settlement_count,
            "reconciliation_ref": self.reconciliation_ref,
        }))
    }
}

pub fn validate_billing_recovery(fact: &BillingRecoveryFact) -> Result<(), &'static str> {
    fact.validate()
}

/// What a restarted caller asks the recovery contract to do for one attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingRecoveryAction {
    /// The process restarted and would re-send work for the attempt.
    Restart,
    /// The caller resumes the attempt and carries accumulated usage forward.
    Continue,
    /// The caller rebuilds the committed decision without changing state.
    Replay,
    /// Reserved for the explicit reconciliation path; a bare request may not claim it.
    Reconcile,
}

impl BillingRecoveryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Continue => "continue",
            Self::Replay => "replay",
            Self::Reconcile => "reconcile",
        }
    }
}

/// The committed disposition of one attempt. `Unknown` is neither a success nor a release, and
/// `InFlight` still occupies its quota reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingAttemptDisposition {
    InFlight,
    Consumed,
    Unknown,
    Released,
}

/// Server-owned committed recap of one attempt, read from one committed source event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingAttemptSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub attempt_id: String,
    pub reservation_digest: String,
    pub disposition: BillingAttemptDisposition,
    pub usage_unknown: bool,
    pub accumulated_units: u64,
    pub settlement_count: u8,
    pub reconciliation_ref: Option<String>,
    pub source_event_digest: String,
    pub source_cursor: u64,
    pub snapshot_digest: String,
}

impl BillingAttemptSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attempt_id: impl Into<String>,
        reservation_digest: impl Into<String>,
        disposition: BillingAttemptDisposition,
        accumulated_units: u64,
        reconciliation_ref: Option<String>,
        source_event_digest: impl Into<String>,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: BILLING_RECOVERY_SNAPSHOT_SCHEMA.to_owned(),
            version: BILLING_RECOVERY_VERSION,
            attempt_id: attempt_id.into(),
            reservation_digest: reservation_digest.into(),
            disposition,
            usage_unknown: matches!(disposition, BillingAttemptDisposition::Unknown),
            accumulated_units,
            settlement_count: u8::from(matches!(disposition, BillingAttemptDisposition::Consumed)),
            reconciliation_ref,
            source_event_digest: source_event_digest.into(),
            source_cursor,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_RECOVERY_SNAPSHOT_SCHEMA
            || !self.version.is_compatible_with(&BILLING_RECOVERY_VERSION)
            || !valid_text(&self.attempt_id)
            || self.settlement_count > 1
            || self.source_cursor == 0
            || self
                .reconciliation_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self.snapshot_digest != self.digest()
        {
            return Err("billing_recovery_snapshot_invalid".to_owned());
        }
        require_digest(
            &self.reservation_digest,
            "billing_recovery_reservation_digest_invalid",
        )?;
        require_digest(
            &self.source_event_digest,
            "billing_recovery_source_event_digest_invalid",
        )?;
        let consistent = match self.disposition {
            BillingAttemptDisposition::InFlight => {
                !self.usage_unknown
                    && self.settlement_count == 0
                    && self.reconciliation_ref.is_none()
            }
            BillingAttemptDisposition::Consumed => {
                !self.usage_unknown && self.settlement_count == 1
            }
            BillingAttemptDisposition::Unknown => {
                self.usage_unknown
                    && self.settlement_count == 0
                    && self.reconciliation_ref.is_none()
            }
            BillingAttemptDisposition::Released => {
                !self.usage_unknown
                    && self.settlement_count == 0
                    && self.reconciliation_ref.is_some()
            }
        };
        if !consistent {
            return Err("billing_recovery_snapshot_invalid".to_owned());
        }
        Ok(())
    }

    /// Advance an unknown attempt to its single, evidence-backed settlement.
    ///
    /// The reconciliation is bound to this snapshot (attempt, reservation digest, prior source
    /// event and cursor) and to the current boot fence. A second reconcile, a reconcile of an
    /// in-flight or already terminal attempt, a stale epoch and a release that claims consumption
    /// are all refused.
    pub fn settled_by(
        &self,
        fence: &BillingBootFence,
        reconciliation: &BillingReconciliation,
    ) -> Result<Self, String> {
        validate_reconciliation(self, fence, reconciliation)?;
        let accumulated_units = self
            .accumulated_units
            .checked_add(reconciliation.consumed_units)
            .ok_or_else(|| "billing_recovery_accumulated_units_overflow".to_owned())?;
        let disposition = match reconciliation.outcome {
            BillingReconciliationOutcome::Consumed => BillingAttemptDisposition::Consumed,
            BillingReconciliationOutcome::Released => BillingAttemptDisposition::Released,
        };
        let mut next = Self {
            schema: BILLING_RECOVERY_SNAPSHOT_SCHEMA.to_owned(),
            version: BILLING_RECOVERY_VERSION,
            attempt_id: self.attempt_id.clone(),
            reservation_digest: self.reservation_digest.clone(),
            disposition,
            usage_unknown: false,
            accumulated_units,
            settlement_count: u8::from(matches!(disposition, BillingAttemptDisposition::Consumed)),
            reconciliation_ref: Some(reconciliation.reconciliation_ref.clone()),
            source_event_digest: reconciliation.source_event_digest.clone(),
            source_cursor: reconciliation.source_cursor,
            snapshot_digest: String::new(),
        };
        next.snapshot_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "attempt_id": self.attempt_id,
            "reservation_digest": self.reservation_digest,
            "disposition": self.disposition,
            "usage_unknown": self.usage_unknown,
            "accumulated_units": self.accumulated_units,
            "settlement_count": self.settlement_count,
            "reconciliation_ref": self.reconciliation_ref,
            "source_event_digest": self.source_event_digest,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// Server-owned boot fence of the current process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingBootFence {
    pub schema: String,
    pub version: SchemaVersion,
    pub boot_generation: u64,
    pub authority_epoch: u64,
    pub lease_epoch: u64,
    pub source_cursor: u64,
    pub source_event_digest: String,
    pub fence_digest: String,
}

impl BillingBootFence {
    pub fn new(
        boot_generation: u64,
        authority_epoch: u64,
        lease_epoch: u64,
        source_cursor: u64,
        source_event_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fence = Self {
            schema: BILLING_RECOVERY_FENCE_SCHEMA.to_owned(),
            version: BILLING_RECOVERY_VERSION,
            boot_generation,
            authority_epoch,
            lease_epoch,
            source_cursor,
            source_event_digest: source_event_digest.into(),
            fence_digest: String::new(),
        };
        fence.fence_digest = fence.digest();
        fence.validate()?;
        Ok(fence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_RECOVERY_FENCE_SCHEMA
            || !self.version.is_compatible_with(&BILLING_RECOVERY_VERSION)
            || self.boot_generation == 0
            || self.authority_epoch == 0
            || self.lease_epoch == 0
            || self.source_cursor == 0
            || self.fence_digest != self.digest()
        {
            return Err("billing_recovery_boot_fence_invalid".to_owned());
        }
        require_digest(
            &self.source_event_digest,
            "billing_recovery_fence_source_event_digest_invalid",
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "boot_generation": self.boot_generation,
            "authority_epoch": self.authority_epoch,
            "lease_epoch": self.lease_epoch,
            "source_cursor": self.source_cursor,
            "source_event_digest": self.source_event_digest,
        }))
    }
}

/// A restarted caller's assertion about one attempt. Every field is compared with the committed
/// snapshot and the boot fence; none of them is trusted on its own.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingRecoveryRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub action: BillingRecoveryAction,
    pub attempt_id: String,
    pub reservation_digest: String,
    pub boot_generation: u64,
    pub authority_epoch: u64,
    pub lease_epoch: u64,
    pub accumulated_units: u64,
    pub source_event_digest: String,
    pub request_digest: String,
}

impl BillingRecoveryRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action: BillingRecoveryAction,
        attempt_id: impl Into<String>,
        reservation_digest: impl Into<String>,
        boot_generation: u64,
        authority_epoch: u64,
        lease_epoch: u64,
        accumulated_units: u64,
        source_event_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: BILLING_RECOVERY_REQUEST_SCHEMA.to_owned(),
            version: BILLING_RECOVERY_VERSION,
            action,
            attempt_id: attempt_id.into(),
            reservation_digest: reservation_digest.into(),
            boot_generation,
            authority_epoch,
            lease_epoch,
            accumulated_units,
            source_event_digest: source_event_digest.into(),
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_RECOVERY_REQUEST_SCHEMA
            || !self.version.is_compatible_with(&BILLING_RECOVERY_VERSION)
            || !valid_text(&self.attempt_id)
            || self.boot_generation == 0
            || self.authority_epoch == 0
            || self.lease_epoch == 0
            || self.request_digest != self.digest()
        {
            return Err("billing_recovery_request_invalid".to_owned());
        }
        require_digest(
            &self.reservation_digest,
            "billing_recovery_request_reservation_digest_invalid",
        )?;
        require_digest(
            &self.source_event_digest,
            "billing_recovery_request_source_event_digest_invalid",
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "action": self.action,
            "attempt_id": self.attempt_id,
            "reservation_digest": self.reservation_digest,
            "boot_generation": self.boot_generation,
            "authority_epoch": self.authority_epoch,
            "lease_epoch": self.lease_epoch,
            "accumulated_units": self.accumulated_units,
            "source_event_digest": self.source_event_digest,
        }))
    }
}

/// The accepted decision for one attempt. It never permits a re-dispatch, and `Paused` without a
/// held reservation is only reachable through an evidence-backed release.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingRecoveryPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub attempt_id: String,
    pub reservation_digest: String,
    pub action: BillingRecoveryAction,
    pub state: BillingRecoveryState,
    pub usage_unknown: bool,
    pub reservation_held: bool,
    pub settlements: u8,
    pub accumulated_units: u64,
    pub reconciliation_ref: Option<String>,
    pub retry_permitted: bool,
    pub redispatch_permitted: bool,
    pub boot_generation: u64,
    pub authority_epoch: u64,
    pub lease_epoch: u64,
    pub plan_digest: String,
}

impl BillingRecoveryPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_RECOVERY_PLAN_SCHEMA
            || !self.version.is_compatible_with(&BILLING_RECOVERY_VERSION)
            || !valid_text(&self.attempt_id)
            || self.settlements > 1
            || self.boot_generation == 0
            || self.authority_epoch == 0
            || self.lease_epoch == 0
            || self.redispatch_permitted
            || self
                .reconciliation_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || self.plan_digest != self.digest()
        {
            return Err("billing_recovery_plan_invalid".to_owned());
        }
        require_digest(
            &self.reservation_digest,
            "billing_recovery_plan_reservation_digest_invalid",
        )?;
        let consistent = match self.state {
            BillingRecoveryState::Paused => {
                self.settlements == 0
                    && !self.usage_unknown
                    && (self.reservation_held
                        || (self.retry_permitted && self.reconciliation_ref.is_some()))
                    && !(self.reservation_held && self.retry_permitted)
            }
            BillingRecoveryState::NeedsReconciliation => {
                self.settlements == 0
                    && self.usage_unknown
                    && self.reservation_held
                    && !self.retry_permitted
                    && self.reconciliation_ref.is_none()
            }
            BillingRecoveryState::Recovered => {
                self.settlements == 1
                    && !self.usage_unknown
                    && !self.reservation_held
                    && !self.retry_permitted
            }
        };
        if !consistent {
            return Err("billing_recovery_plan_invalid".to_owned());
        }
        Ok(())
    }

    /// Compare this decision with the committed BQ-21 recovery fact for the same attempt.
    ///
    /// The fact is the durable record and the plan is the decision a restarted caller is about to
    /// persist. They must agree on the attempt, on whether a settlement exists and on whether
    /// usage is still unknown, so a plan can neither claim a settlement the committed fact does
    /// not have nor hide one it already has.
    pub fn matches_fact(&self, fact: &BillingRecoveryFact) -> Result<(), String> {
        self.validate()?;
        fact.validate().map_err(|code| code.to_owned())?;
        if self.attempt_id != fact.attempt_id {
            return Err("billing_recovery_plan_fact_attempt_mismatch".to_owned());
        }
        if u8::from(matches!(self.state, BillingRecoveryState::Recovered)) != fact.settlement_count
        {
            return Err("billing_recovery_plan_fact_settlement_mismatch".to_owned());
        }
        if self.usage_unknown != fact.usage_unknown {
            return Err("billing_recovery_plan_fact_unknown_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "attempt_id": self.attempt_id,
            "reservation_digest": self.reservation_digest,
            "action": self.action,
            "state": self.state,
            "usage_unknown": self.usage_unknown,
            "reservation_held": self.reservation_held,
            "settlements": self.settlements,
            "accumulated_units": self.accumulated_units,
            "reconciliation_ref": self.reconciliation_ref,
            "retry_permitted": self.retry_permitted,
            "redispatch_permitted": self.redispatch_permitted,
            "boot_generation": self.boot_generation,
            "authority_epoch": self.authority_epoch,
            "lease_epoch": self.lease_epoch,
        }))
    }
}

/// Whether the explicit reconciliation consumed the reserved units or released them unused.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingReconciliationOutcome {
    Consumed,
    Released,
}

/// Explicit, evidence-backed reconciliation for one unknown attempt.
///
/// The record carries only identity, digests, epochs and the reconciled amount. `usage_known` is
/// part of the evidence: a reconciliation that still cannot say whether usage happened must not be
/// accepted as a settlement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingReconciliation {
    pub schema: String,
    pub version: SchemaVersion,
    pub attempt_id: String,
    pub reservation_digest: String,
    pub prior_source_event_digest: String,
    pub prior_source_cursor: u64,
    pub authority_epoch: u64,
    pub lease_epoch: u64,
    pub reconciliation_ref: String,
    pub usage_known: bool,
    pub consumed_units: u64,
    pub outcome: BillingReconciliationOutcome,
    pub source_event_digest: String,
    pub source_cursor: u64,
    pub reconciliation_digest: String,
}

impl BillingReconciliation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attempt_id: impl Into<String>,
        reservation_digest: impl Into<String>,
        prior_source_event_digest: impl Into<String>,
        prior_source_cursor: u64,
        authority_epoch: u64,
        lease_epoch: u64,
        reconciliation_ref: impl Into<String>,
        usage_known: bool,
        consumed_units: u64,
        outcome: BillingReconciliationOutcome,
        source_event_digest: impl Into<String>,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let mut reconciliation = Self {
            schema: BILLING_RECOVERY_RECONCILIATION_SCHEMA.to_owned(),
            version: BILLING_RECOVERY_VERSION,
            attempt_id: attempt_id.into(),
            reservation_digest: reservation_digest.into(),
            prior_source_event_digest: prior_source_event_digest.into(),
            prior_source_cursor,
            authority_epoch,
            lease_epoch,
            reconciliation_ref: reconciliation_ref.into(),
            usage_known,
            consumed_units,
            outcome,
            source_event_digest: source_event_digest.into(),
            source_cursor,
            reconciliation_digest: String::new(),
        };
        reconciliation.reconciliation_digest = reconciliation.digest();
        reconciliation.validate()?;
        Ok(reconciliation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_RECOVERY_RECONCILIATION_SCHEMA
            || !self.version.is_compatible_with(&BILLING_RECOVERY_VERSION)
            || !valid_text(&self.attempt_id)
            || self.prior_source_cursor == 0
            || self.source_cursor <= self.prior_source_cursor
            || self.authority_epoch == 0
            || self.lease_epoch == 0
            || self.reconciliation_digest != self.digest()
        {
            return Err("billing_recovery_reconciliation_invalid".to_owned());
        }
        if !valid_text(&self.reconciliation_ref) {
            return Err("billing_recovery_reconcile_reference_invalid".to_owned());
        }
        require_digest(
            &self.reservation_digest,
            "billing_recovery_reconciliation_reservation_digest_invalid",
        )?;
        require_digest(
            &self.prior_source_event_digest,
            "billing_recovery_reconciliation_prior_event_digest_invalid",
        )?;
        require_digest(
            &self.source_event_digest,
            "billing_recovery_reconciliation_source_event_digest_invalid",
        )?;
        if matches!(self.outcome, BillingReconciliationOutcome::Released) && self.consumed_units > 0
        {
            return Err("billing_recovery_release_claims_consumption".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "attempt_id": self.attempt_id,
            "reservation_digest": self.reservation_digest,
            "prior_source_event_digest": self.prior_source_event_digest,
            "prior_source_cursor": self.prior_source_cursor,
            "authority_epoch": self.authority_epoch,
            "lease_epoch": self.lease_epoch,
            "reconciliation_ref": self.reconciliation_ref,
            "usage_known": self.usage_known,
            "consumed_units": self.consumed_units,
            "outcome": self.outcome,
            "source_event_digest": self.source_event_digest,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// Result of recording one accepted plan in the append-only recovery ledger.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BillingRecoveryApplyOutcome {
    /// The decision was recorded once.
    Applied(BillingRecoveryPlan),
    /// The identical decision was seen again; the original plan is returned and no second
    /// settlement is created.
    Replayed(BillingRecoveryPlan),
}

/// Append-only reducer over accepted recovery decisions.
///
/// It enforces the BQ-21 settlement rule: one attempt is settled at most once, a
/// `NeedsReconciliation` attempt cannot be relabelled as `Paused`, and a recovered attempt cannot
/// be downgraded. It exposes no update/delete operation and never appends EventLog facts itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BillingRecoveryLedger {
    plans: BTreeMap<String, BillingRecoveryPlan>,
}

impl BillingRecoveryLedger {
    /// Record one decision or replay the original plan for an identical repeat.
    ///
    /// Refusals: a second settlement for an attempt that is already recovered
    /// (`billing_recovery_second_settlement`), a `Paused` decision that skips a required
    /// reconciliation (`billing_recovery_reconciliation_skipped`), a decision that downgrades a
    /// recovered attempt (`billing_recovery_decision_downgrade`) and any other different decision
    /// for a known attempt (`billing_recovery_replay_conflict`).
    pub fn apply(
        &mut self,
        plan: BillingRecoveryPlan,
    ) -> Result<BillingRecoveryApplyOutcome, String> {
        plan.validate()?;
        if let Some(existing) = self.plans.get(&plan.attempt_id) {
            if existing.plan_digest == plan.plan_digest {
                return Ok(BillingRecoveryApplyOutcome::Replayed(existing.clone()));
            }
            if matches!(existing.state, BillingRecoveryState::NeedsReconciliation)
                && !matches!(plan.state, BillingRecoveryState::Recovered)
            {
                return Err("billing_recovery_reconciliation_skipped".to_owned());
            }
            if matches!(existing.state, BillingRecoveryState::Recovered) {
                return Err(if plan.settlements == 1 {
                    "billing_recovery_second_settlement"
                } else {
                    "billing_recovery_decision_downgrade"
                }
                .to_owned());
            }
            if !matches!(plan.state, BillingRecoveryState::Recovered) {
                return Err("billing_recovery_replay_conflict".to_owned());
            }
        }
        self.plans.insert(plan.attempt_id.clone(), plan.clone());
        Ok(BillingRecoveryApplyOutcome::Applied(plan))
    }

    /// Borrow the latest accepted decision for an attempt.
    pub fn plan_for(&self, attempt_id: &str) -> Option<&BillingRecoveryPlan> {
        self.plans.get(attempt_id)
    }

    /// Number of settlements recorded for an attempt (0 or 1).
    pub fn settlement_count(&self, attempt_id: &str) -> u8 {
        self.plans
            .get(attempt_id)
            .map_or(0, |plan| plan.settlements)
    }

    /// Iterate accepted decisions in attempt-id order.
    pub fn plans(&self) -> impl Iterator<Item = &BillingRecoveryPlan> {
        self.plans.values()
    }

    /// Number of attempts with an accepted decision.
    pub fn len(&self) -> usize {
        self.plans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plans.is_empty()
    }
}

/// Decide what a restarted, continuing or replaying caller may do with one attempt.
///
/// Deny-first: a caller assertion that does not match the committed snapshot or the boot fence,
/// a stale boot/authority/lease epoch, an epoch from the future, a regressed source cursor, a
/// `Continue` that resets accumulated usage, a re-dispatch of a completed attempt, a `Continue`
/// of an unknown attempt and a bare `Reconcile` without evidence are all refused. The accepted
/// decisions are `Paused` (in-flight reservation still held), `NeedsReconciliation` (usage
/// unknown, still occupied) and, for a replay of a committed settlement, `Recovered`.
pub fn plan_billing_recovery(
    snapshot: &BillingAttemptSnapshot,
    fence: &BillingBootFence,
    request: &BillingRecoveryRequest,
) -> Result<BillingRecoveryPlan, String> {
    snapshot.validate()?;
    fence.validate()?;
    request.validate()?;
    if request.attempt_id != snapshot.attempt_id
        || request.reservation_digest != snapshot.reservation_digest
    {
        return Err("billing_recovery_attempt_binding_mismatch".to_owned());
    }
    if request.source_event_digest != snapshot.source_event_digest {
        return Err("billing_recovery_source_event_mismatch".to_owned());
    }
    if request.boot_generation != fence.boot_generation {
        return Err("billing_recovery_boot_generation_stale".to_owned());
    }
    if request.authority_epoch < fence.authority_epoch {
        return Err("billing_recovery_authority_epoch_stale".to_owned());
    }
    if request.lease_epoch < fence.lease_epoch {
        return Err("billing_recovery_lease_epoch_stale".to_owned());
    }
    if request.authority_epoch > fence.authority_epoch || request.lease_epoch > fence.lease_epoch {
        return Err("billing_recovery_epoch_ahead".to_owned());
    }
    if fence.source_cursor < snapshot.source_cursor {
        return Err("billing_recovery_source_cursor_regression".to_owned());
    }
    if matches!(request.action, BillingRecoveryAction::Reconcile) {
        return Err("billing_recovery_reconcile_requires_evidence".to_owned());
    }
    match snapshot.disposition {
        BillingAttemptDisposition::InFlight => {}
        BillingAttemptDisposition::Unknown => {
            if matches!(request.action, BillingRecoveryAction::Continue) {
                return Err("billing_recovery_unknown_requires_reconciliation".to_owned());
            }
        }
        BillingAttemptDisposition::Consumed | BillingAttemptDisposition::Released => {
            if !matches!(request.action, BillingRecoveryAction::Replay) {
                return Err("billing_recovery_attempt_already_terminal".to_owned());
            }
        }
    }
    if matches!(request.action, BillingRecoveryAction::Continue)
        && request.accumulated_units < snapshot.accumulated_units
    {
        return Err("billing_recovery_continue_resets_accumulation".to_owned());
    }
    let (state, reservation_held, settlements, retry_permitted) = match snapshot.disposition {
        BillingAttemptDisposition::InFlight => (BillingRecoveryState::Paused, true, 0, false),
        BillingAttemptDisposition::Unknown => {
            (BillingRecoveryState::NeedsReconciliation, true, 0, false)
        }
        BillingAttemptDisposition::Consumed => (BillingRecoveryState::Recovered, false, 1, false),
        BillingAttemptDisposition::Released => (BillingRecoveryState::Paused, false, 0, true),
    };
    let mut plan = BillingRecoveryPlan {
        schema: BILLING_RECOVERY_PLAN_SCHEMA.to_owned(),
        version: BILLING_RECOVERY_VERSION,
        attempt_id: snapshot.attempt_id.clone(),
        reservation_digest: snapshot.reservation_digest.clone(),
        action: request.action,
        state,
        usage_unknown: snapshot.usage_unknown,
        reservation_held,
        settlements,
        accumulated_units: snapshot.accumulated_units,
        reconciliation_ref: snapshot.reconciliation_ref.clone(),
        retry_permitted,
        redispatch_permitted: false,
        boot_generation: fence.boot_generation,
        authority_epoch: fence.authority_epoch,
        lease_epoch: fence.lease_epoch,
        plan_digest: String::new(),
    };
    plan.plan_digest = plan.digest();
    plan.validate()?;
    Ok(plan)
}

/// Decide the single settlement of an unknown attempt from explicit reconciliation evidence.
///
/// This is the only path that may produce a `Recovered` decision for an attempt that was not
/// already settled: the evidence must be bound to this snapshot and to the current fence, usage
/// must be known, and the resulting plan records exactly one settlement. A reconciliation that
/// consumed nothing releases the reservation instead and yields a `Paused` plan with zero
/// settlements and an explicit retry permission. Applying the same plan again replays it; a
/// second, different settlement is refused by `BillingRecoveryLedger`.
pub fn reconcile_attempt(
    snapshot: &BillingAttemptSnapshot,
    fence: &BillingBootFence,
    reconciliation: &BillingReconciliation,
) -> Result<BillingRecoveryPlan, String> {
    validate_reconciliation(snapshot, fence, reconciliation)?;
    let accumulated_units = snapshot
        .accumulated_units
        .checked_add(reconciliation.consumed_units)
        .ok_or_else(|| "billing_recovery_accumulated_units_overflow".to_owned())?;
    let (state, reservation_held, settlements, retry_permitted) = match reconciliation.outcome {
        BillingReconciliationOutcome::Consumed => {
            (BillingRecoveryState::Recovered, false, 1, false)
        }
        BillingReconciliationOutcome::Released => (BillingRecoveryState::Paused, false, 0, true),
    };
    let mut plan = BillingRecoveryPlan {
        schema: BILLING_RECOVERY_PLAN_SCHEMA.to_owned(),
        version: BILLING_RECOVERY_VERSION,
        attempt_id: snapshot.attempt_id.clone(),
        reservation_digest: snapshot.reservation_digest.clone(),
        action: BillingRecoveryAction::Reconcile,
        state,
        usage_unknown: false,
        reservation_held,
        settlements,
        accumulated_units,
        reconciliation_ref: Some(reconciliation.reconciliation_ref.clone()),
        retry_permitted,
        redispatch_permitted: false,
        boot_generation: fence.boot_generation,
        authority_epoch: fence.authority_epoch,
        lease_epoch: fence.lease_epoch,
        plan_digest: String::new(),
    };
    plan.plan_digest = plan.digest();
    plan.validate()?;
    Ok(plan)
}

fn validate_reconciliation(
    snapshot: &BillingAttemptSnapshot,
    fence: &BillingBootFence,
    reconciliation: &BillingReconciliation,
) -> Result<(), String> {
    snapshot.validate()?;
    fence.validate()?;
    reconciliation.validate()?;
    if !matches!(snapshot.disposition, BillingAttemptDisposition::Unknown) {
        return Err(match snapshot.disposition {
            BillingAttemptDisposition::InFlight => "billing_recovery_reconcile_requires_unknown",
            _ => "billing_recovery_reconcile_already_settled",
        }
        .to_owned());
    }
    if reconciliation.attempt_id != snapshot.attempt_id
        || reconciliation.reservation_digest != snapshot.reservation_digest
    {
        return Err("billing_recovery_reconcile_binding_mismatch".to_owned());
    }
    if reconciliation.prior_source_event_digest != snapshot.source_event_digest
        || reconciliation.prior_source_cursor != snapshot.source_cursor
    {
        return Err("billing_recovery_reconcile_source_mismatch".to_owned());
    }
    if reconciliation.authority_epoch < fence.authority_epoch {
        return Err("billing_recovery_authority_epoch_stale".to_owned());
    }
    if reconciliation.lease_epoch < fence.lease_epoch {
        return Err("billing_recovery_lease_epoch_stale".to_owned());
    }
    if reconciliation.authority_epoch > fence.authority_epoch
        || reconciliation.lease_epoch > fence.lease_epoch
    {
        return Err("billing_recovery_epoch_ahead".to_owned());
    }
    if !reconciliation.usage_known {
        return Err("billing_recovery_unknown_cannot_be_success".to_owned());
    }
    Ok(())
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn require_digest(value: &str, code: &str) -> Result<(), String> {
    if valid_digest(value) {
        Ok(())
    } else {
        Err(code.to_owned())
    }
}
