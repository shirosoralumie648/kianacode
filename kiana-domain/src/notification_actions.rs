//! Typed notification action commands, decisions and wait keys.
//!
//! A notification action is a server-bound command candidate. It carries an opaque action ref and
//! exact target material, but it is not itself approval, HumanTask mutation or capability effect.
//! The ControlPlane must re-admit it against the authoritative action ref before any command runs.
//!
//! Two facts are deliberately kept apart:
//!
//! * `NotificationActionCommand` is the *candidate* a client submits.
//! * `NotificationActionDecision` is the append-only outcome (`applied`/`failed`/`unknown`) of the
//!   original ControlPlane command, bound to one action ref and at most one wait key.
//!
//! Neither type carries delivery/read state: a channel ACK or an inbox read can never be decoded
//! into a decision, and an acknowledgement action never gains decision authority.

use crate::{
    canonical_journal_bytes, json_digest, redact_value, ActionRef, ActionRefId,
    NotificationActionDecisionId, NotificationId, RequestId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const NOTIFICATION_ACTION_COMMAND_SCHEMA: &str = "kiana.notification-action-command.v1";
pub const NOTIFICATION_ACTION_DECISION_SCHEMA: &str = "kiana.notification-action-decision.v1";
pub const NOTIFICATION_ACTION_WAIT_KEY_SCHEMA: &str = "kiana.notification-action-wait-key.v1";
pub const NOTIFICATION_ACTION_MAX_PAYLOAD_BYTES: usize = 16 * 1024;
pub const NOTIFICATION_ACTION_MAX_TEXT_BYTES: usize = 256;
pub const NOTIFICATION_ACTION_MAX_OUTCOME_CODE_BYTES: usize = 128;
pub const NOTIFICATION_ACTION_MAX_WAIT_KEY_BYTES: usize = 256;

/// Payload keys that would smuggle an approval/decision/delivery claim past the action kind.
///
/// The action kind itself is the decision; a free-form payload may carry a reason or note but it
/// can never turn an acknowledgement into an approval or a delivery ACK into a task result.
const FORBIDDEN_DECISION_PAYLOAD_KEYS: [&str; 6] = [
    "acknowledged",
    "approval",
    "approved",
    "decision",
    "delivered",
    "task_status",
];

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

fn payload_carries_decision(payload: &Value) -> bool {
    payload.as_object().is_some_and(|object| {
        object
            .keys()
            .any(|key| FORBIDDEN_DECISION_PAYLOAD_KEYS.contains(&key.as_str()))
    })
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
    /// The closed registry of server-registered action commands,
    /// `NotificationActionKind::REGISTERED`.
    pub const REGISTERED: [Self; 8] = [
        Self::Acknowledge,
        Self::Approve,
        Self::Review,
        Self::Reconcile,
        Self::Snooze,
        Self::Escalate,
        Self::Delegate,
        Self::Withdraw,
    ];

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

    /// Resolve a registered command string; anything else is an unknown action.
    pub fn from_command(command: &str) -> Option<Self> {
        Self::REGISTERED
            .iter()
            .copied()
            .find(|kind| kind.command() == command)
    }

    /// Read/ACK/snooze only touch the inbox projection; they never decide the original task.
    pub fn is_presentation_only(self) -> bool {
        matches!(self, Self::Acknowledge | Self::Snooze)
    }

    /// What this action is allowed to change on the original authority object.
    pub fn authority_effect(self) -> NotificationActionAuthorityEffect {
        match self {
            Self::Acknowledge | Self::Snooze => NotificationActionAuthorityEffect::Presentation,
            Self::Escalate | Self::Delegate | Self::Withdraw => {
                NotificationActionAuthorityEffect::Governance
            }
            Self::Approve | Self::Review | Self::Reconcile => {
                NotificationActionAuthorityEffect::Decision
            }
        }
    }
}

/// The authority surface an action may touch.
///
/// It is derived from the action kind and re-checked on every decision: a presentation action can
/// never carry a `decision` authority effect, no matter what the client claimed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationActionAuthorityEffect {
    Presentation,
    Governance,
    Decision,
}

