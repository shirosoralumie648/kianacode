//! ControlPlane re-admission boundary for notification action commands.
//!
//! This gate validates a notification action against server-owned ref/target material and returns
//! a typed admission only. It does not mutate HumanTask/Approval, append an event, consume a
//! permit, invoke Broker or execute the action.

use kiana_domain::{ActionRef, NotificationActionCommand, NotificationActionKind};
use thiserror::Error;

pub const NOTIFICATION_ACTION_GATE_SCHEMA: &str = "kiana.notification-action-gate.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationActionAdmission {
    pub schema: String,
    pub action: NotificationActionKind,
    pub action_ref_id: kiana_domain::ActionRefId,
    pub command_digest: String,
    pub control_plane_required: bool,
    pub direct_effect: bool,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationActionGateError {
    #[error("notification_action_invalid:{0}")]
    Invalid(String),
    #[error("notification_action_ref_not_authoritative")]
    RefNotAuthoritative,
    #[error("notification_action_recipient_mismatch")]
    RecipientMismatch,
    #[error("notification_action_revision_conflict")]
    RevisionConflict,
    #[error("notification_action_digest_conflict")]
    DigestConflict,
    #[error("notification_action_authority_conflict")]
    AuthorityConflict,
    #[error("notification_action_cursor_conflict")]
    CursorConflict,
    #[error("notification_action_expired")]
    Expired,
}

pub struct NotificationActionGate;

impl NotificationActionGate {
    #[allow(clippy::too_many_arguments)]
    pub fn admit(
        command: &NotificationActionCommand,
        authoritative_ref: &ActionRef,
        server_recipient_id: &str,
        current_revision: u64,
        current_target_digest: &str,
        current_authority_epoch: u64,
        current_source_cursor: u64,
        now_unix_ms: u64,
    ) -> Result<NotificationActionAdmission, NotificationActionGateError> {
        command
            .validate()
            .map_err(NotificationActionGateError::Invalid)?;
        authoritative_ref
            .validate()
            .map_err(NotificationActionGateError::Invalid)?;
        if command.action_ref.action_ref_id != authoritative_ref.action_ref_id
            || command.action_ref.action_digest != authoritative_ref.action_digest
        {
            return Err(NotificationActionGateError::RefNotAuthoritative);
        }
        if command.submitted_by != server_recipient_id
            || command.recipient_id != server_recipient_id
        {
            return Err(NotificationActionGateError::RecipientMismatch);
        }
        if command.expected_revision != current_revision
            || command.action_ref.target_revision != current_revision
        {
            return Err(NotificationActionGateError::RevisionConflict);
        }
        if command.expected_target_digest != current_target_digest {
            return Err(NotificationActionGateError::DigestConflict);
        }
        if command.expected_authority_epoch != current_authority_epoch {
            return Err(NotificationActionGateError::AuthorityConflict);
        }
        if command.expected_source_cursor != current_source_cursor {
            return Err(NotificationActionGateError::CursorConflict);
        }
        if now_unix_ms < authoritative_ref.created_at_unix_ms
            || now_unix_ms >= authoritative_ref.expires_at_unix_ms
        {
            return Err(NotificationActionGateError::Expired);
        }
        Ok(NotificationActionAdmission {
            schema: NOTIFICATION_ACTION_GATE_SCHEMA.to_owned(),
            action: command.action,
            action_ref_id: authoritative_ref.action_ref_id,
            command_digest: command.command_digest.clone(),
            control_plane_required: true,
            direct_effect: false,
        })
    }
}
