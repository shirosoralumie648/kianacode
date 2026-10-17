//! ControlPlane re-admission and single-consume ledger for notification action commands.
//!
//! The gate validates a notification action against server-owned ref/target material and returns
//! a typed admission only. The ledger is the ControlPlane-side authority that consumes one
//! admission exactly once, records an `applied`/`failed`/`unknown` decision and wakes at most one
//! registered wait key. Neither type mutates HumanTask/Approval, appends an event, consumes a
//! permit, invokes a capability or executes the action.

use kiana_domain::{
    ActionRef, ActionRefId, NotificationActionCommand, NotificationActionDecision,
    NotificationActionKind, NotificationActionOutcome, NotificationActionWaitKey, NotificationId,
};
use std::collections::BTreeMap;
use thiserror::Error;

pub const NOTIFICATION_ACTION_GATE_SCHEMA: &str = "kiana.notification-action-gate.v1";

fn has_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Server-authoritative state of the original target the action points at.
///
/// `criteria_digest` is the acceptance/criteria revision, when the target has one. The gate
/// compares it exactly: a client may not omit or keep a stale criteria revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationActionTarget {
    pub revision: u64,
    pub target_digest: String,
    pub criteria_digest: Option<String>,
    pub authority_epoch: u64,
    pub source_cursor: u64,
}

