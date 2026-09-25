//! Cancellation, revocation, expiry and Unknown recovery contracts for notification delivery.
//!
//! The planner is deliberately conservative. A send that may have started is never retried from
//! this contract, and a cancellation request is never rewritten as a delivered result. Known
//! pre-send failures can request a fresh authority/delivery-key admission; the planner itself does
//! not generate either key or call a connector.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NOTIFICATION_RECOVERY_SCHEMA: &str = "kiana.notification-recovery.v1";
pub const NOTIFICATION_RECOVERY_MIN_RETRY_LIMIT: u8 = 1;
pub const NOTIFICATION_RECOVERY_MAX_RETRY_LIMIT: u8 = 8;

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
    pub attempt: u8,
    pub max_pre_send_retries: u8,
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
            NotificationRecoveryState::Revoked => (
                NotificationRecoveryDisposition::TerminalNoSend,
                "subscription_revoked",
                self.attempt,
                false,
            ),
            NotificationRecoveryState::Expired => (
                NotificationRecoveryDisposition::TerminalNoSend,
                "notification_expired",
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
                "pre_send_retry_not_admissible",
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
        NotificationRecoveryPlan::new(
            self.notification_id.clone(),
            self.delivery_key_digest.clone(),
            self.authority_epoch,
            self.state,
            disposition,
            reason,
            next_attempt,
            new_keys,
        )
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
    pub next_attempt: u8,
    pub new_authority_and_delivery_key_required: bool,
    pub plan_digest: String,
}

impl NotificationRecoveryPlan {
    fn new(
        notification_id: String,
        delivery_key_digest: String,
        authority_epoch: u64,
        state: NotificationRecoveryState,
        disposition: NotificationRecoveryDisposition,
        reason: &str,
        next_attempt: u8,
        new_authority_and_delivery_key_required: bool,
    ) -> Result<Self, String> {
        let mut plan = Self {
            schema: NOTIFICATION_RECOVERY_SCHEMA.to_owned(),
            notification_id,
            delivery_key_digest,
            authority_epoch,
            state,
            disposition,
            reason: reason.to_owned(),
            next_attempt,
            new_authority_and_delivery_key_required,
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
        digest(&self.plan_digest, "notification_recovery_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("notification_recovery_plan_digest_mismatch".to_owned());
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
            "disposition": self.disposition,
            "reason": self.reason,
            "next_attempt": self.next_attempt,
            "new_authority_and_delivery_key_required": self.new_authority_and_delivery_key_required,
        }))
    }
}
