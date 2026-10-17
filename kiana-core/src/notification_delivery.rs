//! Bounded notification delivery worker planning.
//!
//! The worker is a state/lease planner only. It never spawns a task, calls a channel, consumes a
//! Broker permit or changes HumanTask authority. A composition root may execute the resulting
//! typed plan through the existing controlled ports and must commit a separate receipt.
//!
//! NM-08 adds three planning surfaces on top of the lease/state machine:
//!
//! - `plan_with_subscription` refuses to claim or dispatch an intent whose subscription was
//!   revoked/expired/paused (or whose notification window closed) and returns an explicit abandon
//!   reason instead of delivering stale content;
//! - `recovery_plan` re-plans a restored set of records after a restart, so a fresh process resumes
//!   with the same fences and never re-dispatches a claim another worker still owns;
//! - `drain` is a bounded shutdown drain: it releases only claims that were never submitted, within
//!   a release budget and a deadline, and reports every outstanding protected record instead of
//!   hiding it.

use kiana_domain::{
    json_digest, DeliveryAttemptId, NotificationDispatchIntent, NotificationOutboxClass,
    NotificationOutboxLease, NotificationOutboxRecord, NotificationOutboxState,
    NotificationOutboxTerminalReason, Subscription, NOTIFICATION_OUTBOX_MAX_LEASE_MS,
};
use serde_json::json;

pub const NOTIFICATION_DELIVERY_WORKER_SCHEMA: &str = "kiana.notification-delivery-worker.v1";
pub const NOTIFICATION_OUTBOX_DRAIN_SCHEMA: &str = "kiana.notification-outbox-drain.v1";
pub const NOTIFICATION_OUTBOX_DRAIN_POLICY_SCHEMA: &str =
    "kiana.notification-outbox-drain-policy.v1";
pub const NOTIFICATION_OUTBOX_DRAIN_MAX_RELEASES: u64 = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationDeliveryWorker {
    pub schema: String,
    pub worker_id: String,
    pub authority_epoch: u64,
    pub lease_ttl_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationDeliveryPlan {
    ClaimRequired,
    ReclaimExpiredClaim,
    Dispatch(NotificationDispatchIntent),
    AwaitReceipt,
    ReconcileUnknown,
    /// The record is claimed by another live worker. This worker must not dispatch it; the caller
    /// waits for the claim to expire (then `ReclaimExpiredClaim`) instead of duplicating an action.
    ForeignClaim,
    /// Terminate the intent with an explicit reason instead of dispatching it.
    Abandon {
        reason: NotificationOutboxTerminalReason,
    },
    Terminal,
}

/// One record of a restart recovery plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationDeliveryRecoveryStep {
    pub outbox_id: DeliveryAttemptId,
    pub revision: u64,
    pub class: NotificationOutboxClass,
    pub plan: NotificationDeliveryPlan,
}

/// Bounded shutdown drain policy. `max_releases` bounds how many live claims one shutdown may
/// release, and `deadline_unix_ms` bounds when the drain must stop; anything left over stays with
/// its lease and is reported as outstanding for the next worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationOutboxDrainPolicy {
    pub schema: String,
    pub worker_id: String,
    pub authority_epoch: u64,
    pub deadline_unix_ms: u64,
    pub max_releases: u64,
}

