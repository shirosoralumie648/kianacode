//! Server-owned UI action command and journal contracts.
//!
//! A UI action is an optimistic request only.  The command carries every value that can affect
//! authorization or replay (owner, scope, instance epoch, cursor, target revision, deadline and
//! payload digest).  [`UiActionJournal`] is a deterministic reducer used by the ControlPlane and
//! by storage adapters; it never performs an effect and it never treats a missing response as a
//! successful effect.

use crate::{json_digest, RequestId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const UI_ACTION_COMMAND_SCHEMA: &str = "kiana.ui-action-command.v1";
pub const UI_ACTION_RECORD_SCHEMA: &str = "kiana.ui-action-record.v1";
pub const UI_ACTION_JOURNAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const UI_ACTION_AGGREGATE_TYPE: &str = "ui_action";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
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

fn bare_digest(value: &str) -> String {
    value.strip_prefix("sha256:").unwrap_or(value).to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiActionState {
    Accepted,
    Applied,
    Rejected,
    Unknown,
}

impl UiActionState {
    pub const fn terminal(self) -> bool {
        matches!(self, Self::Applied | Self::Rejected | Self::Unknown)
    }
}

/// Immutable action identity supplied by a surface adapter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionCommand {
    pub schema: String,
    pub version: SchemaVersion,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub target_id: String,
    pub owner_id: String,
    pub scope_digest: String,
    pub expected_epoch: String,
    pub expected_cursor: u64,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub deadline_unix_ms: u64,
    pub payload: Value,
    pub payload_digest: String,
    pub command_digest: String,
}

impl UiActionCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command_id: RequestId,
        idempotency_key: impl Into<String>,
        target_id: impl Into<String>,
        owner_id: impl Into<String>,
        scope_digest: impl Into<String>,
        expected_epoch: impl Into<String>,
        expected_cursor: u64,
        expected_revision: Option<u64>,
        deadline_unix_ms: u64,
        payload: Value,
    ) -> Result<Self, String> {
        let payload_digest = json_digest(&payload);
        let mut command = Self {
            schema: UI_ACTION_COMMAND_SCHEMA.to_owned(),
            version: UI_ACTION_JOURNAL_VERSION,
            command_id,
            idempotency_key: idempotency_key.into(),
            target_id: target_id.into(),
            owner_id: owner_id.into(),
            scope_digest: scope_digest.into(),
            expected_epoch: expected_epoch.into(),
            expected_cursor,
            expected_revision,
            deadline_unix_ms,
            payload,
            payload_digest,
            command_digest: String::new(),
        };
        command.command_digest = command.digest();
        command.validate()?;
        Ok(command)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_COMMAND_SCHEMA
            || !self
                .version
                .is_compatible_with(&UI_ACTION_JOURNAL_VERSION)
            || self.command_id.as_uuid().is_nil()
            || self.expected_revision == Some(0)
            || self.deadline_unix_ms == 0
        {
            return Err("ui_action_command_header_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_action_idempotency_key", 256)?;
        required(&self.target_id, "ui_action_target", 512)?;
        required(&self.owner_id, "ui_action_owner", 256)?;
        required(&self.expected_epoch, "ui_action_epoch", 256)?;
        digest(&self.scope_digest, "ui_action_scope_digest")?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        digest(&self.command_digest, "ui_action_command_digest")?;
        if self.payload_digest != json_digest(&self.payload) {
            return Err("ui_action_payload_digest_mismatch".to_owned());
        }
        if self.command_digest != self.digest() {
            return Err("ui_action_command_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Digest only immutable command identity.  Runtime state and server receipts are excluded.
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "command_id": self.command_id,
            "idempotency_key": self.idempotency_key,
            "target_id": self.target_id,
            "owner_id": self.owner_id,
            "scope_digest": self.scope_digest,
            "expected_epoch": self.expected_epoch,
            "expected_cursor": self.expected_cursor,
            "expected_revision": self.expected_revision,
            "deadline_unix_ms": self.deadline_unix_ms,
            "payload_digest": self.payload_digest,
        }))
    }

    pub fn journal_digest(&self) -> String {
        bare_digest(&self.command_digest)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiActionRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub command_digest: String,
    pub payload_digest: String,
    pub target_id: String,
    pub owner_id: String,
    pub scope_digest: String,
    pub state: UiActionState,
    pub accepted_at_unix_ms: u64,
    #[serde(default)]
    pub applied_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
    pub effect_count: u32,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    pub record_digest: String,
}