impl NotificationActionTarget {
    pub fn new(
        revision: u64,
        target_digest: impl Into<String>,
        criteria_digest: Option<String>,
        authority_epoch: u64,
        source_cursor: u64,
    ) -> Result<Self, String> {
        let target = Self {
            revision,
            target_digest: target_digest.into(),
            criteria_digest,
            authority_epoch,
            source_cursor,
        };
        target.validate()?;
        Ok(target)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.revision == 0 || self.authority_epoch == 0 || self.source_cursor == 0 {
            return Err("notification_action_target_header_invalid".to_owned());
        }
        if !has_digest(&self.target_digest) {
            return Err("notification_action_target_digest_invalid".to_owned());
        }
        if let Some(criteria) = self.criteria_digest.as_deref() {
            if !has_digest(criteria) {
                return Err("notification_action_target_criteria_digest_invalid".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationActionAdmission {
    pub schema: String,
    pub action: NotificationActionKind,
    pub action_ref_id: ActionRefId,
    pub command_digest: String,
    pub control_plane_required: bool,
    pub direct_effect: bool,
}

impl NotificationActionAdmission {
    /// The admission is a routing receipt only: it must always require ControlPlane handling and
    /// must never claim a direct effect.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_ACTION_GATE_SCHEMA
            || self.action_ref_id.as_uuid().is_nil()
            || !self.control_plane_required
            || self.direct_effect
            || !has_digest(&self.command_digest)
        {
            return Err("notification_action_admission_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationActionGateError {
    #[error("notification_action_invalid:{0}")]
    Invalid(String),
    #[error("notification_action_unknown")]
    UnknownAction,
    #[error("notification_action_ref_not_authoritative")]
    RefNotAuthoritative,
    #[error("notification_action_recipient_mismatch")]
    RecipientMismatch,
    #[error("notification_action_revision_conflict")]
    RevisionConflict,
    #[error("notification_action_digest_conflict")]
    DigestConflict,
    #[error("notification_action_criteria_conflict")]
    CriteriaConflict,
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
        target: &NotificationActionTarget,
        now_unix_ms: u64,
    ) -> Result<NotificationActionAdmission, NotificationActionGateError> {
        command
            .validate()
            .map_err(NotificationActionGateError::Invalid)?;
        authoritative_ref
            .validate()
            .map_err(NotificationActionGateError::Invalid)?;
        target
            .validate()
            .map_err(NotificationActionGateError::Invalid)?;
        // The server-owned ref is the action registry: an unknown command name can never be
        // routed, even if the candidate carries a structurally valid kind.
        if NotificationActionKind::from_command(&authoritative_ref.command) != Some(command.action)
        {
            return Err(NotificationActionGateError::UnknownAction);
        }
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
        if command.expected_revision != target.revision
            || command.action_ref.target_revision != target.revision
        {
            return Err(NotificationActionGateError::RevisionConflict);
        }
        if command.expected_target_digest != target.target_digest {
            return Err(NotificationActionGateError::DigestConflict);
        }
        if command.expected_criteria_digest != target.criteria_digest {
            return Err(NotificationActionGateError::CriteriaConflict);
        }
        if command.expected_authority_epoch != target.authority_epoch {
            return Err(NotificationActionGateError::AuthorityConflict);
        }
        if command.expected_source_cursor != target.source_cursor {
            return Err(NotificationActionGateError::CursorConflict);
        }
        if now_unix_ms < authoritative_ref.created_at_unix_ms
            || now_unix_ms >= authoritative_ref.expires_at_unix_ms
        {
            return Err(NotificationActionGateError::Expired);
        }
        let admission = NotificationActionAdmission {
            schema: NOTIFICATION_ACTION_GATE_SCHEMA.to_owned(),
            action: command.action,
            action_ref_id: authoritative_ref.action_ref_id,
            command_digest: command.command_digest.clone(),
            control_plane_required: true,
            direct_effect: false,
        };
        admission
            .validate()
            .map_err(NotificationActionGateError::Invalid)?;
        Ok(admission)
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum HumanTaskActionLedgerError {
    #[error("notification_action_ledger_invalid:{0}")]
    Invalid(String),
    #[error(transparent)]
    Gate(#[from] NotificationActionGateError),
    #[error("notification_action_admission_mismatch")]
    AdmissionMismatch,
    #[error("notification_action_already_consumed")]
    AlreadyConsumed,
    #[error("notification_action_idempotency_conflict")]
    IdempotencyConflict,
    #[error("notification_action_wait_key_conflict")]
    WaitKeyConflict,
    #[error("notification_action_wait_key_ambiguous")]
    WaitKeyAmbiguous,
    #[error("notification_action_wait_key_consumed")]
    WaitKeyConsumed,
    #[error("notification_action_wait_key_binding_mismatch")]
    WaitKeyBindingMismatch,
}

/// The single wait-key wake produced by one decision.
///
/// Only the first consumption of an action ref can produce a wake; replays return the original
/// decision with `wake = None`, so one decision wakes exactly one wait key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationActionWake {
    pub wait_key: String,
    pub action_ref_id: ActionRefId,
    pub notification_id: NotificationId,
    pub source_cursor: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotificationActionConsumption {
    pub decision: NotificationActionDecision,
    pub wake: Option<NotificationActionWake>,
    pub replayed: bool,
}

/// ControlPlane-side single-consume ledger for HumanTask actions.
///
/// The ledger owns append-only decisions keyed by `ActionRefId` plus the registered wait keys.
/// It stores no delivery/read state; a channel ACK or inbox read cannot be routed through it.
#[derive(Default)]
pub struct HumanTaskActionLedger {
    decisions: BTreeMap<ActionRefId, NotificationActionDecision>,
    wait_keys: BTreeMap<String, NotificationActionWaitKey>,
    action_ref_wait_keys: BTreeMap<ActionRefId, String>,
}

impl HumanTaskActionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the wait key of the activation that is waiting for this action.
    ///
    /// The binding must match the authoritative ref/target exactly; a second distinct wait key
    /// for the same action ref is ambiguous and refused.
    pub fn register_wait_key(
        &mut self,
        wait_key: NotificationActionWaitKey,
        authoritative_ref: &ActionRef,
        target: &NotificationActionTarget,
        now_unix_ms: u64,
    ) -> Result<(), HumanTaskActionLedgerError> {
        wait_key
            .validate()
            .map_err(HumanTaskActionLedgerError::Invalid)?;
        authoritative_ref
            .validate()
            .map_err(HumanTaskActionLedgerError::Invalid)?;
        target
            .validate()
            .map_err(HumanTaskActionLedgerError::Invalid)?;
        if wait_key.action_ref_id != authoritative_ref.action_ref_id
            || wait_key.target_revision != authoritative_ref.target_revision
            || wait_key.target_revision != target.revision
            || wait_key.target_digest != target.target_digest
            || wait_key.authority_epoch != target.authority_epoch
            || wait_key.source_cursor != target.source_cursor
            || !authoritative_ref
                .scope
                .iter()
                .any(|scope| scope == &format!("notification:{}", wait_key.notification_id))
        {
            return Err(HumanTaskActionLedgerError::WaitKeyBindingMismatch);
        }
        if now_unix_ms < wait_key.registered_at_unix_ms {
            return Err(HumanTaskActionLedgerError::Invalid(
                "notification_action_wait_key_clock_regression".to_owned(),
            ));
        }
        if now_unix_ms >= authoritative_ref.expires_at_unix_ms {
            return Err(HumanTaskActionLedgerError::Invalid(
                "notification_action_wait_key_expired".to_owned(),
            ));
        }
        if self.decisions.contains_key(&wait_key.action_ref_id) {
            return Err(HumanTaskActionLedgerError::WaitKeyConsumed);
        }
        if let Some(existing) = self.wait_keys.get(&wait_key.wait_key) {
            if existing == &wait_key {
                return Ok(());
            }
            return Err(HumanTaskActionLedgerError::WaitKeyConflict);
        }
        if let Some(existing) = self.action_ref_wait_keys.get(&wait_key.action_ref_id) {
            if existing == &wait_key.wait_key {
                return Ok(());
            }
            return Err(HumanTaskActionLedgerError::WaitKeyAmbiguous);
        }
        self.action_ref_wait_keys
            .insert(wait_key.action_ref_id, wait_key.wait_key.clone());
        self.wait_keys.insert(wait_key.wait_key.clone(), wait_key);
        Ok(())
    }

    /// Consume one admitted action command exactly once and record its outcome.
    ///
    /// The gate is re-run against the current target before anything is written (TOCTOU). A
    /// replay of the same command digest is idempotent and never wakes a second wait key; any
    /// other second command for the same action ref is a double consume and is refused.
    #[allow(clippy::too_many_arguments)]
    pub fn consume(
        &mut self,
        admission: &NotificationActionAdmission,
        command: &NotificationActionCommand,
        authoritative_ref: &ActionRef,
        server_recipient_id: &str,
        target: &NotificationActionTarget,
        outcome: NotificationActionOutcome,
        outcome_code: Option<String>,
        now_unix_ms: u64,
    ) -> Result<NotificationActionConsumption, HumanTaskActionLedgerError> {
        let current = NotificationActionGate::admit(
            command,
            authoritative_ref,
            server_recipient_id,
            target,
            now_unix_ms,
        )?;
        if admission != &current {
            return Err(HumanTaskActionLedgerError::AdmissionMismatch);
        }
        let action_ref_id = command.action_ref.action_ref_id;
        if let Some(existing) = self.decisions.get(&action_ref_id) {
            if existing.command_digest == command.command_digest
                && existing.idempotency_key == command.idempotency_key
            {
                return Ok(NotificationActionConsumption {
                    decision: existing.clone(),
                    wake: None,
                    replayed: true,
                });
            }
            if existing.idempotency_key == command.idempotency_key {
                return Err(HumanTaskActionLedgerError::IdempotencyConflict);
            }
            return Err(HumanTaskActionLedgerError::AlreadyConsumed);
        }
        let wait_key = self
            .action_ref_wait_keys
            .get(&action_ref_id)
            .and_then(|wait_key| self.wait_keys.get(wait_key))
            .cloned();
        if let Some(record) = &wait_key {
            if record.notification_id != command.notification_id
                || record.target_revision != target.revision
                || record.target_digest != target.target_digest
                || record.authority_epoch != target.authority_epoch
                || record.source_cursor != target.source_cursor
            {
                return Err(HumanTaskActionLedgerError::WaitKeyBindingMismatch);
            }
        }
        let decision = NotificationActionDecision::new(
            action_ref_id,
            command.notification_id,
            command.action,
            command.command_digest.clone(),
            command.idempotency_key.clone(),
            server_recipient_id.to_owned(),
            outcome,
            outcome_code,
            now_unix_ms,
            target.source_cursor,
            wait_key.as_ref().map(|record| record.wait_key.clone()),
        )
        .map_err(HumanTaskActionLedgerError::Invalid)?;
        let wake = decision
            .wait_key
            .as_ref()
            .map(|wait_key| NotificationActionWake {
                wait_key: wait_key.clone(),
                action_ref_id: decision.action_ref_id,
                notification_id: decision.notification_id,
                source_cursor: decision.source_cursor,
            });
        if let Some(record) = wait_key {
            self.action_ref_wait_keys.remove(&action_ref_id);
            self.wait_keys.remove(&record.wait_key);
        }
        self.decisions.insert(action_ref_id, decision.clone());
        Ok(NotificationActionConsumption {
            decision,
            wake,
            replayed: false,
        })
    }

    pub fn decision(&self, action_ref_id: ActionRefId) -> Option<&NotificationActionDecision> {
        self.decisions.get(&action_ref_id)
    }

    pub fn decision_count(&self) -> usize {
        self.decisions.len()
    }

    pub fn pending_wait_key(
        &self,
        action_ref_id: ActionRefId,
    ) -> Option<&NotificationActionWaitKey> {
        self.action_ref_wait_keys
            .get(&action_ref_id)
            .and_then(|wait_key| self.wait_keys.get(wait_key))
    }

    pub fn wait_key_count(&self) -> usize {
        self.wait_keys.len()
    }
}

impl super::ControlPlane {
    /// Route a notification action through the original ControlPlane admission surface.
    ///
    /// This method is pure and intentionally does not append, dispatch or mutate anything; a
    /// higher-level command transaction must consume the returned admission through the ledger.
    pub fn admit_notification_action(
        &self,
        command: &NotificationActionCommand,
        authoritative_ref: &ActionRef,
        server_recipient_id: &str,
        target: &NotificationActionTarget,
        now_unix_ms: u64,
    ) -> Result<NotificationActionAdmission, NotificationActionGateError> {
        NotificationActionGate::admit(
            command,
            authoritative_ref,
            server_recipient_id,
            target,
            now_unix_ms,
        )
    }
}
