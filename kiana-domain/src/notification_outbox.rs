//! Bounded notification outbox and delivery lease contracts.
//!
//! An outbox record is a server-owned delivery intent. It is not a channel adapter, a Broker
//! permit, a notification bus or an agent loop. The record carries only redaction-safe identity
//! and digest material; an external adapter must still return a separately validated receipt.

use crate::{
    json_digest, DeliveryAttempt, DeliveryAttemptId, DeliveryAttemptStatus, DeliveryReceipt,
    Notification, NotificationChannel, NotificationId, NotificationStatus, Subscription,
    SubscriptionId, SubscriptionStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_OUTBOX_SCHEMA: &str = "kiana.notification-outbox.v1";
pub const NOTIFICATION_OUTBOX_LEASE_SCHEMA: &str = "kiana.notification-outbox-lease.v1";
pub const NOTIFICATION_DISPATCH_INTENT_SCHEMA: &str = "kiana.notification-dispatch-intent.v1";
pub const NOTIFICATION_OUTBOX_TRANSITION_SCHEMA: &str = "kiana.notification-outbox-transition.v1";
pub const NOTIFICATION_OUTBOX_ADMISSION_SCHEMA: &str = "kiana.notification-outbox-admission.v1";
pub const NOTIFICATION_OUTBOX_PRESSURE_SCHEMA: &str = "kiana.notification-outbox-pressure.v1";
pub const NOTIFICATION_OUTBOX_MAX_WORKER_ID_BYTES: usize = 128;
pub const NOTIFICATION_OUTBOX_MAX_LEASE_MS: u64 = 24 * 60 * 60 * 1_000;

fn scope_is_subset(inner: &[String], outer: &[String]) -> bool {
    inner.iter().all(|entry| outer.contains(entry))
}

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

/// Server-owned delivery class for an outbox record.
///
/// The class is a delivery-priority declaration, not an authority grant. `BestEffort` records may
/// be shed at admission when the bounded queue is full; `Critical` (incident/terminal facts) and
/// `Actionable` (an approval/review/acceptance/incident/reconciliation intent carries an action
/// ref) records are never shed silently. The caller may escalate the class but never understate
/// the floor that the notification itself proves.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationOutboxClass {
    BestEffort,
    Critical,
    Actionable,
}

impl NotificationOutboxClass {
    /// Ordinal used for the pressure ledger and for `max(declared, floor)` escalation.
    pub const fn rank(self) -> u8 {
        match self {
            Self::BestEffort => 1,
            Self::Critical => 2,
            Self::Actionable => 3,
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::BestEffort => "best_effort",
            Self::Critical => "critical",
            Self::Actionable => "actionable",
        }
    }

    /// Protected records are never shed by queue pressure and are always reported as outstanding.
    pub const fn protected(self) -> bool {
        matches!(self, Self::Critical | Self::Actionable)
    }

    /// The class that the notification itself proves: an actionable notification must be declared
    /// `Actionable` so a bounded queue cannot drop an approval intent.
    pub fn required_floor(notification: &Notification) -> Self {
        if notification.action_ref_id.is_some() {
            Self::Actionable
        } else {
            Self::BestEffort
        }
    }
}

/// Why an outbox record left the delivery pipeline without a channel receipt.
///
/// Every `Failed` record carries exactly one terminal reason so a bounded queue, an expired
/// window or a revoked subscription can be accounted for instead of disappearing.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationOutboxTerminalReason {
    /// A channel/transport failure reported through a lease (bounded retry is a later step).
    DeliveryFailed,
    /// The subscription that authorized the intent was revoked before delivery.
    SubscriptionRevoked,
    /// The subscription expired before delivery.
    SubscriptionExpired,
    /// The subscription was paused before delivery.
    SubscriptionPaused,
    /// The notification's own delivery window closed before the intent was dispatched.
    NotificationExpired,
}

impl NotificationOutboxTerminalReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::DeliveryFailed => "delivery_failed",
            Self::SubscriptionRevoked => "subscription_revoked",
            Self::SubscriptionExpired => "subscription_expired",
            Self::SubscriptionPaused => "subscription_paused",
            Self::NotificationExpired => "notification_expired",
        }
    }
}

/// Why a best-effort record was refused admission instead of entering the outbox.
///
/// A shed is a refusal before any intent exists: no attempt, no lease and no dispatch can be
/// duplicated. Protected (critical/actionable) records are never shed, at any reason.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationOutboxShedReason {
    /// The bounded best-effort queue is at capacity.
    QueueFull,
    /// The notification's delivery window already closed.
    NotificationExpired,
}

impl NotificationOutboxShedReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::QueueFull => "queue_full",
            Self::NotificationExpired => "notification_expired",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationOutboxState {
    Pending,
    Claimed,
    Submitted,
    Acknowledged,
    Failed,
    Unknown,
}