impl UiActionRecord {
    fn from_command(command: &UiActionCommand, now_unix_ms: u64) -> Self {
        let mut record = Self {
            schema: UI_ACTION_RECORD_SCHEMA.to_owned(),
            version: UI_ACTION_JOURNAL_VERSION,
            command_id: command.command_id,
            idempotency_key: command.idempotency_key.clone(),
            command_digest: command.command_digest.clone(),
            payload_digest: command.payload_digest.clone(),
            target_id: command.target_id.clone(),
            owner_id: command.owner_id.clone(),
            scope_digest: command.scope_digest.clone(),
            state: UiActionState::Accepted,
            accepted_at_unix_ms: now_unix_ms,
            applied_at_unix_ms: None,
            reason: None,
            effect_count: 0,
            receipt_digest: None,
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != UI_ACTION_RECORD_SCHEMA
            || !self
                .version
                .is_compatible_with(&UI_ACTION_JOURNAL_VERSION)
            || self.command_id.as_uuid().is_nil()
            || self.accepted_at_unix_ms == 0
            || self.effect_count > 1
        {
            return Err("ui_action_record_header_invalid".to_owned());
        }
        required(&self.idempotency_key, "ui_action_idempotency_key", 256)?;
        digest(&self.command_digest, "ui_action_command_digest")?;
        digest(&self.payload_digest, "ui_action_payload_digest")?;
        digest(&self.scope_digest, "ui_action_scope_digest")?;
        digest(&self.record_digest, "ui_action_record_digest")?;
        if self.state == UiActionState::Applied
            && (self.effect_count != 1 || self.applied_at_unix_ms.is_none())
        {
            return Err("ui_action_applied_effect_missing".to_owned());
        }
        if self.state != UiActionState::Applied
            && (self.effect_count != 0 || self.applied_at_unix_ms.is_some())
        {
            return Err("ui_action_effect_state_mismatch".to_owned());
        }
        if self.state.terminal() && self.reason.is_none() && self.state != UiActionState::Applied {
            return Err("ui_action_terminal_reason_missing".to_owned());
        }
        if self.record_digest != self.digest() {
            return Err("ui_action_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "command_id": self.command_id,
            "idempotency_key": self.idempotency_key,
            "command_digest": self.command_digest,
            "payload_digest": self.payload_digest,
            "target_id": self.target_id,
            "owner_id": self.owner_id,
            "scope_digest": self.scope_digest,
            "state": self.state,
            "accepted_at_unix_ms": self.accepted_at_unix_ms,
            "applied_at_unix_ms": self.applied_at_unix_ms,
            "reason": self.reason,
            "effect_count": self.effect_count,
            "receipt_digest": self.receipt_digest,
        }))
    }

    pub fn validate_for_query(&self) -> Result<(), String> {
        self.validate()
    }

    pub fn apply_transition(
        &self,
        receipt_digest: impl Into<String>,
        now_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut journal = UiActionJournal::from_record(self.clone())?;
        journal
            .apply(&self.idempotency_key, receipt_digest, now_unix_ms)
    }

    pub fn reject_transition(&self, reason: impl Into<String>) -> Result<Self, String> {
        let mut journal = UiActionJournal::from_record(self.clone())?;
        journal.reject(&self.idempotency_key, reason)
    }

    pub fn unknown_transition(&self, reason: impl Into<String>) -> Result<Self, String> {
        let mut journal = UiActionJournal::from_record(self.clone())?;
        journal.unknown(&self.idempotency_key, reason)
    }
}

#[derive(Clone, Debug, Default)]
pub struct UiActionJournal {
    records: BTreeMap<String, UiActionRecord>,
}

