//! Bounded notification outbox and delivery lease contracts.
//!
//! An outbox record is a server-owned delivery intent. It is not a channel adapter, a Broker
//! permit, a notification bus or an agent loop. The record carries only redaction-safe identity
//! and digest material; an external adapter must still return a separately validated receipt.

use crate::{
    json_digest, DeliveryAttempt, DeliveryAttemptId, DeliveryAttemptStatus, DeliveryReceipt,
    Notification, NotificationChannel, NotificationId, NotificationStatus, SubscriptionId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_OUTBOX_SCHEMA: &str = "kiana.notification-outbox.v1";
pub const NOTIFICATION_OUTBOX_LEASE_SCHEMA: &str = "kiana.notification-outbox-lease.v1";
pub const NOTIFICATION_DISPATCH_INTENT_SCHEMA: &str = "kiana.notification-dispatch-intent.v1";
pub const NOTIFICATION_OUTBOX_MAX_WORKER_ID_BYTES: usize = 128;
pub const NOTIFICATION_OUTBOX_MAX_LEASE_MS: u64 = 24 * 60 * 60 * 1_000;

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
                    Self::Submitted | Self::Failed | Self::Unknown
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
    pub revision: u64,
    pub authority_epoch: u64,
    #[serde(default)]
    pub lease_owner: Option<String>,
    pub lease_token: u64,
    #[serde(default)]
    pub lease_expires_at_unix_ms: Option<u64>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub record_digest: String,
}

impl NotificationOutboxRecord {
    pub fn new(
        mut notification: Notification,
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
            revision: 1,
            authority_epoch,
            lease_owner: None,
            lease_token: 0,
            lease_expires_at_unix_ms: None,
            created_at_unix_ms,
            updated_at_unix_ms: created_at_unix_ms,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
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
            }
            _ => return Err("notification_outbox_lease_partial".to_owned()),
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
        self.lease_token = self.lease_token.checked_add(1).unwrap_or(1);
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
        self.state = self
            .state
            .transition(NotificationOutboxState::Failed)
            .map_err(str::to_owned)?;
        self.attempt
            .transition_status(DeliveryAttemptStatus::Failed, now_unix_ms)?;
        self.notification
            .transition_status(NotificationStatus::Failed)?;
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
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "lease_owner": self.lease_owner,
            "lease_token": self.lease_token,
            "lease_expires_at_unix_ms": self.lease_expires_at_unix_ms,
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
