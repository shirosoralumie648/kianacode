//! Core facade for the pure notification recovery planner and the NM-08 outbox bridge.
//!
//! The facade keeps cancellation/revocation/expiry/Unknown decisions on the same server-owned
//! contract without dispatching a channel or turning reconciliation into an automatic retry. The
//! bridge derives the planner input from one committed outbox record plus its subscription, so the
//! NM-08 delivery boundary and the NM-18 recovery gate cannot disagree about whether a send may
//! already have started.

use kiana_domain::{
    json_digest, NotificationOutboxRecord, NotificationOutboxState,
    NotificationOutboxTerminalReason, Subscription, SubscriptionStatus,
};
use serde_json::json;

use crate::notification_delivery::NotificationDeliveryWorker;

pub use kiana_domain::{
    NotificationDeadLetter, NotificationDeadLetterKind, NotificationReconciliationOutcome,
    NotificationReconciliationRequirement, NotificationRecoveryDisposition,
    NotificationRecoveryInput, NotificationRecoveryPlan, NotificationRecoveryResolution,
    NotificationRecoveryState, NotificationRetryAdmission, NOTIFICATION_DEAD_LETTER_SCHEMA,
    NOTIFICATION_RECONCILIATION_SCHEMA, NOTIFICATION_RECOVERY_MAX_RETRY_LIMIT,
    NOTIFICATION_RECOVERY_MIN_RETRY_LIMIT, NOTIFICATION_RECOVERY_SCHEMA,
    NOTIFICATION_RETRY_ADMISSION_SCHEMA,
};

/// What the caller knows about the channel send of one outbox record.
///
/// The bridge never guesses this: a caller that cannot affirm "the attempt never reached the
/// channel" must present [`Self::PossiblyStarted`], which routes every uncertainty to
/// dead-letter/reconcile instead of a bounded retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationAttemptObservation {
    /// The attempt never reached the channel, so a reported failure is still a pre-send failure.
    NeverStarted,
    /// A send may have started; only a receipt or a reconciliation may settle its outcome.
    PossiblyStarted,
}

pub fn plan_notification_recovery(
    input: &NotificationRecoveryInput,
) -> Result<NotificationRecoveryPlan, String> {
    input.plan()
}

/// The delivery key of one outbox attempt: stable across the attempt lifecycle and unique per
/// notification/attempt, so a retry cannot reuse the key of the attempt it supersedes.
fn outbox_delivery_key_digest(record: &NotificationOutboxRecord) -> String {
    json_digest(&json!({
        "notification_id": record.notification.notification_id,
        "delivery_attempt_id": record.attempt.delivery_attempt_id,
        "subscription_id": record.attempt.subscription_id,
        "attempt_number": record.attempt.attempt_number,
    }))
}

fn outbox_recovery_state(record: &NotificationOutboxRecord) -> NotificationRecoveryState {
    match record.state {
        NotificationOutboxState::Pending | NotificationOutboxState::Claimed => {
            NotificationRecoveryState::Pending
        }
        NotificationOutboxState::Submitted => NotificationRecoveryState::Submitted,
        NotificationOutboxState::Acknowledged => NotificationRecoveryState::Acknowledged,
        NotificationOutboxState::Unknown => NotificationRecoveryState::Unknown,
        NotificationOutboxState::Failed => match record.terminal_reason {
            Some(
                NotificationOutboxTerminalReason::SubscriptionRevoked
                | NotificationOutboxTerminalReason::SubscriptionPaused,
            ) => NotificationRecoveryState::Revoked,
            Some(
                NotificationOutboxTerminalReason::SubscriptionExpired
                | NotificationOutboxTerminalReason::NotificationExpired,
            ) => NotificationRecoveryState::Expired,
            Some(NotificationOutboxTerminalReason::DeliveryFailed) | None => {
                NotificationRecoveryState::PreSendFailed
            }
        },
    }
}

/// Re-plan one NM-08 outbox record through the NM-18 recovery gate.
///
/// The bridge is read-only: it validates the record/subscription pair, refuses a live claim another
/// worker owns, and returns the typed plan (with its dead-letter case) for the composition root to
/// commit. Identity/revision/scope drift is a caller error and is refused; revocation, expiry and
/// epoch drift are recovery inputs and become `TerminalNoSend` or `AwaitReconciliation`.
///
/// `observation` is the caller's knowledge of the send itself; `cancel_requested` lets a
/// cancellation request be planned even though the outbox record has no cancellation state of its
/// own. A cancellation before the send is `TerminalNoSend`, never a delivered or dispatched plan.
pub fn plan_notification_outbox_recovery(
    worker: &NotificationDeliveryWorker,
    record: &NotificationOutboxRecord,
    subscription: &Subscription,
    observation: NotificationAttemptObservation,
    cancel_requested: bool,
    max_pre_send_retries: u32,
    now_unix_ms: u64,
) -> Result<NotificationRecoveryPlan, String> {
    record.validate()?;
    subscription.validate()?;
    if record.state == NotificationOutboxState::Claimed
        && record
            .lease_expires_at_unix_ms
            .is_some_and(|expires| expires > now_unix_ms)
        && record.lease_owner.as_deref() != Some(worker.worker_id.as_str())
    {
        return Err("notification_recovery_foreign_live_claim".to_owned());
    }
    match record.subscription_fence(subscription, now_unix_ms) {
        Ok(()) => {}
        Err(refusal) if refusal.terminal_reason().is_some() => {}
        Err(refusal) => return Err(refusal.code().to_owned()),
    }
    let delivery_started = !matches!(observation, NotificationAttemptObservation::NeverStarted)
        || matches!(
            record.state,
            NotificationOutboxState::Submitted
                | NotificationOutboxState::Acknowledged
                | NotificationOutboxState::Unknown
        );
    let state = if cancel_requested {
        NotificationRecoveryState::CancelRequested
    } else {
        outbox_recovery_state(record)
    };
    // The planner's attempt field is bounded by the retry envelope (`max + 1`). An outbox attempt
    // beyond that envelope is still planned — the bounded value only makes every retry path fall
    // through to a parked/not-admissible case — instead of refusing a legitimate record.
    let attempt = record
        .attempt
        .attempt_number
        .min(max_pre_send_retries.saturating_add(1));
    let input = NotificationRecoveryInput {
        schema: NOTIFICATION_RECOVERY_SCHEMA.to_owned(),
        notification_id: record.notification.notification_id.to_string(),
        delivery_key_digest: outbox_delivery_key_digest(record),
        authority_epoch: worker.authority_epoch,
        expected_authority_epoch: record.authority_epoch,
        subscription_active: subscription.status == SubscriptionStatus::Active
            && subscription.expires_at_unix_ms > now_unix_ms,
        state,
        attempt,
        max_pre_send_retries,
        delivery_started,
        now_unix_ms,
        expires_at_unix_ms: record.notification.expires_at_unix_ms,
    };
    input.plan()
}