impl NotificationOutboxState {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        if self == next {
            return Err("notification_outbox_duplicate_transition");
        }
        let allowed = matches!(
            (self, next),
            (Self::Pending, Self::Claimed | Self::Failed)
                | (
                    Self::Claimed,
                    Self::Submitted | Self::Failed | Self::Unknown | Self::Pending
                )
                | (
                    Self::Submitted,
                    Self::Acknowledged | Self::Failed | Self::Unknown
                )
        );
        if allowed {
            Ok(next)
        } else {
            Err("notification_outbox_transition_invalid")
        }
    }

    pub fn terminal(self) -> bool {
        matches!(self, Self::Acknowledged | Self::Failed | Self::Unknown)
    }
}

/// A subscription fence failure that keeps an outbox record from being claimed or dispatched.
///
/// Identity/revision/scope failures are caller errors: the intent is preserved and the caller must
/// present the right subscription. Revoked/paused/expired subscriptions and a closed notification
/// window are terminal: the caller abandons the intent with the matching reason instead of
/// delivering stale content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationOutboxFenceRefusal {
    SubscriptionMismatch,
    SubscriptionRevisionMismatch,
    SubscriptionScopeMismatch,
    SubscriptionRevoked,
    SubscriptionExpired,
    SubscriptionPaused,
    NotificationExpired,
}

impl NotificationOutboxFenceRefusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::SubscriptionMismatch => "notification_outbox_subscription_mismatch",
            Self::SubscriptionRevisionMismatch => {
                "notification_outbox_subscription_revision_mismatch"
            }
            Self::SubscriptionScopeMismatch => "notification_outbox_subscription_scope_mismatch",
            Self::SubscriptionRevoked => "notification_outbox_subscription_revoked",
            Self::SubscriptionExpired => "notification_outbox_subscription_expired",
            Self::SubscriptionPaused => "notification_outbox_subscription_paused",
            Self::NotificationExpired => "notification_outbox_notification_expired",
        }
    }

    pub const fn terminal_reason(self) -> Option<NotificationOutboxTerminalReason> {
        match self {
            Self::SubscriptionRevoked => {
                Some(NotificationOutboxTerminalReason::SubscriptionRevoked)
            }
            Self::SubscriptionExpired => {
                Some(NotificationOutboxTerminalReason::SubscriptionExpired)
            }
            Self::SubscriptionPaused => Some(NotificationOutboxTerminalReason::SubscriptionPaused),
            Self::NotificationExpired => {
                Some(NotificationOutboxTerminalReason::NotificationExpired)
            }
            Self::SubscriptionMismatch
            | Self::SubscriptionRevisionMismatch
            | Self::SubscriptionScopeMismatch => None,
        }
    }
}

/// The outcome of offering a record to a bounded outbox.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxAdmission {
    pub schema: String,
    pub outbox_id: DeliveryAttemptId,
    pub class: NotificationOutboxClass,
    pub protected: bool,
    pub admitted: bool,
    pub shed_reason: Option<NotificationOutboxShedReason>,
    pub admission_digest: String,
}

impl NotificationOutboxAdmission {
    pub fn admitted(record: &NotificationOutboxRecord) -> Result<Self, String> {
        Self::new(record, true, None)
    }

    pub fn shed(
        record: &NotificationOutboxRecord,
        reason: NotificationOutboxShedReason,
    ) -> Result<Self, String> {
        Self::new(record, false, Some(reason))
    }

    fn new(
        record: &NotificationOutboxRecord,
        admitted: bool,
        shed_reason: Option<NotificationOutboxShedReason>,
    ) -> Result<Self, String> {
        let class = record.effective_class();
        if !admitted && class.protected() {
            return Err("notification_outbox_protected_shed_refused".to_owned());
        }
        let mut admission = Self {
            schema: NOTIFICATION_OUTBOX_ADMISSION_SCHEMA.to_owned(),
            outbox_id: record.outbox_id,
            class,
            protected: class.protected(),
            admitted,
            shed_reason,
            admission_digest: String::new(),
        };
        admission.admission_digest = admission.digest();
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_ADMISSION_SCHEMA || self.outbox_id.as_uuid().is_nil()
        {
            return Err("notification_outbox_admission_header_invalid".to_owned());
        }
        if self.protected != self.class.protected() {
            return Err("notification_outbox_admission_class_mismatch".to_owned());
        }
        match (self.admitted, self.shed_reason) {
            (true, None) => {}
            (false, Some(_)) => {
                if self.protected {
                    return Err("notification_outbox_protected_shed_refused".to_owned());
                }
            }
            _ => return Err("notification_outbox_admission_outcome_invalid".to_owned()),
        }
        digest(
            &self.admission_digest,
            "notification_outbox_admission_digest",
        )?;
        if self.admission_digest != self.digest() {
            return Err("notification_outbox_admission_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "outbox_id": self.outbox_id,
            "class": self.class,
            "protected": self.protected,
            "admitted": self.admitted,
            "shed_reason": self.shed_reason,
        }))
    }
}

/// One reason bucket of the shed counters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxShedCount {
    pub reason: NotificationOutboxShedReason,
    pub count: u64,
}