/// The result of submitting an action to the original ControlPlane authority.
///
/// `unknown` is not a synonym for failure or success: it means the effect could not be observed
/// and must stay separate from delivery ACK state and from a false `applied` claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationActionOutcome {
    Applied,
    Failed,
    Unknown,
}

impl NotificationActionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
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
    /// Digest of the criteria/acceptance revision, when the target has one.
    #[serde(default)]
    pub expected_criteria_digest: Option<String>,
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
            expected_criteria_digest: None,
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

    /// Bind the criteria/acceptance digest the server must still be serving.
    pub fn with_criteria_digest(
        mut self,
        criteria_digest: impl Into<String>,
    ) -> Result<Self, String> {
        self.expected_criteria_digest = Some(criteria_digest.into());
        self.command_digest = self.digest();
        self.validate()?;
        Ok(self)
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
        if let Some(criteria) = self.expected_criteria_digest.as_deref() {
            digest(criteria, "notification_action_criteria_digest")?;
        }
        let encoded = canonical_journal_bytes(&self.payload)
            .map_err(|_| "notification_action_payload_invalid".to_owned())?;
        if encoded.len() > NOTIFICATION_ACTION_MAX_PAYLOAD_BYTES
            || redact_value(&self.payload) != self.payload
        {
            return Err("notification_action_payload_invalid".to_owned());
        }
        if payload_carries_decision(&self.payload) {
            return Err("notification_action_payload_forged_decision".to_owned());
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
            "expected_criteria_digest": self.expected_criteria_digest,
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

/// The append-only result of submitting `NotificationActionCommand` to the original authority.
///
/// The decision carries no delivery, read or task-status field. A channel ACK, a browser read or
/// a toast display can never be decoded into this shape, and only `applied` means the original
/// command was accepted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationActionDecision {
    pub schema: String,
    pub decision_id: NotificationActionDecisionId,
    pub action_ref_id: ActionRefId,
    pub notification_id: NotificationId,
    pub action: NotificationActionKind,
    pub authority_effect: NotificationActionAuthorityEffect,
    pub command_digest: String,
    pub idempotency_key: String,
    pub decider_principal_id: String,
    pub outcome: NotificationActionOutcome,
    /// Required for `failed`/`unknown`, forbidden for `applied`.
    pub outcome_code: Option<String>,
    pub decided_at_unix_ms: u64,
    pub source_cursor: u64,
    /// The one wait key woken by this decision, if the original action was waiting.
    pub wait_key: Option<String>,
    pub decision_digest: String,
}

impl NotificationActionDecision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        action_ref_id: ActionRefId,
        notification_id: NotificationId,
        action: NotificationActionKind,
        command_digest: impl Into<String>,
        idempotency_key: impl Into<String>,
        decider_principal_id: impl Into<String>,
        outcome: NotificationActionOutcome,
        outcome_code: Option<String>,
        decided_at_unix_ms: u64,
        source_cursor: u64,
        wait_key: Option<String>,
    ) -> Result<Self, String> {
        let mut decision = Self {
            schema: NOTIFICATION_ACTION_DECISION_SCHEMA.to_owned(),
            decision_id: NotificationActionDecisionId::new(),
            action_ref_id,
            notification_id,
            action,
            authority_effect: action.authority_effect(),
            command_digest: command_digest.into(),
            idempotency_key: idempotency_key.into(),
            decider_principal_id: decider_principal_id.into(),
            outcome,
            outcome_code,
            decided_at_unix_ms,
            source_cursor,
            wait_key,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ACTION_DECISION_SCHEMA
            || self.decision_id.as_uuid().is_nil()
            || self.action_ref_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.decided_at_unix_ms == 0
            || self.source_cursor == 0
        {
            return Err("notification_action_decision_header_invalid".to_owned());
        }
        required(
            &self.decider_principal_id,
            "notification_action_decision_decider",
            NOTIFICATION_ACTION_MAX_TEXT_BYTES,
        )?;
        required(
            &self.idempotency_key,
            "notification_action_decision_idempotency",
            NOTIFICATION_ACTION_MAX_TEXT_BYTES,
        )?;
        digest(
            &self.command_digest,
            "notification_action_decision_command_digest",
        )?;
        if let Some(wait_key) = self.wait_key.as_deref() {
            required(
                wait_key,
                "notification_action_decision_wait_key",
                NOTIFICATION_ACTION_MAX_WAIT_KEY_BYTES,
            )?;
        }
        match self.outcome {
            NotificationActionOutcome::Applied => {
                if self.outcome_code.is_some() {
                    return Err("notification_action_decision_applied_with_code".to_owned());
                }
            }
            NotificationActionOutcome::Failed | NotificationActionOutcome::Unknown => {
                let Some(code) = self.outcome_code.as_deref() else {
                    return Err("notification_action_decision_code_required".to_owned());
                };
                required(
                    code,
                    "notification_action_decision_code",
                    NOTIFICATION_ACTION_MAX_OUTCOME_CODE_BYTES,
                )?;
            }
        }
        if self.authority_effect != self.action.authority_effect() {
            return Err("notification_action_decision_authority_effect_mismatch".to_owned());
        }
        digest(&self.decision_digest, "notification_action_decision_digest")?;
        if self.decision_digest != self.digest() {
            return Err("notification_action_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "decision_id": self.decision_id,
            "action_ref_id": self.action_ref_id,
            "notification_id": self.notification_id,
            "action": self.action,
            "authority_effect": self.authority_effect,
            "command_digest": self.command_digest,
            "idempotency_key": self.idempotency_key,
            "decider_principal_id": self.decider_principal_id,
            "outcome": self.outcome,
            "outcome_code": self.outcome_code,
            "decided_at_unix_ms": self.decided_at_unix_ms,
            "source_cursor": self.source_cursor,
            "wait_key": self.wait_key,
        }))
    }
}

