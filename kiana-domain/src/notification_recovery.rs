//! Cancellation, revocation, expiry, Unknown and reconciliation contracts for notification
//! delivery.
//!
//! The planner is deliberately conservative. A send that may have started is never retried from
//! this contract, and a cancellation request is never rewritten as a delivered result. Known
//! pre-send failures can request a fresh authority/delivery-key admission; the planner itself does
//! not generate either key or call a connector.
//!
//! Three artifacts close the loop around that decision:
//!
//! - every `AwaitReconciliation` plan carries a [`NotificationDeadLetter`] case, so an uncertain
//!   send/ACK is parked with an explicit reconciliation requirement instead of being resent or
//!   silently forgotten;
//! - a retry cannot be expressed as "try the old attempt again": [`NotificationRetryAdmission`]
//!   only authorizes the exact `RetryRequiresReAdmission` plan it supersedes, and refuses a reused
//!   delivery key or an authority epoch that did not advance;
//! - a dead-letter case closes only through [`NotificationRecoveryPlan::reconcile`] with an
//!   evidence digest: a confirmed delivery is terminal (no further send), a confirmed
//!   non-delivery still needs the same re-admission, and an unresolved outcome stays dead-lettered.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_RECOVERY_SCHEMA: &str = "kiana.notification-recovery.v1";
pub const NOTIFICATION_DEAD_LETTER_SCHEMA: &str = "kiana.notification-dead-letter.v1";
pub const NOTIFICATION_RETRY_ADMISSION_SCHEMA: &str = "kiana.notification-retry-admission.v1";
pub const NOTIFICATION_RECONCILIATION_SCHEMA: &str = "kiana.notification-reconciliation.v1";
pub const NOTIFICATION_RECOVERY_MIN_RETRY_LIMIT: u32 = 1;
pub const NOTIFICATION_RECOVERY_MAX_RETRY_LIMIT: u32 = 8;
pub const NOTIFICATION_RECOVERY_MAX_OPERATOR_BYTES: usize = 128;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationRecoveryState {
    Pending,
    PreSendFailed,
    InFlight,
    Submitted,
    Acknowledged,
    Unknown,
    CancelRequested,
    Revoked,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationRecoveryDisposition {
    DispatchAllowed,
    RetryRequiresReAdmission,
    AwaitReconciliation,
    TerminalNoSend,
}

/// Why one uncertain send/ACK was dead-lettered instead of retried.
///
/// The kind is derived from the same state/reason the plan reports; it exists so a reconciliation
/// worker can group cases without parsing prose, and so a fixture can prove that *every*
/// `AwaitReconciliation` outcome carries a typed case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeadLetterKind {
    /// A cancellation was requested after a send may have started.
    CancelRequestedAfterPossibleSend,
    /// An in-flight/submitted/Unknown attempt has no committed outcome yet.
    DeliveryOutcomeUnknown,
    /// The presented authority epoch is not the one that owned the attempt.
    AuthorityEpochChanged,
    /// The subscription is revoked/paused/expired while the attempt is unresolved.
    SubscriptionInactive,
    /// The notification window closed before the attempt was dispatched.
    ExpiredBeforeSend,
    /// The notification window closed while a send may already have started.
    ExpiredAfterPossibleSend,
    /// A pre-send failure could not be re-admitted (attempt budget, drift or missing activation).
    PreSendRetryNotAdmissible,
}

impl NotificationDeadLetterKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::CancelRequestedAfterPossibleSend => "cancel_requested_after_possible_send",
            Self::DeliveryOutcomeUnknown => "delivery_outcome_unknown",
            Self::AuthorityEpochChanged => "authority_epoch_changed",
            Self::SubscriptionInactive => "subscription_inactive",
            Self::ExpiredBeforeSend => "notification_expired_before_send",
            Self::ExpiredAfterPossibleSend => "notification_expired_after_possible_send",
            Self::PreSendRetryNotAdmissible => "pre_send_retry_not_admissible",
        }
    }
}