/// Append-only-to-the-caller accounting of admissions and sheds.
///
/// The ledger exists so a bounded queue can explain itself: `shed` always equals the per-reason
/// buckets and `protected_shed` must stay zero. A ledger that claims a protected record was shed
/// is invalid, so queue pressure can never be used to hide a dropped approval or incident.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxPressure {
    pub schema: String,
    pub admitted: u64,
    pub shed: u64,
    pub best_effort_shed: u64,
    pub protected_shed: u64,
    pub shed_by_reason: Vec<NotificationOutboxShedCount>,
    pub pressure_digest: String,
}

impl NotificationOutboxPressure {
    pub fn new() -> Self {
        let mut pressure = Self {
            schema: NOTIFICATION_OUTBOX_PRESSURE_SCHEMA.to_owned(),
            admitted: 0,
            shed: 0,
            best_effort_shed: 0,
            protected_shed: 0,
            shed_by_reason: Vec::new(),
            pressure_digest: String::new(),
        };
        pressure.pressure_digest = pressure.digest();
        pressure
    }

    /// Record one admission outcome. Shedding a protected record is refused before it is counted.
    pub fn record(&mut self, admission: &NotificationOutboxAdmission) -> Result<(), String> {
        admission.validate()?;
        if admission.admitted {
            self.admitted = self
                .admitted
                .checked_add(1)
                .ok_or_else(|| "notification_outbox_pressure_overflow".to_owned())?;
        } else {
            let reason = admission
                .shed_reason
                .ok_or_else(|| "notification_outbox_admission_outcome_invalid".to_owned())?;
            if admission.protected {
                return Err("notification_outbox_protected_shed_refused".to_owned());
            }
            self.shed = self
                .shed
                .checked_add(1)
                .ok_or_else(|| "notification_outbox_pressure_overflow".to_owned())?;
            self.best_effort_shed = self
                .best_effort_shed
                .checked_add(1)
                .ok_or_else(|| "notification_outbox_pressure_overflow".to_owned())?;
            match self
                .shed_by_reason
                .iter_mut()
                .find(|entry| entry.reason == reason)
            {
                Some(entry) => {
                    entry.count = entry
                        .count
                        .checked_add(1)
                        .ok_or_else(|| "notification_outbox_pressure_overflow".to_owned())?;
                }
                None => {
                    self.shed_by_reason
                        .push(NotificationOutboxShedCount { reason, count: 1 });
                    self.shed_by_reason.sort_by_key(|entry| entry.reason);
                }
            }
        }
        self.pressure_digest = self.digest();
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_PRESSURE_SCHEMA {
            return Err("notification_outbox_pressure_header_invalid".to_owned());
        }
        if self.protected_shed != 0 {
            return Err("notification_outbox_protected_shed_observed".to_owned());
        }
        match self.best_effort_shed.checked_add(self.protected_shed) {
            Some(total) if total == self.shed => {}
            _ => return Err("notification_outbox_pressure_totals_invalid".to_owned()),
        }
        let mut total = 0u64;
        let mut previous: Option<NotificationOutboxShedReason> = None;
        for entry in &self.shed_by_reason {
            if entry.count == 0 {
                return Err("notification_outbox_pressure_bucket_empty".to_owned());
            }
            if previous.is_some_and(|reason| reason >= entry.reason) {
                return Err("notification_outbox_pressure_buckets_noncanonical".to_owned());
            }
            previous = Some(entry.reason);
            total = total
                .checked_add(entry.count)
                .ok_or_else(|| "notification_outbox_pressure_overflow".to_owned())?;
        }
        if total != self.shed {
            return Err("notification_outbox_pressure_totals_invalid".to_owned());
        }
        digest(&self.pressure_digest, "notification_outbox_pressure_digest")?;
        if self.pressure_digest != self.digest() {
            return Err("notification_outbox_pressure_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "admitted": self.admitted,
            "shed": self.shed,
            "best_effort_shed": self.best_effort_shed,
            "protected_shed": self.protected_shed,
            "shed_by_reason": self.shed_by_reason,
        }))
    }
}

impl Default for NotificationOutboxPressure {
    fn default() -> Self {
        Self::new()
    }
}

/// One committed outbox transition, used to rebuild a record's state instead of trusting it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxTransition {
    pub schema: String,
    pub outbox_id: DeliveryAttemptId,
    pub revision: u64,
    pub from: NotificationOutboxState,
    pub to: NotificationOutboxState,
    pub delivery_attempt_id: DeliveryAttemptId,
    pub attempt_number: u32,
    pub lease_token: u64,
    pub occurred_at_unix_ms: u64,
    pub transition_digest: String,
}