impl NotificationOutboxDrainPolicy {
    pub fn new(
        worker: &NotificationDeliveryWorker,
        deadline_unix_ms: u64,
        max_releases: u64,
    ) -> Result<Self, String> {
        let policy = Self {
            schema: NOTIFICATION_OUTBOX_DRAIN_POLICY_SCHEMA.to_owned(),
            worker_id: worker.worker_id.clone(),
            authority_epoch: worker.authority_epoch,
            deadline_unix_ms,
            max_releases,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_DRAIN_POLICY_SCHEMA
            || self.worker_id.trim().is_empty()
            || self.authority_epoch == 0
            || self.deadline_unix_ms == 0
            || self.max_releases == 0
            || self.max_releases > NOTIFICATION_OUTBOX_DRAIN_MAX_RELEASES
        {
            return Err("notification_outbox_drain_policy_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "worker_id": self.worker_id,
            "authority_epoch": self.authority_epoch,
            "deadline_unix_ms": self.deadline_unix_ms,
            "max_releases": self.max_releases,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationOutboxDrainAction {
    /// Release a live claim that was never submitted; the caller commits the release transition.
    ReleaseClaim,
    /// A submitted send is in flight; only its receipt may resolve it.
    AwaitReceipt,
    /// Left alone: pending work, an expired claim the next claim reclaims, or budget exhaustion.
    Untouched,
    /// Already terminal (acknowledged/failed/unknown).
    Terminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationOutboxDrainDisposition {
    pub outbox_id: DeliveryAttemptId,
    pub class: NotificationOutboxClass,
    pub action: NotificationOutboxDrainAction,
    pub lease: Option<NotificationOutboxLease>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationOutboxDrainReport {
    pub schema: String,
    pub worker_id: String,
    pub authority_epoch: u64,
    pub deadline_unix_ms: u64,
    pub max_releases: u64,
    pub considered: u64,
    pub released: u64,
    pub await_receipt: u64,
    pub untouched: u64,
    pub terminal: u64,
    /// Non-terminal `Critical`/`Actionable` records that survive the drain. They must be visible to
    /// the shutdown path (log/alert), never hidden by a bounded drain.
    pub protected_outstanding: u64,
    pub budget_exhausted: bool,
    pub dispositions: Vec<NotificationOutboxDrainDisposition>,
    pub report_digest: String,
}

impl NotificationOutboxDrainReport {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_DRAIN_SCHEMA
            || self.worker_id.trim().is_empty()
            || self.authority_epoch == 0
            || self.deadline_unix_ms == 0
            || self.max_releases == 0
            || self.considered != self.dispositions.len() as u64
        {
            return Err("notification_outbox_drain_report_header_invalid".to_owned());
        }
        let mut released = 0u64;
        let mut await_receipt = 0u64;
        let mut untouched = 0u64;
        let mut terminal = 0u64;
        let mut protected_outstanding = 0u64;
        let mut previous: Option<DeliveryAttemptId> = None;
        for disposition in &self.dispositions {
            if previous.is_some_and(|id| id >= disposition.outbox_id) {
                return Err("notification_outbox_drain_dispositions_noncanonical".to_owned());
            }
            previous = Some(disposition.outbox_id);
            match disposition.action {
                NotificationOutboxDrainAction::ReleaseClaim => {
                    if disposition.lease.is_none() {
                        return Err("notification_outbox_drain_lease_missing".to_owned());
                    }
                    released += 1;
                    protected_outstanding += u64::from(disposition.class.protected());
                }
                NotificationOutboxDrainAction::AwaitReceipt => {
                    if disposition.lease.is_some() {
                        return Err("notification_outbox_drain_lease_unexpected".to_owned());
                    }
                    await_receipt += 1;
                    protected_outstanding += u64::from(disposition.class.protected());
                }
                NotificationOutboxDrainAction::Untouched => {
                    if disposition.lease.is_some() {
                        return Err("notification_outbox_drain_lease_unexpected".to_owned());
                    }
                    untouched += 1;
                    protected_outstanding += u64::from(disposition.class.protected());
                }
                NotificationOutboxDrainAction::Terminal => {
                    if disposition.lease.is_some() {
                        return Err("notification_outbox_drain_lease_unexpected".to_owned());
                    }
                    terminal += 1;
                }
            }
        }
        if released != self.released
            || await_receipt != self.await_receipt
            || untouched != self.untouched
            || terminal != self.terminal
            || protected_outstanding != self.protected_outstanding
        {
            return Err("notification_outbox_drain_counts_mismatch".to_owned());
        }
        if self.released > self.max_releases {
            return Err("notification_outbox_drain_budget_exceeded".to_owned());
        }
        if self.budget_exhausted && self.released != self.max_releases {
            return Err("notification_outbox_drain_budget_inconsistent".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("notification_outbox_drain_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let dispositions = self
            .dispositions
            .iter()
            .map(|disposition| {
                json!({
                    "outbox_id": disposition.outbox_id,
                    "class": disposition.class,
                    "action": match disposition.action {
                        NotificationOutboxDrainAction::ReleaseClaim => "release_claim",
                        NotificationOutboxDrainAction::AwaitReceipt => "await_receipt",
                        NotificationOutboxDrainAction::Untouched => "untouched",
                        NotificationOutboxDrainAction::Terminal => "terminal",
                    },
                    "lease": disposition.lease,
                })
            })
            .collect::<Vec<_>>();
        json_digest(&json!({
            "schema": self.schema,
            "worker_id": self.worker_id,
            "authority_epoch": self.authority_epoch,
            "deadline_unix_ms": self.deadline_unix_ms,
            "max_releases": self.max_releases,
            "considered": self.considered,
            "released": self.released,
            "await_receipt": self.await_receipt,
            "untouched": self.untouched,
            "terminal": self.terminal,
            "protected_outstanding": self.protected_outstanding,
            "budget_exhausted": self.budget_exhausted,
            "dispositions": dispositions,
        }))
    }
}

impl NotificationDeliveryWorker {
    pub fn new(
        worker_id: impl Into<String>,
        authority_epoch: u64,
        lease_ttl_ms: u64,
    ) -> Result<Self, String> {
        let worker_id = worker_id.into();
        if worker_id.trim().is_empty() || worker_id.len() > 128 || worker_id.contains(['\0', '\n'])
        {
            return Err("notification_delivery_worker_id_invalid".to_owned());
        }
        if authority_epoch == 0
            || lease_ttl_ms == 0
            || lease_ttl_ms > NOTIFICATION_OUTBOX_MAX_LEASE_MS
        {
            return Err("notification_delivery_worker_config_invalid".to_owned());
        }
        Ok(Self {
            schema: NOTIFICATION_DELIVERY_WORKER_SCHEMA.to_owned(),
            worker_id,
            authority_epoch,
            lease_ttl_ms,
        })
    }

    pub fn plan(
        &self,
        record: &NotificationOutboxRecord,
        now_unix_ms: u64,
    ) -> Result<NotificationDeliveryPlan, String> {
        record.validate()?;
        if record.authority_epoch != self.authority_epoch {
            return Ok(NotificationDeliveryPlan::ReconcileUnknown);
        }
        match record.state {
            NotificationOutboxState::Pending => {
                if record.notification.expires_at_unix_ms <= now_unix_ms {
                    return Ok(NotificationDeliveryPlan::Abandon {
                        reason: NotificationOutboxTerminalReason::NotificationExpired,
                    });
                }
                Ok(NotificationDeliveryPlan::ClaimRequired)
            }
            NotificationOutboxState::Claimed => {
                let expires = record
                    .lease_expires_at_unix_ms
                    .ok_or_else(|| "notification_delivery_lease_missing".to_owned())?;
                if expires <= now_unix_ms {
                    return Ok(NotificationDeliveryPlan::ReclaimExpiredClaim);
                }
                if record.lease_owner.as_deref() != Some(self.worker_id.as_str()) {
                    return Ok(NotificationDeliveryPlan::ForeignClaim);
                }
                if record.notification.expires_at_unix_ms <= now_unix_ms {
                    return Ok(NotificationDeliveryPlan::Abandon {
                        reason: NotificationOutboxTerminalReason::NotificationExpired,
                    });
                }
                let lease = record.current_lease()?;
                Ok(NotificationDeliveryPlan::Dispatch(
                    record.dispatch_intent(&lease)?,
                ))
            }
            NotificationOutboxState::Submitted => {
                let expires = record
                    .lease_expires_at_unix_ms
                    .ok_or_else(|| "notification_delivery_lease_missing".to_owned())?;
                if expires <= now_unix_ms {
                    Ok(NotificationDeliveryPlan::ReconcileUnknown)
                } else {
                    Ok(NotificationDeliveryPlan::AwaitReceipt)
                }
            }
            NotificationOutboxState::Acknowledged
            | NotificationOutboxState::Failed
            | NotificationOutboxState::Unknown => Ok(NotificationDeliveryPlan::Terminal),
        }
    }

    /// Plan with the subscription fence applied first.
    ///
    /// A revoked/paused/expired subscription or a closed notification window produces an explicit
    /// `Abandon` reason for everything that was not submitted yet; a submitted send keeps its
    /// receipt path (`AwaitReceipt`/`ReconcileUnknown`) because only a receipt may resolve it.
    /// Identity/revision/scope mismatches fail closed without touching the record.
    pub fn plan_with_subscription(
        &self,
        record: &NotificationOutboxRecord,
        subscription: &Subscription,
        now_unix_ms: u64,
    ) -> Result<NotificationDeliveryPlan, String> {
        record.validate()?;
        subscription.validate()?;
        let foreign_live_claim = record.state == NotificationOutboxState::Claimed
            && record
                .lease_expires_at_unix_ms
                .is_some_and(|expires| expires > now_unix_ms)
            && record.lease_owner.as_deref() != Some(self.worker_id.as_str());
        match record.subscription_fence(subscription, now_unix_ms) {
            Ok(()) => self.plan(record, now_unix_ms),
            Err(refusal) => match refusal.terminal_reason() {
                Some(reason)
                    if record.state != NotificationOutboxState::Submitted
                        && !foreign_live_claim =>
                {
                    Ok(NotificationDeliveryPlan::Abandon { reason })
                }
                // An in-flight send keeps its receipt path, and a claim owned by another live
                // worker is never abandoned or dispatched from here.
                Some(_) => self.plan(record, now_unix_ms),
                // Identity/revision/scope mismatches are caller errors: refuse without a plan so an
                // intent is never dispatched against the wrong subscription.
                None => Err(refusal.code().to_owned()),
            },
        }
    }

    /// Re-plan a restored set of records after a restart.
    ///
    /// Every plan comes from the same state machine, so a live lease owned by another worker stays
    /// `Dispatch`-less (it is not claimable) and an expired claim becomes `ReclaimExpiredClaim`
    /// with a new attempt. Duplicate records are refused instead of planned twice.
    pub fn recovery_plan(
        &self,
        records: &[NotificationOutboxRecord],
        now_unix_ms: u64,
    ) -> Result<Vec<NotificationDeliveryRecoveryStep>, String> {
        let mut steps = Vec::with_capacity(records.len());
        for record in records {
            record.validate()?;
            steps.push(NotificationDeliveryRecoveryStep {
                outbox_id: record.outbox_id,
                revision: record.revision,
                class: record.effective_class(),
                plan: self.plan(record, now_unix_ms)?,
            });
        }
        steps.sort_by_key(|step| step.outbox_id);
        if steps
            .windows(2)
            .any(|pair| pair[0].outbox_id == pair[1].outbox_id)
        {
            return Err("notification_delivery_recovery_duplicate_record".to_owned());
        }
        Ok(steps)
    }

    /// Bounded shutdown drain plan.
    ///
    /// Refuses records owned by another worker, wrong authority epochs and a drain that starts after
    /// its deadline. Within the budget it releases live claims for records that were never
    /// submitted; submitted sends, pending work and expired claims are reported untouched, and
    /// every non-terminal protected record is counted as outstanding.
    pub fn drain(
        &self,
        records: &[NotificationOutboxRecord],
        policy: &NotificationOutboxDrainPolicy,
        now_unix_ms: u64,
    ) -> Result<NotificationOutboxDrainReport, String> {
        policy.validate()?;
        if policy.worker_id != self.worker_id || policy.authority_epoch != self.authority_epoch {
            return Err("notification_outbox_drain_worker_mismatch".to_owned());
        }
        if now_unix_ms > policy.deadline_unix_ms {
            return Err("notification_outbox_drain_deadline_exceeded".to_owned());
        }
        let mut ordered = records.to_vec();
        ordered.sort_by_key(|record| record.outbox_id);
        let mut dispositions = Vec::with_capacity(ordered.len());
        let mut released = 0u64;
        let mut budget_exhausted = false;
        for record in &ordered {
            record.validate()?;
            if record.authority_epoch != self.authority_epoch {
                return Err("notification_outbox_drain_epoch_mismatch".to_owned());
            }
            let action = match record.state {
                NotificationOutboxState::Claimed => {
                    let owner = record.lease_owner.as_deref();
                    let live = record
                        .lease_expires_at_unix_ms
                        .is_some_and(|expires| expires > now_unix_ms);
                    // Only a *live* foreign claim blocks the drain; an expired one is reclaimable by
                    // the next worker and is reported untouched instead.
                    if live && owner.is_some_and(|owner| owner != self.worker_id) {
                        return Err("notification_outbox_drain_foreign_claim".to_owned());
                    }
                    if live && owner.is_some() {
                        if released < policy.max_releases {
                            released += 1;
                            NotificationOutboxDrainAction::ReleaseClaim
                        } else {
                            budget_exhausted = true;
                            NotificationOutboxDrainAction::Untouched
                        }
                    } else {
                        NotificationOutboxDrainAction::Untouched
                    }
                }
                NotificationOutboxState::Submitted => NotificationOutboxDrainAction::AwaitReceipt,
                NotificationOutboxState::Pending => NotificationOutboxDrainAction::Untouched,
                NotificationOutboxState::Acknowledged
                | NotificationOutboxState::Failed
                | NotificationOutboxState::Unknown => NotificationOutboxDrainAction::Terminal,
            };
            let lease = if action == NotificationOutboxDrainAction::ReleaseClaim {
                Some(record.current_lease()?)
            } else {
                None
            };
            dispositions.push(NotificationOutboxDrainDisposition {
                outbox_id: record.outbox_id,
                class: record.effective_class(),
                action,
                lease,
            });
        }
        let mut released_count = 0u64;
        let mut await_receipt = 0u64;
        let mut untouched = 0u64;
        let mut terminal = 0u64;
        let mut protected_outstanding = 0u64;
        for disposition in &dispositions {
            match disposition.action {
                NotificationOutboxDrainAction::ReleaseClaim => {
                    released_count += 1;
                    protected_outstanding += u64::from(disposition.class.protected());
                }
                NotificationOutboxDrainAction::AwaitReceipt => {
                    await_receipt += 1;
                    protected_outstanding += u64::from(disposition.class.protected());
                }
                NotificationOutboxDrainAction::Untouched => {
                    untouched += 1;
                    protected_outstanding += u64::from(disposition.class.protected());
                }
                NotificationOutboxDrainAction::Terminal => terminal += 1,
            }
        }
        if released_count != released {
            return Err("notification_outbox_drain_budget_inconsistent".to_owned());
        }
        let mut report = NotificationOutboxDrainReport {
            schema: NOTIFICATION_OUTBOX_DRAIN_SCHEMA.to_owned(),
            worker_id: self.worker_id.clone(),
            authority_epoch: self.authority_epoch,
            deadline_unix_ms: policy.deadline_unix_ms,
            max_releases: policy.max_releases,
            considered: dispositions.len() as u64,
            released,
            await_receipt,
            untouched,
            terminal,
            protected_outstanding,
            budget_exhausted,
            dispositions,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }
}