/// What a dead-letter case needs before it can be closed.
///
/// Both requirements are evidence, never an inference: a provider/channel receipt for the concrete
/// attempt, or an explicit operator/ControlPlane decision. Neither can be produced by re-sending.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationReconciliationRequirement {
    /// A receipt from the channel/provider for this delivery key.
    ProviderReceipt,
    /// An explicit operator or ControlPlane reconciliation decision.
    OperatorDecision,
}

impl NotificationReconciliationRequirement {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ProviderReceipt => "provider_receipt",
            Self::OperatorDecision => "operator_decision",
        }
    }

    /// Canonical order inside [`NotificationDeadLetter::resolution`].
    pub const fn rank(self) -> u8 {
        match self {
            Self::ProviderReceipt => 0,
            Self::OperatorDecision => 1,
        }
    }
}

fn resolution_for(kind: NotificationDeadLetterKind) -> Vec<NotificationReconciliationRequirement> {
    use NotificationReconciliationRequirement::{OperatorDecision, ProviderReceipt};
    match kind {
        // A send may have reached the recipient; only a channel/provider receipt can settle it, and
        // an operator still owns the follow-up decision.
        NotificationDeadLetterKind::CancelRequestedAfterPossibleSend
        | NotificationDeadLetterKind::DeliveryOutcomeUnknown
        | NotificationDeadLetterKind::ExpiredAfterPossibleSend => {
            vec![ProviderReceipt, OperatorDecision]
        }
        // Nothing was sent for these cases; a decision is required, a receipt cannot exist.
        NotificationDeadLetterKind::AuthorityEpochChanged
        | NotificationDeadLetterKind::SubscriptionInactive
        | NotificationDeadLetterKind::ExpiredBeforeSend
        | NotificationDeadLetterKind::PreSendRetryNotAdmissible => vec![OperatorDecision],
    }
}

/// A parked, unresolved delivery case. It never permits a blind retry: `auto_retry_allowed` is
/// always `false` and `requires_reconciliation` is always `true`, both enforced by `validate()`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDeadLetter {
    pub schema: String,
    pub notification_id: String,
    pub delivery_key_digest: String,
    pub authority_epoch: u64,
    pub state: NotificationRecoveryState,
    pub kind: NotificationDeadLetterKind,
    pub reason: String,
    pub attempt: u32,
    pub dead_lettered_at_unix_ms: u64,
    pub auto_retry_allowed: bool,
    pub requires_reconciliation: bool,
    pub resolution: Vec<NotificationReconciliationRequirement>,
    pub dead_letter_digest: String,
}

impl NotificationDeadLetter {
    #[allow(clippy::too_many_arguments)]
    fn new(
        notification_id: String,
        delivery_key_digest: String,
        authority_epoch: u64,
        state: NotificationRecoveryState,
        kind: NotificationDeadLetterKind,
        reason: &str,
        attempt: u32,
        dead_lettered_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut dead_letter = Self {
            schema: NOTIFICATION_DEAD_LETTER_SCHEMA.to_owned(),
            notification_id,
            delivery_key_digest,
            authority_epoch,
            state,
            kind,
            reason: reason.to_owned(),
            attempt,
            dead_lettered_at_unix_ms,
            auto_retry_allowed: false,
            requires_reconciliation: true,
            resolution: resolution_for(kind),
            dead_letter_digest: String::new(),
        };
        dead_letter.dead_letter_digest = dead_letter.digest();
        dead_letter.validate()?;
        Ok(dead_letter)
    }