impl NotificationOutboxTransition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        outbox_id: DeliveryAttemptId,
        revision: u64,
        from: NotificationOutboxState,
        to: NotificationOutboxState,
        delivery_attempt_id: DeliveryAttemptId,
        attempt_number: u32,
        lease_token: u64,
        occurred_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut transition = Self {
            schema: NOTIFICATION_OUTBOX_TRANSITION_SCHEMA.to_owned(),
            outbox_id,
            revision,
            from,
            to,
            delivery_attempt_id,
            attempt_number,
            lease_token,
            occurred_at_unix_ms,
            transition_digest: String::new(),
        };
        transition.transition_digest = transition.digest();
        transition.validate()?;
        Ok(transition)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_TRANSITION_SCHEMA
            || self.outbox_id.as_uuid().is_nil()
            || self.delivery_attempt_id.as_uuid().is_nil()
            || self.revision == 0
            || self.attempt_number == 0
            || self.occurred_at_unix_ms == 0
        {
            return Err("notification_outbox_transition_header_invalid".to_owned());
        }
        if self.from != self.to && self.from.transition(self.to).is_err() {
            return Err("notification_outbox_transition_edge_invalid".to_owned());
        }
        // `from == to` is a same-state commit: an expired claim was replaced by a new attempt and
        // the token advanced. The rebuild step still requires the token to advance.
        if self.from == self.to && self.lease_token == 0 {
            return Err("notification_outbox_transition_edge_invalid".to_owned());
        }
        digest(
            &self.transition_digest,
            "notification_outbox_transition_digest",
        )?;
        if self.transition_digest != self.digest() {
            return Err("notification_outbox_transition_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "outbox_id": self.outbox_id,
            "revision": self.revision,
            "from": self.from,
            "to": self.to,
            "delivery_attempt_id": self.delivery_attempt_id,
            "attempt_number": self.attempt_number,
            "lease_token": self.lease_token,
            "occurred_at_unix_ms": self.occurred_at_unix_ms,
        }))
    }
}

/// State rebuilt from a transition chain. Callers compare it with the stored record; a mismatch is
/// `notification_outbox_rebuild_diverged` and must fail closed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxRebuild {
    pub outbox_id: DeliveryAttemptId,
    pub state: NotificationOutboxState,
    pub revision: u64,
    /// Highest lease token observed on a transition into `Claimed`.
    pub last_claim_token: u64,
    pub transitions: usize,
}

