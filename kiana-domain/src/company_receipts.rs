//! Stable Company command receipts and post-commit dispatch intents.
//!
//! A receipt describes a committed Company fact; it never claims that a later effect succeeded.
//! DispatchIntent is the explicit handoff to the existing runner/effect path and can remain
//! pending for reconciliation after a process crash.

use crate::{derived_request_id, json_digest, CompanyCommand, EventId, RequestContext, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const COMPANY_COMMAND_RECEIPT_SCHEMA: &str = "kiana.company-command-receipt.v1";
pub const COMPANY_DISPATCH_INTENT_SCHEMA: &str = "kiana.company-dispatch-intent.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyReceiptStatus {
    Committed,
    Replayed,
    ResultUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchIntentStatus {
    Prepared,
    Consumed,
    AwaitingReconciliation,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchIntent {
    pub schema: String,
    pub intent_id: RequestId,
    pub command_id: RequestId,
    pub kind: String,
    pub target_digest: String,
    pub scope_digest: String,
    pub status: DispatchIntentStatus,
    pub attempt: u32,
    pub created_at_unix_ms: u64,
}

impl DispatchIntent {
    pub fn new(
        command_id: RequestId,
        kind: impl Into<String>,
        target_digest: impl Into<String>,
        scope_digest: impl Into<String>,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let intent = Self {
            schema: COMPANY_DISPATCH_INTENT_SCHEMA.to_owned(),
            intent_id: derived_request_id("company.dispatch", &command_id.to_string()),
            command_id,
            kind: kind.into(),
            target_digest: target_digest.into(),
            scope_digest: scope_digest.into(),
            status: DispatchIntentStatus::Prepared,
            attempt: 0,
            created_at_unix_ms,
        };
        intent.validate()?;
        Ok(intent)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_DISPATCH_INTENT_SCHEMA
            || self.kind.trim().is_empty()
            || self.kind.len() > 128
            || self.attempt > 1_024
            || self.created_at_unix_ms == 0
        {
            return Err("company_dispatch_intent_invalid".to_owned());
        }
        valid_digest(&self.target_digest, "company_dispatch_target_digest")?;
        valid_digest(&self.scope_digest, "company_dispatch_scope_digest")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyCommandReceipt {
    pub schema: String,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub payload_digest: String,
    pub authority_digest: String,
    pub expected_revision: u64,
    pub committed_revision: Option<u64>,
    pub event_id: Option<EventId>,
    pub status: CompanyReceiptStatus,
    pub dispatch_intent: Option<DispatchIntent>,
}

impl CompanyCommandReceipt {
    pub fn command_id(aggregate_id: &str, idempotency_key: &str) -> RequestId {
        derived_request_id(
            "protected.command",
            &format!("company:{aggregate_id}:{idempotency_key}"),
        )
    }

    pub fn payload_digest(command: &CompanyCommand) -> String {
        json_digest(&json!(command))
    }

    pub fn authority_digest(context: &RequestContext) -> String {
        json_digest(&json!({
            "actor_id": context.actor_id,
            "session_id": context.session_id,
            "role_id": context.role_id,
            "department_id": context.department_id,
            "project_root": context.project_root,
        }))
    }

    pub fn new(
        aggregate_id: &str,
        idempotency_key: &str,
        command: &CompanyCommand,
        context: &RequestContext,
        expected_revision: u64,
        committed_revision: Option<u64>,
        event_id: Option<EventId>,
        status: CompanyReceiptStatus,
        dispatch_intent: Option<DispatchIntent>,
    ) -> Result<Self, String> {
        if aggregate_id.trim().is_empty()
            || idempotency_key.trim().is_empty()
            || idempotency_key.len() > 256
            || expected_revision > u64::MAX - 1
        {
            return Err("company_command_receipt_identity_invalid".to_owned());
        }
        let receipt = Self {
            schema: COMPANY_COMMAND_RECEIPT_SCHEMA.to_owned(),
            command_id: Self::command_id(aggregate_id, idempotency_key),
            idempotency_key: idempotency_key.to_owned(),
            payload_digest: Self::payload_digest(command),
            authority_digest: Self::authority_digest(context),
            expected_revision,
            committed_revision,
            event_id,
            status,
            dispatch_intent,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_COMMAND_RECEIPT_SCHEMA
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 256
            || self.expected_revision > u64::MAX - 1
        {
            return Err("company_command_receipt_invalid".to_owned());
        }
        valid_digest(&self.payload_digest, "company_command_payload_digest")?;
        valid_digest(&self.authority_digest, "company_command_authority_digest")?;
        if self
            .committed_revision
            .is_some_and(|revision| revision <= self.expected_revision)
        {
            return Err("company_command_receipt_revision_invalid".to_owned());
        }
        if self.status == CompanyReceiptStatus::Committed
            && (self.committed_revision.is_none() || self.event_id.is_none())
        {
            return Err("company_command_receipt_commit_missing".to_owned());
        }
        if let Some(intent) = &self.dispatch_intent {
            intent.validate()?;
            if intent.command_id != self.command_id {
                return Err("company_command_receipt_intent_mismatch".to_owned());
            }
        }
        Ok(())
    }
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