    /// Stable dedup identity of a case: the same attempt and kind park once, not once per restart.
    pub fn case_key(&self) -> String {
        json_digest(&json!({
            "notification_id": self.notification_id,
            "delivery_key_digest": self.delivery_key_digest,
            "attempt": self.attempt,
            "kind": self.kind,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_DEAD_LETTER_SCHEMA
            || self.authority_epoch == 0
            || self.attempt == 0
            || self.dead_lettered_at_unix_ms == 0
        {
            return Err("notification_dead_letter_header_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_dead_letter_notification",
            512,
        )?;
        required(&self.reason, "notification_dead_letter_reason", 256)?;
        digest(
            &self.delivery_key_digest,
            "notification_dead_letter_delivery_key",
        )?;
        if self.auto_retry_allowed {
            return Err("notification_dead_letter_auto_retry_forbidden".to_owned());
        }
        if !self.requires_reconciliation {
            return Err("notification_dead_letter_reconciliation_required".to_owned());
        }
        if self.resolution.is_empty()
            || self
                .resolution
                .windows(2)
                .any(|pair| pair[0].rank() >= pair[1].rank())
        {
            return Err("notification_dead_letter_resolution_invalid".to_owned());
        }
        digest(&self.dead_letter_digest, "notification_dead_letter_digest")?;
        if self.dead_letter_digest != self.digest() {
            return Err("notification_dead_letter_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "delivery_key_digest": self.delivery_key_digest,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "kind": self.kind,
            "reason": self.reason,
            "attempt": self.attempt,
            "dead_lettered_at_unix_ms": self.dead_lettered_at_unix_ms,
            "auto_retry_allowed": self.auto_retry_allowed,
            "requires_reconciliation": self.requires_reconciliation,
            "resolution": self.resolution,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationRecoveryInput {
    pub schema: String,
    pub notification_id: String,
    pub delivery_key_digest: String,
    pub authority_epoch: u64,
    pub expected_authority_epoch: u64,
    pub subscription_active: bool,
    pub state: NotificationRecoveryState,
    pub attempt: u32,
    pub max_pre_send_retries: u32,
    pub delivery_started: bool,
    pub now_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

impl NotificationRecoveryInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_RECOVERY_SCHEMA
            || self.authority_epoch == 0
            || self.expected_authority_epoch == 0
            || self.attempt == 0
            || self.max_pre_send_retries < NOTIFICATION_RECOVERY_MIN_RETRY_LIMIT
            || self.max_pre_send_retries > NOTIFICATION_RECOVERY_MAX_RETRY_LIMIT
            || self.now_unix_ms == 0
            || self.expires_at_unix_ms == 0
        {
            return Err("notification_recovery_header_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_recovery_notification",
            512,
        )?;
        digest(
            &self.delivery_key_digest,
            "notification_recovery_delivery_key",
        )?;
        if self.attempt > self.max_pre_send_retries.saturating_add(1) {
            return Err("notification_recovery_attempt_invalid".to_owned());
        }
        Ok(())
    }

    pub fn plan(&self) -> Result<NotificationRecoveryPlan, String> {
        self.validate()?;
        let epoch_mismatch = self.authority_epoch != self.expected_authority_epoch;
        let (disposition, reason, next_attempt, new_keys) = match self.state {
            NotificationRecoveryState::Acknowledged => (
                NotificationRecoveryDisposition::TerminalNoSend,
                "acknowledged",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::Revoked if !self.delivery_started => (
                NotificationRecoveryDisposition::TerminalNoSend,
                "subscription_revoked_before_send",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::Expired if !self.delivery_started => (
                NotificationRecoveryDisposition::TerminalNoSend,
                "notification_expired_before_send",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::CancelRequested if !self.delivery_started => (
                NotificationRecoveryDisposition::TerminalNoSend,
                "cancel_requested_before_send",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::CancelRequested => (
                NotificationRecoveryDisposition::AwaitReconciliation,
                "cancel_requested_after_possible_send",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::Revoked => (
                NotificationRecoveryDisposition::AwaitReconciliation,
                "subscription_revoked_after_possible_send",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::Expired => (
                NotificationRecoveryDisposition::AwaitReconciliation,
                "notification_expired_after_possible_send",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::InFlight
            | NotificationRecoveryState::Submitted
            | NotificationRecoveryState::Unknown => (
                NotificationRecoveryDisposition::AwaitReconciliation,
                "delivery_outcome_unknown",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::PreSendFailed
                if !self.delivery_started
                    && !epoch_mismatch
                    && self.subscription_active
                    && self.now_unix_ms < self.expires_at_unix_ms
                    && self.attempt <= self.max_pre_send_retries =>
            {
                (
                    NotificationRecoveryDisposition::RetryRequiresReAdmission,
                    "known_pre_send_failure",
                    self.attempt.saturating_add(1),
                    true,
                )
            }
            NotificationRecoveryState::Pending
                if !self.delivery_started
                    && !epoch_mismatch
                    && self.subscription_active
                    && self.now_unix_ms < self.expires_at_unix_ms =>
            {
                (
                    NotificationRecoveryDisposition::DispatchAllowed,
                    "ready_to_dispatch",
                    self.attempt,
                    false,
                )
            }
            NotificationRecoveryState::PreSendFailed => (
                NotificationRecoveryDisposition::AwaitReconciliation,
                if epoch_mismatch {
                    "authority_epoch_changed"
                } else if !self.subscription_active {
                    "subscription_not_active"
                } else if self.now_unix_ms >= self.expires_at_unix_ms {
                    "notification_expired_before_send"
                } else {
                    "pre_send_retry_not_admissible"
                },
                self.attempt,
                false,
            ),
            NotificationRecoveryState::Pending => (
                NotificationRecoveryDisposition::AwaitReconciliation,
                if epoch_mismatch {
                    "authority_epoch_changed"
                } else if !self.subscription_active {
                    "subscription_not_active"
                } else if self.now_unix_ms >= self.expires_at_unix_ms {
                    "notification_expired_before_send"
                } else {
                    "pending_reconciliation_required"
                },
                self.attempt,
                false,
            ),
        };
        NotificationRecoveryPlan::new(self, disposition, reason, next_attempt, new_keys)
    }
}

fn dead_letter_kind(state: NotificationRecoveryState, reason: &str) -> NotificationDeadLetterKind {
    match state {
        NotificationRecoveryState::CancelRequested => {
            NotificationDeadLetterKind::CancelRequestedAfterPossibleSend
        }
        NotificationRecoveryState::Revoked => NotificationDeadLetterKind::SubscriptionInactive,
        NotificationRecoveryState::Expired => NotificationDeadLetterKind::ExpiredAfterPossibleSend,
        NotificationRecoveryState::InFlight
        | NotificationRecoveryState::Submitted
        | NotificationRecoveryState::Unknown => NotificationDeadLetterKind::DeliveryOutcomeUnknown,
        NotificationRecoveryState::PreSendFailed => match reason {
            "authority_epoch_changed" => NotificationDeadLetterKind::AuthorityEpochChanged,
            "subscription_not_active" => NotificationDeadLetterKind::SubscriptionInactive,
            "notification_expired_before_send" => NotificationDeadLetterKind::ExpiredBeforeSend,
            _ => NotificationDeadLetterKind::PreSendRetryNotAdmissible,
        },
        NotificationRecoveryState::Pending => match reason {
            "authority_epoch_changed" => NotificationDeadLetterKind::AuthorityEpochChanged,
            "subscription_not_active" => NotificationDeadLetterKind::SubscriptionInactive,
            "notification_expired_before_send" => NotificationDeadLetterKind::ExpiredBeforeSend,
            _ => NotificationDeadLetterKind::DeliveryOutcomeUnknown,
        },
        // An acknowledged notification is terminal and is never re-sent from here, so no plan
        // carrying this state can reach a dead letter.
        NotificationRecoveryState::Acknowledged => {
            NotificationDeadLetterKind::DeliveryOutcomeUnknown
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationRecoveryPlan {
    pub schema: String,
    pub notification_id: String,
    pub delivery_key_digest: String,
    pub authority_epoch: u64,
    pub state: NotificationRecoveryState,
    pub disposition: NotificationRecoveryDisposition,
    pub reason: String,
    pub next_attempt: u32,
    pub new_authority_and_delivery_key_required: bool,
    /// Present exactly for `AwaitReconciliation`: the parked case a reconciliation worker resolves.
    pub dead_letter: Option<NotificationDeadLetter>,
    pub plan_digest: String,
}

impl NotificationRecoveryPlan {
    fn new(
        input: &NotificationRecoveryInput,
        disposition: NotificationRecoveryDisposition,
        reason: &str,
        next_attempt: u32,
        new_authority_and_delivery_key_required: bool,
    ) -> Result<Self, String> {
        let dead_letter = if disposition == NotificationRecoveryDisposition::AwaitReconciliation {
            Some(NotificationDeadLetter::new(
                input.notification_id.clone(),
                input.delivery_key_digest.clone(),
                input.authority_epoch,
                input.state,
                dead_letter_kind(input.state, reason),
                reason,
                input.attempt,
                input.now_unix_ms,
            )?)
        } else {
            None
        };
        let mut plan = Self {
            schema: NOTIFICATION_RECOVERY_SCHEMA.to_owned(),
            notification_id: input.notification_id.clone(),
            delivery_key_digest: input.delivery_key_digest.clone(),
            authority_epoch: input.authority_epoch,
            state: input.state,
            disposition,
            reason: reason.to_owned(),
            next_attempt,
            new_authority_and_delivery_key_required,
            dead_letter,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_RECOVERY_SCHEMA
            || self.authority_epoch == 0
            || self.next_attempt == 0
        {
            return Err("notification_recovery_plan_header_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_recovery_plan_notification",
            512,
        )?;
        required(&self.reason, "notification_recovery_plan_reason", 256)?;
        digest(
            &self.delivery_key_digest,
            "notification_recovery_plan_delivery_key",
        )?;
        if self.new_authority_and_delivery_key_required
            && self.disposition != NotificationRecoveryDisposition::RetryRequiresReAdmission
        {
            return Err("notification_recovery_new_key_disposition_invalid".to_owned());
        }
        if matches!(
            self.disposition,
            NotificationRecoveryDisposition::AwaitReconciliation
                | NotificationRecoveryDisposition::TerminalNoSend
        ) && self.new_authority_and_delivery_key_required
        {
            return Err("notification_recovery_unknown_retry_forbidden".to_owned());
        }
        match (&self.dead_letter, self.disposition) {
            (Some(dead_letter), NotificationRecoveryDisposition::AwaitReconciliation) => {
                if dead_letter.notification_id != self.notification_id
                    || dead_letter.delivery_key_digest != self.delivery_key_digest
                    || dead_letter.authority_epoch != self.authority_epoch
                    || dead_letter.state != self.state
                    || dead_letter.reason != self.reason
                {
                    return Err("notification_recovery_dead_letter_mismatch".to_owned());
                }
                dead_letter.validate()?;
            }
            (Some(_), _) => return Err("notification_recovery_dead_letter_unexpected".to_owned()),
            (None, NotificationRecoveryDisposition::AwaitReconciliation) => {
                return Err("notification_recovery_dead_letter_missing".to_owned())
            }
            (None, _) => {}
        }
        digest(&self.plan_digest, "notification_recovery_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("notification_recovery_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Resolve a dead-letter case with evidence.
    ///
    /// This never sends and never retries: a confirmed delivery is terminal, a confirmed
    /// non-delivery still requires [`NotificationRetryAdmission`], and an unresolved outcome stays
    /// dead-lettered. A plan without a case (a cancel/revoke/expiry before send, or a dispatched
    /// record) cannot be reconciled at all, so a refused send can never be rewritten as delivered.
    pub fn reconcile(
        &self,
        outcome: NotificationReconciliationOutcome,
        evidence_digest: &str,
        resolved_at_unix_ms: u64,
    ) -> Result<NotificationReconciliationResolution, String> {
        self.validate()?;
        let Some(dead_letter) = self.dead_letter.as_ref() else {
            return Err("notification_reconciliation_no_case".to_owned());
        };
        digest(evidence_digest, "notification_reconciliation_evidence")?;
        if resolved_at_unix_ms == 0 || resolved_at_unix_ms < dead_letter.dead_lettered_at_unix_ms {
            return Err("notification_reconciliation_time_invalid".to_owned());
        }
        let (resolution, retry_admission_required) = match outcome {
            NotificationReconciliationOutcome::Delivered => {
                (NotificationRecoveryResolution::TerminalAcknowledged, false)
            }
            NotificationReconciliationOutcome::NotDelivered => {
                (NotificationRecoveryResolution::RetryAdmissionRequired, true)
            }
            NotificationReconciliationOutcome::Unknown => {
                (NotificationRecoveryResolution::RemainsDeadLettered, false)
            }
        };
        NotificationReconciliationResolution::new(
            self,
            dead_letter,
            outcome,
            resolution,
            evidence_digest,
            resolved_at_unix_ms,
            retry_admission_required,
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "delivery_key_digest": self.delivery_key_digest,
            "authority_epoch": self.authority_epoch,
            "state": self.state,
            "disposition": self.disposition,
            "reason": self.reason,
            "next_attempt": self.next_attempt,
            "new_authority_and_delivery_key_required": self.new_authority_and_delivery_key_required,
            "dead_letter": self.dead_letter,
        }))
    }
}

/// A fresh authority epoch plus delivery key that supersede one failed pre-send attempt.
///
/// The planner never issues this itself: an operator/ControlPlane decision produces it, and
/// [`Self::authorize`] checks it against the exact `RetryRequiresReAdmission` plan it claims to
/// satisfy. Reusing the previous delivery key or keeping the same authority epoch is refused on
/// both `validate()` and `authorize()`, so retrying the old attempt cannot be dressed up as a new
/// admission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationRetryAdmission {
    pub schema: String,
    pub notification_id: String,
    pub previous_delivery_key_digest: String,
    pub previous_authority_epoch: u64,
    pub new_delivery_key_digest: String,
    pub new_authority_epoch: u64,
    pub operator_id: String,
    pub admitted_at_unix_ms: u64,
    pub admission_digest: String,
}

impl NotificationRetryAdmission {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        notification_id: impl Into<String>,
        previous_delivery_key_digest: impl Into<String>,
        previous_authority_epoch: u64,
        new_delivery_key_digest: impl Into<String>,
        new_authority_epoch: u64,
        operator_id: impl Into<String>,
        admitted_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut admission = Self {
            schema: NOTIFICATION_RETRY_ADMISSION_SCHEMA.to_owned(),
            notification_id: notification_id.into(),
            previous_delivery_key_digest: previous_delivery_key_digest.into(),
            previous_authority_epoch,
            new_delivery_key_digest: new_delivery_key_digest.into(),
            new_authority_epoch,
            operator_id: operator_id.into(),
            admitted_at_unix_ms,
            admission_digest: String::new(),
        };
        admission.admission_digest = admission.digest();
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_RETRY_ADMISSION_SCHEMA
            || self.previous_authority_epoch == 0
            || self.new_authority_epoch == 0
            || self.admitted_at_unix_ms == 0
        {
            return Err("notification_retry_admission_header_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_retry_admission_notification",
            512,
        )?;
        required(
            &self.operator_id,
            "notification_retry_admission_operator",
            NOTIFICATION_RECOVERY_MAX_OPERATOR_BYTES,
        )?;
        digest(
            &self.previous_delivery_key_digest,
            "notification_retry_admission_previous_key",
        )?;
        digest(
            &self.new_delivery_key_digest,
            "notification_retry_admission_new_key",
        )?;
        if self.new_delivery_key_digest == self.previous_delivery_key_digest {
            return Err("notification_retry_admission_delivery_key_reused".to_owned());
        }
        if self.new_authority_epoch <= self.previous_authority_epoch {
            return Err("notification_retry_admission_epoch_not_advanced".to_owned());
        }
        digest(
            &self.admission_digest,
            "notification_retry_admission_digest",
        )?;
        if self.admission_digest != self.digest() {
            return Err("notification_retry_admission_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Bind the admission to the plan it supersedes. Every mismatch is a refusal, so an admission
    /// for another notification, another attempt revision or another epoch cannot unlock a retry.
    pub fn authorize(&self, plan: &NotificationRecoveryPlan) -> Result<(), String> {
        self.validate()?;
        plan.validate()?;
        if plan.disposition != NotificationRecoveryDisposition::RetryRequiresReAdmission {
            return Err("notification_retry_admission_disposition_invalid".to_owned());
        }
        if plan.notification_id != self.notification_id {
            return Err("notification_retry_admission_notification_mismatch".to_owned());
        }
        if plan.delivery_key_digest != self.previous_delivery_key_digest {
            return Err("notification_retry_admission_previous_key_mismatch".to_owned());
        }
        if plan.authority_epoch != self.previous_authority_epoch {
            return Err("notification_retry_admission_previous_epoch_mismatch".to_owned());
        }
        if plan.dead_letter.is_some() {
            return Err("notification_retry_admission_dead_letter_present".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "previous_delivery_key_digest": self.previous_delivery_key_digest,
            "previous_authority_epoch": self.previous_authority_epoch,
            "new_delivery_key_digest": self.new_delivery_key_digest,
            "new_authority_epoch": self.new_authority_epoch,
            "operator_id": self.operator_id,
            "admitted_at_unix_ms": self.admitted_at_unix_ms,
        }))
    }
}

/// What a reconciliation worker observed about a dead-lettered attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationReconciliationOutcome {
    /// The channel/provider confirmed the attempt reached the recipient.
    Delivered,
    /// The channel/provider confirmed the attempt never reached the recipient.
    NotDelivered,
    /// The outcome is still unresolved; the case stays parked.
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationRecoveryResolution {
    /// Evidence confirmed delivery: terminal, and nothing is re-sent from here.
    TerminalAcknowledged,
    /// Evidence confirmed non-delivery: only a fresh admission may send again.
    RetryAdmissionRequired,
    /// No evidence settled the attempt: the case stays dead-lettered.
    RemainsDeadLettered,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationReconciliationResolution {
    pub schema: String,
    pub notification_id: String,
    pub delivery_key_digest: String,
    pub authority_epoch: u64,
    pub case_key: String,
    pub outcome: NotificationReconciliationOutcome,
    pub resolution: NotificationRecoveryResolution,
    pub evidence_digest: String,
    pub retry_admission_required: bool,
    pub auto_retry_allowed: bool,
    pub resolved_at_unix_ms: u64,
    pub resolution_digest: String,
}

impl NotificationReconciliationResolution {
    #[allow(clippy::too_many_arguments)]
    fn new(
        plan: &NotificationRecoveryPlan,
        dead_letter: &NotificationDeadLetter,
        outcome: NotificationReconciliationOutcome,
        resolution: NotificationRecoveryResolution,
        evidence_digest: &str,
        resolved_at_unix_ms: u64,
        retry_admission_required: bool,
    ) -> Result<Self, String> {
        let mut resolution_record = Self {
            schema: NOTIFICATION_RECONCILIATION_SCHEMA.to_owned(),
            notification_id: plan.notification_id.clone(),
            delivery_key_digest: plan.delivery_key_digest.clone(),
            authority_epoch: plan.authority_epoch,
            case_key: dead_letter.case_key(),
            outcome,
            resolution,
            evidence_digest: evidence_digest.to_owned(),
            retry_admission_required,
            auto_retry_allowed: false,
            resolved_at_unix_ms,
            resolution_digest: String::new(),
        };
        resolution_record.resolution_digest = resolution_record.digest();
        resolution_record.validate()?;
        Ok(resolution_record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_RECONCILIATION_SCHEMA
            || self.authority_epoch == 0
            || self.resolved_at_unix_ms == 0
        {
            return Err("notification_reconciliation_header_invalid".to_owned());
        }
        required(
            &self.notification_id,
            "notification_reconciliation_notification",
            512,
        )?;
        digest(
            &self.delivery_key_digest,
            "notification_reconciliation_delivery_key",
        )?;
        digest(
            &self.evidence_digest,
            "notification_reconciliation_evidence",
        )?;
        digest(&self.case_key, "notification_reconciliation_case_key")?;
        if self.auto_retry_allowed {
            return Err("notification_reconciliation_auto_retry_forbidden".to_owned());
        }
        let expected_retry =
            self.resolution == NotificationRecoveryResolution::RetryAdmissionRequired;
        if self.retry_admission_required != expected_retry {
            return Err("notification_reconciliation_retry_flag_invalid".to_owned());
        }
        digest(
            &self.resolution_digest,
            "notification_reconciliation_digest",
        )?;
        if self.resolution_digest != self.digest() {
            return Err("notification_reconciliation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "notification_id": self.notification_id,
            "delivery_key_digest": self.delivery_key_digest,
            "authority_epoch": self.authority_epoch,
            "case_key": self.case_key,
            "outcome": self.outcome,
            "resolution": self.resolution,
            "evidence_digest": self.evidence_digest,
            "retry_admission_required": self.retry_admission_required,
            "auto_retry_allowed": self.auto_retry_allowed,
            "resolved_at_unix_ms": self.resolved_at_unix_ms,
        }))
    }
}