/// Rebuild an outbox state from its recorded transitions.
///
/// The chain must start at the record's creation state (revision 1) and every step must be an
/// allowed edge (or a same-state commit that replaces an expired claim) with contiguous revisions
/// and a non-decreasing clock; every transition into `Claimed` must advance the lease token so a
/// stale holder can never be re-fenced into a newer claim. This makes
/// `pending -> claimed -> submitted -> acknowledged/failed/unknown` reconstructable from committed
/// transitions instead of trusting a mutable row.
pub fn rebuild_notification_outbox_state(
    outbox_id: DeliveryAttemptId,
    initial_state: NotificationOutboxState,
    transitions: &[NotificationOutboxTransition],
) -> Result<NotificationOutboxRebuild, String> {
    if outbox_id.as_uuid().is_nil() {
        return Err("notification_outbox_rebuild_identity_missing".to_owned());
    }
    let mut state = initial_state;
    let mut revision = 1u64;
    let mut last_claim_token = 0u64;
    let mut last_time = 0u64;
    for transition in transitions {
        transition.validate()?;
        if transition.outbox_id != outbox_id {
            return Err("notification_outbox_rebuild_identity_mismatch".to_owned());
        }
        if transition.from != state {
            return Err("notification_outbox_rebuild_diverged".to_owned());
        }
        let expected_revision = revision
            .checked_add(1)
            .ok_or_else(|| "notification_outbox_rebuild_revision_overflow".to_owned())?;
        if transition.revision != expected_revision {
            return Err("notification_outbox_rebuild_revision_gap".to_owned());
        }
        if transition.occurred_at_unix_ms < last_time {
            return Err("notification_outbox_rebuild_clock_regression".to_owned());
        }
        if transition.to == NotificationOutboxState::Claimed
            && transition.lease_token <= last_claim_token
        {
            return Err("notification_outbox_rebuild_claim_token_not_advanced".to_owned());
        }
        if transition.to == NotificationOutboxState::Claimed {
            last_claim_token = transition.lease_token;
        }
        state = transition.to;
        revision = transition.revision;
        last_time = transition.occurred_at_unix_ms;
    }
    Ok(NotificationOutboxRebuild {
        outbox_id,
        state,
        revision,
        last_claim_token,
        transitions: transitions.len(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxLease {
    pub schema: String,
    pub outbox_id: DeliveryAttemptId,
    pub notification_id: NotificationId,
    pub worker_id: String,
    pub authority_epoch: u64,
    pub lease_token: u64,
    pub expires_at_unix_ms: u64,
    pub lease_digest: String,
}

impl NotificationOutboxLease {
    fn from_record(record: &NotificationOutboxRecord) -> Result<Self, String> {
        let worker_id = record
            .lease_owner
            .clone()
            .ok_or_else(|| "notification_outbox_lease_owner_missing".to_owned())?;
        let expires_at_unix_ms = record
            .lease_expires_at_unix_ms
            .ok_or_else(|| "notification_outbox_lease_expiry_missing".to_owned())?;
        let mut lease = Self {
            schema: NOTIFICATION_OUTBOX_LEASE_SCHEMA.to_owned(),
            outbox_id: record.outbox_id,
            notification_id: record.notification.notification_id,
            worker_id,
            authority_epoch: record.authority_epoch,
            lease_token: record.lease_token,
            expires_at_unix_ms,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_LEASE_SCHEMA
            || self.outbox_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.lease_token == 0
            || self.expires_at_unix_ms == 0
        {
            return Err("notification_outbox_lease_header_invalid".to_owned());
        }
        required(
            &self.worker_id,
            "notification_outbox_lease_worker",
            NOTIFICATION_OUTBOX_MAX_WORKER_ID_BYTES,
        )?;
        digest(&self.lease_digest, "notification_outbox_lease_digest")?;
        if self.lease_digest != self.digest() {
            return Err("notification_outbox_lease_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "outbox_id": self.outbox_id,
            "notification_id": self.notification_id,
            "worker_id": self.worker_id,
            "authority_epoch": self.authority_epoch,
            "lease_token": self.lease_token,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationOutboxRecord {
    pub schema: String,
    pub outbox_id: DeliveryAttemptId,
    pub notification: Notification,
    pub attempt: DeliveryAttempt,
    pub state: NotificationOutboxState,
    pub class: NotificationOutboxClass,
    pub revision: u64,
    pub authority_epoch: u64,
    #[serde(default)]
    pub lease_owner: Option<String>,
    pub lease_token: u64,
    /// Monotonic claim counter. It survives a lease release, so a released or completed lease can
    /// never match a later claim even when the same worker id reclaims the record.
    #[serde(default)]
    pub claim_generation: u64,
    #[serde(default)]
    pub lease_expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub terminal_reason: Option<NotificationOutboxTerminalReason>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub record_digest: String,
}

impl NotificationOutboxRecord {
    pub fn new(
        mut notification: Notification,
        class: NotificationOutboxClass,
        subscription_id: SubscriptionId,
        authority_epoch: u64,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        notification.validate()?;
        if authority_epoch == 0 || created_at_unix_ms == 0 {
            return Err("notification_outbox_creation_context_invalid".to_owned());
        }
        match notification.status {
            NotificationStatus::Pending => {
                notification.transition_status(NotificationStatus::Queued)?;
            }
            NotificationStatus::Queued => {}
            _ => return Err("notification_outbox_notification_not_pending".to_owned()),
        }
        let attempt = DeliveryAttempt::new(
            notification.notification_id,
            subscription_id,
            authority_epoch,
            created_at_unix_ms,
        )?;
        let mut record = Self {
            schema: NOTIFICATION_OUTBOX_SCHEMA.to_owned(),
            outbox_id: DeliveryAttemptId::new(),
            notification,
            attempt,
            state: NotificationOutboxState::Pending,
            class,
            revision: 1,
            authority_epoch,
            lease_owner: None,
            lease_token: 0,
            claim_generation: 0,
            lease_expires_at_unix_ms: None,
            terminal_reason: None,
            created_at_unix_ms,
            updated_at_unix_ms: created_at_unix_ms,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    /// The class that actually governs shedding: `max(declared, floor proven by the notification)`.
    ///
    /// `validate()` refuses an understated declaration, but shedding and outstanding-counting use
    /// the escalated class so a record that already exists cannot be demoted by rewriting its
    /// declared class.
    pub fn effective_class(&self) -> NotificationOutboxClass {
        self.class
            .max(NotificationOutboxClass::required_floor(&self.notification))
    }

    /// Classify the subscription fence for this record.
    ///
    /// Callers must validate the record itself first; this method only compares the presented
    /// subscription with the intent that the record already carries.
    pub fn subscription_fence(
        &self,
        subscription: &Subscription,
        now_unix_ms: u64,
    ) -> Result<(), NotificationOutboxFenceRefusal> {
        if subscription.subscription_id != self.attempt.subscription_id {
            return Err(NotificationOutboxFenceRefusal::SubscriptionMismatch);
        }
        if subscription.revision != self.notification.subscription_revision {
            return Err(NotificationOutboxFenceRefusal::SubscriptionRevisionMismatch);
        }
        match subscription.status {
            SubscriptionStatus::Revoked => {
                return Err(NotificationOutboxFenceRefusal::SubscriptionRevoked);
            }
            SubscriptionStatus::Paused => {
                return Err(NotificationOutboxFenceRefusal::SubscriptionPaused);
            }
            SubscriptionStatus::Expired => {
                return Err(NotificationOutboxFenceRefusal::SubscriptionExpired);
            }
            SubscriptionStatus::Active => {}
        }
        if subscription.expires_at_unix_ms <= now_unix_ms {
            return Err(NotificationOutboxFenceRefusal::SubscriptionExpired);
        }
        if self.notification.expires_at_unix_ms <= now_unix_ms {
            return Err(NotificationOutboxFenceRefusal::NotificationExpired);
        }
        if self.notification.recipient_id != subscription.recipient_id
            || self.notification.project_id != subscription.project_id
            || !subscription.channels.contains(&self.notification.channel)
            || !scope_is_subset(&self.notification.scope, &subscription.scope)
        {
            return Err(NotificationOutboxFenceRefusal::SubscriptionScopeMismatch);
        }
        Ok(())
    }

    /// Validating form of [`Self::subscription_fence`] used by order-sensitive callers.
    pub fn check_subscription_fence(
        &self,
        subscription: &Subscription,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        subscription.validate()?;
        self.subscription_fence(subscription, now_unix_ms)
            .map_err(|refusal| refusal.code().to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_OUTBOX_SCHEMA
            || self.outbox_id.as_uuid().is_nil()
            || self.revision == 0
            || self.authority_epoch == 0
            || self.created_at_unix_ms == 0
            || self.updated_at_unix_ms < self.created_at_unix_ms
        {
            return Err("notification_outbox_header_invalid".to_owned());
        }
        self.notification.validate()?;
        self.attempt.validate()?;
        let floor = NotificationOutboxClass::required_floor(&self.notification);
        if self.class < floor {
            return Err("notification_outbox_class_understated".to_owned());
        }
        if self.class == NotificationOutboxClass::Actionable && floor != self.class {
            return Err("notification_outbox_class_invalid".to_owned());
        }
        if self.state == NotificationOutboxState::Failed && self.terminal_reason.is_none() {
            return Err("notification_outbox_terminal_reason_missing".to_owned());
        }
        if self.state != NotificationOutboxState::Failed && self.terminal_reason.is_some() {
            return Err("notification_outbox_terminal_reason_unexpected".to_owned());
        }
        if self.notification.notification_id != self.attempt.notification_id
            || self.authority_epoch != self.attempt.authority_epoch
        {
            return Err("notification_outbox_identity_mismatch".to_owned());
        }
        let expected_attempt_status = match self.state {
            NotificationOutboxState::Pending => DeliveryAttemptStatus::Pending,
            NotificationOutboxState::Claimed => DeliveryAttemptStatus::Claimed,
            NotificationOutboxState::Submitted => DeliveryAttemptStatus::Submitted,
            NotificationOutboxState::Acknowledged => DeliveryAttemptStatus::Acknowledged,
            NotificationOutboxState::Failed => DeliveryAttemptStatus::Failed,
            NotificationOutboxState::Unknown => DeliveryAttemptStatus::Unknown,
        };
        if self.attempt.status != expected_attempt_status {
            return Err("notification_outbox_attempt_state_mismatch".to_owned());
        }
        if self.notification.status
            != match self.state {
                NotificationOutboxState::Acknowledged => NotificationStatus::Delivered,
                NotificationOutboxState::Failed => NotificationStatus::Failed,
                NotificationOutboxState::Unknown => NotificationStatus::Unknown,
                _ => NotificationStatus::Queued,
            }
        {
            return Err("notification_outbox_notification_state_mismatch".to_owned());
        }
        match (
            &self.lease_owner,
            self.lease_token,
            self.lease_expires_at_unix_ms,
        ) {
            (None, 0, None) => {}
            (Some(owner), token, Some(expires)) => {
                required(
                    owner,
                    "notification_outbox_lease_owner",
                    NOTIFICATION_OUTBOX_MAX_WORKER_ID_BYTES,
                )?;
                if token == 0 || expires <= self.updated_at_unix_ms {
                    return Err("notification_outbox_lease_invalid".to_owned());
                }
                if token != self.claim_generation {
                    return Err("notification_outbox_lease_generation_mismatch".to_owned());
                }
            }
            _ => return Err("notification_outbox_lease_partial".to_owned()),
        }
        if self.lease_token > self.claim_generation {
            return Err("notification_outbox_lease_generation_mismatch".to_owned());
        }
        digest(&self.record_digest, "notification_outbox_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("notification_outbox_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn claim(
        &mut self,
        worker_id: impl Into<String>,
        authority_epoch: u64,
        now_unix_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<NotificationOutboxLease, String> {
        self.validate()?;
        let worker_id = worker_id.into();
        required(
            &worker_id,
            "notification_outbox_worker_id",
            NOTIFICATION_OUTBOX_MAX_WORKER_ID_BYTES,
        )?;
        if authority_epoch != self.authority_epoch || now_unix_ms < self.updated_at_unix_ms {
            return Err("notification_outbox_authority_or_clock_conflict".to_owned());
        }
        if lease_ttl_ms == 0 || lease_ttl_ms > NOTIFICATION_OUTBOX_MAX_LEASE_MS {
            return Err("notification_outbox_lease_ttl_invalid".to_owned());
        }
        if self.state == NotificationOutboxState::Claimed {
            if self
                .lease_expires_at_unix_ms
                .is_some_and(|expires| expires > now_unix_ms)
            {
                return Err("notification_outbox_lease_active".to_owned());
            }
            let mut attempt = DeliveryAttempt::new(
                self.notification.notification_id,
                self.attempt.subscription_id,
                authority_epoch,
                now_unix_ms,
            )?;
            attempt.attempt_number = self
                .attempt
                .attempt_number
                .checked_add(1)
                .ok_or_else(|| "notification_outbox_attempt_overflow".to_owned())?;
            attempt.attempt_digest = attempt.digest();
            attempt.validate()?;
            self.attempt = attempt;
            self.state = NotificationOutboxState::Pending;
            self.lease_owner = None;
            self.lease_expires_at_unix_ms = None;
        }
        if self.state != NotificationOutboxState::Pending {
            return Err("notification_outbox_not_claimable".to_owned());
        }
        self.state = self
            .state
            .transition(NotificationOutboxState::Claimed)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Claimed, now_unix_ms)?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "notification_outbox_revision_overflow".to_owned())?;
        self.claim_generation = self
            .claim_generation
            .checked_add(1)
            .ok_or_else(|| "notification_outbox_claim_generation_overflow".to_owned())?;
        self.lease_token = self.claim_generation;
        self.lease_owner = Some(worker_id);
        self.lease_expires_at_unix_ms = Some(
            now_unix_ms
                .checked_add(lease_ttl_ms)
                .ok_or_else(|| "notification_outbox_lease_time_overflow".to_owned())?,
        );
        self.updated_at_unix_ms = now_unix_ms;
        self.refresh_digest_and_validate()?;
        NotificationOutboxLease::from_record(self)
    }

    pub fn mark_submitted(
        &mut self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.assert_lease(lease, now_unix_ms)?;
        if self.state != NotificationOutboxState::Claimed {
            return Err("notification_outbox_submit_state_invalid".to_owned());
        }
        self.state = self
            .state
            .transition(NotificationOutboxState::Submitted)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Submitted, now_unix_ms)?;
        self.bump_revision(now_unix_ms)?;
        Ok(())
    }

    pub fn acknowledge(
        &mut self,
        lease: &NotificationOutboxLease,
        receipt: &DeliveryReceipt,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.assert_lease(lease, now_unix_ms)?;
        receipt.validate()?;
        if self.state != NotificationOutboxState::Submitted
            || receipt.notification_id != self.notification.notification_id
            || receipt.delivery_attempt_id != self.attempt.delivery_attempt_id
            || receipt.status != crate::DeliveryReceiptStatus::Acknowledged
        {
            return Err("notification_outbox_receipt_mismatch".to_owned());
        }
        self.state = self
            .state
            .transition(NotificationOutboxState::Acknowledged)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Acknowledged, now_unix_ms)?;
        self.notification
            .transition_status(NotificationStatus::Delivered)?;
        self.clear_lease();
        self.bump_revision(now_unix_ms)?;
        Ok(())
    }

    pub fn fail(
        &mut self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.assert_lease(lease, now_unix_ms)?;
        if !matches!(
            self.state,
            NotificationOutboxState::Claimed | NotificationOutboxState::Submitted
        ) {
            return Err("notification_outbox_fail_state_invalid".to_owned());
        }
        self.terminate_failed(
            NotificationOutboxTerminalReason::DeliveryFailed,
            now_unix_ms,
        )
    }

    /// Release a claim that was never submitted, for a bounded shutdown drain.
    ///
    /// Nothing was dispatched, so returning the record to `Pending` cannot duplicate an action.
    /// The lease token is kept (only readable through the new claim) so a stale holder still
    /// cannot submit after the release. A submitted record can never be released: its receipt path
    /// is `acknowledge`/`reconcile_unknown`.
    pub fn release(
        &mut self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.assert_lease(lease, now_unix_ms)?;
        if self.state != NotificationOutboxState::Claimed {
            return Err("notification_outbox_release_state_invalid".to_owned());
        }
        self.state = self
            .state
            .transition(NotificationOutboxState::Pending)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Pending, now_unix_ms)?;
        self.clear_lease();
        self.bump_revision(now_unix_ms)?;
        Ok(())
    }

    /// Terminate a leased, not-yet-submitted claim with an explicit reason (revocation/expiry).
    pub fn abandon(
        &mut self,
        lease: &NotificationOutboxLease,
        reason: NotificationOutboxTerminalReason,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.assert_lease(lease, now_unix_ms)?;
        if self.state != NotificationOutboxState::Claimed {
            return Err("notification_outbox_abandon_state_invalid".to_owned());
        }
        self.terminate_failed(reason, now_unix_ms)
    }

    /// Terminate a record that holds no live claim (pending, or a crashed claim whose lease has
    /// already expired). A submitted record is never abandoned here: only its receipt can resolve
    /// it, so an in-flight send cannot be silently retried or cancelled.
    pub fn abandon_unclaimed(
        &mut self,
        reason: NotificationOutboxTerminalReason,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        if reason == NotificationOutboxTerminalReason::DeliveryFailed {
            return Err("notification_outbox_terminal_reason_invalid".to_owned());
        }
        match self.state {
            NotificationOutboxState::Pending => {}
            NotificationOutboxState::Claimed => {
                if self
                    .lease_expires_at_unix_ms
                    .is_some_and(|expires| expires > now_unix_ms)
                {
                    return Err("notification_outbox_abandon_lease_active".to_owned());
                }
            }
            _ => return Err("notification_outbox_abandon_state_invalid".to_owned()),
        }
        self.terminate_failed(reason, now_unix_ms)
    }

    fn terminate_failed(
        &mut self,
        reason: NotificationOutboxTerminalReason,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.state = self
            .state
            .transition(NotificationOutboxState::Failed)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Failed, now_unix_ms)?;
        self.notification
            .transition_status(NotificationStatus::Failed)?;
        self.terminal_reason = Some(reason);
        self.clear_lease();
        self.bump_revision(now_unix_ms)?;
        Ok(())
    }

    pub fn reconcile_unknown(
        &mut self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.assert_lease(lease, now_unix_ms)?;
        if !matches!(
            self.state,
            NotificationOutboxState::Claimed | NotificationOutboxState::Submitted
        ) {
            return Err("notification_outbox_unknown_state_invalid".to_owned());
        }
        self.state = self
            .state
            .transition(NotificationOutboxState::Unknown)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Unknown, now_unix_ms)?;
        self.notification
            .transition_status(NotificationStatus::Unknown)?;
        self.clear_lease();
        self.bump_revision(now_unix_ms)?;
        Ok(())
    }

    pub fn dispatch_intent(
        &self,
        lease: &NotificationOutboxLease,
    ) -> Result<NotificationDispatchIntent, String> {
        self.validate()?;
        lease.validate()?;
        if self.state != NotificationOutboxState::Claimed
            || lease.outbox_id != self.outbox_id
            || lease.lease_token != self.lease_token
        {
            return Err("notification_outbox_dispatch_lease_mismatch".to_owned());
        }
        NotificationDispatchIntent::from_record(self, lease)
    }

    pub fn current_lease(&self) -> Result<NotificationOutboxLease, String> {
        NotificationOutboxLease::from_record(self)
    }

    fn assert_lease(
        &self,
        lease: &NotificationOutboxLease,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        lease.validate()?;
        if now_unix_ms > lease.expires_at_unix_ms {
            return Err("notification_outbox_lease_expired".to_owned());
        }
        if lease.outbox_id != self.outbox_id
            || lease.notification_id != self.notification.notification_id
            || lease.worker_id != self.lease_owner.as_deref().unwrap_or_default()
            || lease.authority_epoch != self.authority_epoch
            || lease.lease_token != self.lease_token
        {
            return Err("notification_outbox_lease_fence_mismatch".to_owned());
        }
        Ok(())
    }

    fn clear_lease(&mut self) {
        self.lease_owner = None;
        self.lease_token = 0;
        self.lease_expires_at_unix_ms = None;
    }

    fn bump_revision(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "notification_outbox_revision_overflow".to_owned())?;
        self.updated_at_unix_ms = now_unix_ms;
        self.refresh_digest_and_validate()
    }

    fn refresh_digest_and_validate(&mut self) -> Result<(), String> {
        self.record_digest = self.digest();
        self.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "outbox_id": self.outbox_id,
            "notification": self.notification,
            "attempt": self.attempt,
            "state": self.state,
            "class": self.class,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "lease_owner": self.lease_owner,
            "lease_token": self.lease_token,
            "claim_generation": self.claim_generation,
            "lease_expires_at_unix_ms": self.lease_expires_at_unix_ms,
            "terminal_reason": self.terminal_reason,
            "created_at_unix_ms": self.created_at_unix_ms,
            "updated_at_unix_ms": self.updated_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationDispatchIntent {
    pub schema: String,
    pub outbox_id: DeliveryAttemptId,
    pub notification_id: NotificationId,
    pub delivery_attempt_id: DeliveryAttemptId,
    pub channel: NotificationChannel,
    pub recipient_id: String,
    pub content_digest: String,
    pub lease: NotificationOutboxLease,
    pub intent_digest: String,
}

impl NotificationDispatchIntent {
    fn from_record(
        record: &NotificationOutboxRecord,
        lease: &NotificationOutboxLease,
    ) -> Result<Self, String> {
        let mut intent = Self {
            schema: NOTIFICATION_DISPATCH_INTENT_SCHEMA.to_owned(),
            outbox_id: record.outbox_id,
            notification_id: record.notification.notification_id,
            delivery_attempt_id: record.attempt.delivery_attempt_id,
            channel: record.notification.channel,
            recipient_id: record.notification.recipient_id.clone(),
            content_digest: record.notification.content_digest(),
            lease: lease.clone(),
            intent_digest: String::new(),
        };
        intent.intent_digest = intent.digest();
        intent.validate()?;
        Ok(intent)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_DISPATCH_INTENT_SCHEMA
            || self.outbox_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.delivery_attempt_id.as_uuid().is_nil()
        {
            return Err("notification_dispatch_intent_header_invalid".to_owned());
        }
        required(&self.recipient_id, "notification_dispatch_recipient", 512)?;
        digest(&self.content_digest, "notification_dispatch_content_digest")?;
        self.lease.validate()?;
        if self.lease.outbox_id != self.outbox_id
            || self.lease.notification_id != self.notification_id
        {
            return Err("notification_dispatch_lease_identity_mismatch".to_owned());
        }
        digest(&self.intent_digest, "notification_dispatch_intent_digest")?;
        if self.intent_digest != self.digest() {
            return Err("notification_dispatch_intent_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "outbox_id": self.outbox_id,
            "notification_id": self.notification_id,
            "delivery_attempt_id": self.delivery_attempt_id,
            "channel": self.channel,
            "recipient_id": self.recipient_id,
            "content_digest": self.content_digest,
            "lease": self.lease,
        }))
    }
}
