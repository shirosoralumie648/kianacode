//! Typed notification action commands.
//!
//! A notification action is a server-bound command candidate. It carries an opaque action ref and
//! exact target material, but it is not itself approval, HumanTask mutation or capability effect.
//! The ControlPlane must re-admit it against the authoritative action ref before any command runs.

use crate::{
    canonical_journal_bytes, json_digest, redact_value, ActionRef, ActionRefId, NotificationId,
    RequestId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const NOTIFICATION_ACTION_COMMAND_SCHEMA: &str = "kiana.notification-action-command.v1";
pub const NOTIFICATION_ACTION_MAX_PAYLOAD_BYTES: usize = 16 * 1024;

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
pub enum NotificationActionKind {
    Acknowledge,
    Approve,
    Review,
    Reconcile,
    Snooze,
    Escalate,
    Delegate,
    Withdraw,
}

impl NotificationActionKind {
    pub fn command(self) -> &'static str {
        match self {
            Self::Acknowledge => "notification.acknowledge",
            Self::Approve => "notification.approve",
            Self::Review => "notification.review",
            Self::Reconcile => "notification.reconcile",
            Self::Snooze => "notification.snooze",
            Self::Escalate => "notification.escalate",
            Self::Delegate => "notification.delegate",
            Self::Withdraw => "notification.withdraw",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationActionCommand {
    pub schema: String,
    pub command_id: RequestId,
    pub action_ref: ActionRef,
    pub action: NotificationActionKind,
    pub notification_id: NotificationId,
    pub recipient_id: String,
    pub expected_revision: u64,
    pub expected_target_digest: String,
    pub expected_authority_epoch: u64,
    pub expected_source_cursor: u64,
    pub payload: Value,
    pub submitted_by: String,
    pub idempotency_key: String,
    pub command_digest: String,
}

impl NotificationActionCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action_ref: ActionRef,
        action: NotificationActionKind,
        notification_id: NotificationId,
        recipient_id: impl Into<String>,
        expected_revision: u64,
        expected_target_digest: impl Into<String>,
        expected_authority_epoch: u64,
        expected_source_cursor: u64,
        payload: Value,
        submitted_by: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, String> {
        let mut command = Self {
            schema: NOTIFICATION_ACTION_COMMAND_SCHEMA.to_owned(),
            command_id: RequestId::new(),
            action_ref,
            action,
            notification_id,
            recipient_id: recipient_id.into(),
            expected_revision,
            expected_target_digest: expected_target_digest.into(),
            expected_authority_epoch,
            expected_source_cursor,
            payload,
            submitted_by: submitted_by.into(),
            idempotency_key: idempotency_key.into(),
            command_digest: String::new(),
        };
        command.command_digest = command.digest();
        command.validate()?;
        Ok(command)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ACTION_COMMAND_SCHEMA
            || self.command_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.expected_revision == 0
            || self.expected_authority_epoch == 0
            || self.expected_source_cursor == 0
        {
            return Err("notification_action_command_header_invalid".to_owned());
        }
        self.action_ref.validate()?;
        if self.action_ref.command != self.action.command() {
            return Err("notification_action_command_ref_kind_mismatch".to_owned());
        }
        if self.action_ref.target_revision != self.expected_revision
            || !self
                .action_ref
                .scope
                .iter()
                .any(|scope| scope == &format!("notification:{}", self.notification_id))
        {
            return Err("notification_action_command_target_scope_mismatch".to_owned());
        }
        required(&self.recipient_id, "notification_action_recipient", 256)?;
        required(&self.submitted_by, "notification_action_submitted_by", 256)?;
        required(
            &self.idempotency_key,
            "notification_action_idempotency",
            256,
        )?;
        digest(
            &self.expected_target_digest,
            "notification_action_target_digest",
        )?;
        let encoded = canonical_journal_bytes(&self.payload)
            .map_err(|_| "notification_action_payload_invalid".to_owned())?;
        if encoded.len() > NOTIFICATION_ACTION_MAX_PAYLOAD_BYTES
            || redact_value(&self.payload) != self.payload
        {
            return Err("notification_action_payload_invalid".to_owned());
        }
        digest(&self.command_digest, "notification_action_command_digest")?;
        if self.command_digest != self.digest() {
            return Err("notification_action_command_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "command_id": self.command_id,
            "action_ref": self.action_ref,
            "action": self.action,
            "notification_id": self.notification_id,
            "recipient_id": self.recipient_id,
            "expected_revision": self.expected_revision,
            "expected_target_digest": self.expected_target_digest,
            "expected_authority_epoch": self.expected_authority_epoch,
            "expected_source_cursor": self.expected_source_cursor,
            "payload": self.payload,
            "submitted_by": self.submitted_by,
            "idempotency_key": self.idempotency_key,
        }))
    }

    pub fn action_ref_id(&self) -> ActionRefId {
        self.action_ref.action_ref_id
    }
}