/// A durable wait key bound to one action ref and one exact target revision.
///
/// A decision may wake at most one registered wait key; registering a second key for the same
/// action ref is ambiguous and therefore refused by the ledger.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationActionWaitKey {
    pub schema: String,
    pub wait_key: String,
    pub action_ref_id: ActionRefId,
    pub notification_id: NotificationId,
    pub target_revision: u64,
    pub target_digest: String,
    pub authority_epoch: u64,
    pub source_cursor: u64,
    pub registered_at_unix_ms: u64,
    pub registration_digest: String,
}

impl NotificationActionWaitKey {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        wait_key: impl Into<String>,
        action_ref_id: ActionRefId,
        notification_id: NotificationId,
        target_revision: u64,
        target_digest: impl Into<String>,
        authority_epoch: u64,
        source_cursor: u64,
        registered_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut wait_key = Self {
            schema: NOTIFICATION_ACTION_WAIT_KEY_SCHEMA.to_owned(),
            wait_key: wait_key.into(),
            action_ref_id,
            notification_id,
            target_revision,
            target_digest: target_digest.into(),
            authority_epoch,
            source_cursor,
            registered_at_unix_ms,
            registration_digest: String::new(),
        };
        wait_key.registration_digest = wait_key.digest();
        wait_key.validate()?;
        Ok(wait_key)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ACTION_WAIT_KEY_SCHEMA
            || self.action_ref_id.as_uuid().is_nil()
            || self.notification_id.as_uuid().is_nil()
            || self.target_revision == 0
            || self.authority_epoch == 0
            || self.source_cursor == 0
            || self.registered_at_unix_ms == 0
        {
            return Err("notification_action_wait_key_header_invalid".to_owned());
        }
        required(
            &self.wait_key,
            "notification_action_wait_key",
            NOTIFICATION_ACTION_MAX_WAIT_KEY_BYTES,
        )?;
        digest(
            &self.target_digest,
            "notification_action_wait_key_target_digest",
        )?;
        digest(
            &self.registration_digest,
            "notification_action_wait_key_digest",
        )?;
        if self.registration_digest != self.digest() {
            return Err("notification_action_wait_key_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "wait_key": self.wait_key,
            "action_ref_id": self.action_ref_id,
            "notification_id": self.notification_id,
            "target_revision": self.target_revision,
            "target_digest": self.target_digest,
            "authority_epoch": self.authority_epoch,
            "source_cursor": self.source_cursor,
            "registered_at_unix_ms": self.registered_at_unix_ms,
        }))
    }
}