impl UiActionJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_record(record: UiActionRecord) -> Result<Self, String> {
        record.validate()?;
        let mut journal = Self::new();
        journal
            .records
            .insert(record.idempotency_key.clone(), record);
        Ok(journal)
    }

    /// Admit once. Replaying the exact command returns the original record and never increments
    /// `effect_count`; changing payload, owner, scope or command digest under one key fails closed.
    pub fn accept(
        &mut self,
        command: &UiActionCommand,
        authority_epoch: &str,
        current_cursor: u64,
        current_revision: Option<u64>,
        owner_id: &str,
        scope_digest: &str,
        now_unix_ms: u64,
    ) -> Result<(UiActionRecord, bool), String> {
        command.validate()?;
        if let Some(original) = self.records.get(&command.idempotency_key) {
            if original.command_digest != command.command_digest {
                return Err("ui_action_idempotency_digest_mismatch".to_owned());
            }
            if original.owner_id != command.owner_id || original.owner_id != owner_id {
                return Err("ui_action_owner_mismatch".to_owned());
            }
            if original.scope_digest != command.scope_digest || original.scope_digest != scope_digest
            {
                return Err("ui_action_scope_mismatch".to_owned());
            }
            return Ok((original.clone(), true));
        }
        if now_unix_ms > command.deadline_unix_ms {
            return Err("ui_action_deadline_expired".to_owned());
        }
        if command.expected_epoch != authority_epoch {
            return Err("ui_action_stale_epoch".to_owned());
        }
        if command.expected_cursor != current_cursor {
            return Err("ui_action_stale_cursor".to_owned());
        }
        if command.expected_revision != current_revision {
            return Err("ui_action_stale_revision".to_owned());
        }
        if command.owner_id != owner_id {
            return Err("ui_action_owner_mismatch".to_owned());
        }
        if command.scope_digest != scope_digest {
            return Err("ui_action_scope_mismatch".to_owned());
        }
        let record = UiActionRecord::from_command(command, now_unix_ms);
        self.records
            .insert(command.idempotency_key.clone(), record.clone());
        Ok((record, false))
    }

    pub fn apply(
        &mut self,
        idempotency_key: &str,
        receipt_digest: impl Into<String>,
        now_unix_ms: u64,
    ) -> Result<UiActionRecord, String> {
        let record = self
            .records
            .get_mut(idempotency_key)
            .ok_or_else(|| "ui_action_not_accepted".to_owned())?;
        record.validate()?;
        match record.state {
            UiActionState::Applied => return Ok(record.clone()),
            UiActionState::Rejected => return Err("ui_action_rejected".to_owned()),
            UiActionState::Unknown => return Err("ui_action_result_unknown".to_owned()),
            UiActionState::Accepted => {}
        }
        if now_unix_ms < record.accepted_at_unix_ms {
            return Err("ui_action_clock_regressed".to_owned());
        }
        let receipt_digest = receipt_digest.into();
        digest(&receipt_digest, "ui_action_receipt_digest")?;
        record.state = UiActionState::Applied;
        record.applied_at_unix_ms = Some(now_unix_ms);
        record.effect_count = 1;
        record.receipt_digest = Some(receipt_digest);
        record.reason = None;
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record.clone())
    }

    pub fn reject(
        &mut self,
        idempotency_key: &str,
        reason: impl Into<String>,
    ) -> Result<UiActionRecord, String> {
        self.finish(idempotency_key, UiActionState::Rejected, reason.into())
    }

    pub fn unknown(
        &mut self,
        idempotency_key: &str,
        reason: impl Into<String>,
    ) -> Result<UiActionRecord, String> {
        self.finish(idempotency_key, UiActionState::Unknown, reason.into())
    }

    fn finish(
        &mut self,
        idempotency_key: &str,
        state: UiActionState,
        reason: String,
    ) -> Result<UiActionRecord, String> {
        if reason.trim().is_empty() || reason.len() > 512 {
            return Err("ui_action_reason_invalid".to_owned());
        }
        let record = self
            .records
            .get_mut(idempotency_key)
            .ok_or_else(|| "ui_action_not_accepted".to_owned())?;
        record.validate()?;
        match record.state {
            UiActionState::Applied => return Ok(record.clone()),
            UiActionState::Rejected | UiActionState::Unknown if record.state != state => {
                return Err("ui_action_terminal_conflict".to_owned())
            }
            _ => {}
        }
        record.state = state;
        record.reason = Some(reason);
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record.clone())
    }

    pub fn query_original(&self, idempotency_key: &str) -> Option<&UiActionRecord> {
        self.records.get(idempotency_key)
    }

    pub fn query_command(&self, command_id: RequestId) -> Option<&UiActionRecord> {
        self.records.values().find(|record| record.command_id == command_id)
    }

    pub fn records(&self) -> impl Iterator<Item = &UiActionRecord> {
        self.records.values()
    }
}
