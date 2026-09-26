//! INT-22 connector Unknown quarantine and explicit reconciliation evidence.
//!
//! An Unknown receipt is a durable quarantine input. Query or manual evidence may be attached
//! only when it matches the original invocation identity and payload. Reconciliation produces a
//! successor fact; it never edits the original Unknown receipt or grants an automatic retry.

use crate::{
    json_digest, provider_payload_hash_valid, EffectObservation, InvocationId, ProviderOutcome,
    ProviderReceipt,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_RECONCILIATION_SCHEMA: &str = "kiana.connector-reconciliation-case.v1";
pub const CONNECTOR_RECONCILIATION_EVIDENCE_SCHEMA: &str =
    "kiana.connector-reconciliation-evidence.v1";
pub const CONNECTOR_HUMAN_INBOX_SCHEMA: &str = "kiana.connector-human-inbox-item.v1";
const MAX_ACTIONS: usize = 8;
const MAX_EVIDENCE_REFS: usize = 32;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
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
pub enum ConnectorReconciliationReason {
    ProviderUnknown,
    TransportTimeout,
    TransportDisconnected,
    CommitUnknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReconciliationSource {
    ProviderQuery,
    ManualEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReconciliationState {
    Pending,
    EvidenceAttached,
    Reconciled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReconciliationAction {
    ProviderQuery,
    SubmitManualEvidence,
    HumanReview,
    AutomaticRetry,
    ReuseIdempotencyKey,
    ReplaceOriginalReceipt,
}

fn safe_actions() -> Vec<ConnectorReconciliationAction> {
    vec![
        ConnectorReconciliationAction::ProviderQuery,
        ConnectorReconciliationAction::SubmitManualEvidence,
        ConnectorReconciliationAction::HumanReview,
    ]
}

fn forbidden_actions() -> Vec<ConnectorReconciliationAction> {
    vec![
        ConnectorReconciliationAction::AutomaticRetry,
        ConnectorReconciliationAction::ReuseIdempotencyKey,
        ConnectorReconciliationAction::ReplaceOriginalReceipt,
    ]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorReconciliationEvidence {
    pub schema: String,
    pub source: ConnectorReconciliationSource,
    pub receipt: ProviderReceipt,
    pub observation: EffectObservation,
    pub evidence_ref_digests: Vec<String>,
    pub evidence_digest: String,
}

impl ConnectorReconciliationEvidence {
    pub fn new(
        source: ConnectorReconciliationSource,
        receipt: ProviderReceipt,
        observation: EffectObservation,
        evidence_ref_digests: Vec<String>,
    ) -> Result<Self, String> {
        receipt.validate()?;
        if receipt.outcome == ProviderOutcome::Unknown {
            return Err("connector_reconciliation_unknown_evidence_forbidden".to_owned());
        }
        observation.validate_for_receipt(
            &receipt,
            &observation.owner_digest,
            &observation.audience_digest,
        )?;
        let mut evidence = Self {
            schema: CONNECTOR_RECONCILIATION_EVIDENCE_SCHEMA.to_owned(),
            source,
            receipt,
            observation,
            evidence_ref_digests,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RECONCILIATION_EVIDENCE_SCHEMA
            || self.evidence_ref_digests.len() > MAX_EVIDENCE_REFS
            || self
                .evidence_ref_digests
                .iter()
                .any(|value| digest(value, "connector_reconciliation_evidence_ref").is_err())
            || !valid_digest(&self.evidence_digest)
        {
            return Err("connector_reconciliation_evidence_invalid".to_owned());
        }
        self.receipt.validate()?;
        if self.receipt.outcome == ProviderOutcome::Unknown {
            return Err("connector_reconciliation_unknown_evidence_forbidden".to_owned());
        }
        self.observation.validate_for_receipt(
            &self.receipt,
            &self.observation.owner_digest,
            &self.observation.audience_digest,
        )?;
        if self.evidence_digest != self.digest() {
            return Err("connector_reconciliation_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source": self.source,
            "receipt": self.receipt,
            "observation": self.observation,
            "evidence_ref_digests": self.evidence_ref_digests,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorReconciliationCase {
    pub schema: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub operation: String,
    pub idempotency_key_digest: String,
    pub payload_sha256: String,
    pub owner_digest: String,
    pub audience_digest: String,
    pub original_receipt_digest: String,
    pub reason: ConnectorReconciliationReason,
    pub state: ConnectorReconciliationState,
    pub reconciliation_required: bool,
    pub automatic_retry_allowed: bool,
    pub safe_actions: Vec<ConnectorReconciliationAction>,
    pub forbidden_actions: Vec<ConnectorReconciliationAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<ConnectorReconciliationEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_outcome: Option<ProviderOutcome>,
    pub case_digest: String,
}

impl ConnectorReconciliationCase {
    pub fn from_unknown(
        receipt: &ProviderReceipt,
        observation: &EffectObservation,
        reason: ConnectorReconciliationReason,
    ) -> Result<Self, String> {
        receipt.validate()?;
        if receipt.outcome != ProviderOutcome::Unknown {
            return Err("connector_reconciliation_unknown_receipt_required".to_owned());
        }
        observation.validate_for_receipt(
            receipt,
            &observation.owner_digest,
            &observation.audience_digest,
        )?;
        let mut case = Self {
            schema: CONNECTOR_RECONCILIATION_SCHEMA.to_owned(),
            invocation_id: observation.invocation_id,
            attempt: observation.attempt,
            connector_id: receipt.connector_id.clone(),
            binding_id: receipt.binding_id.clone(),
            account_id: receipt.account_id.clone(),
            operation: receipt.operation.clone(),
            idempotency_key_digest: observation.idempotency_key_digest.clone(),
            payload_sha256: receipt.final_payload_sha256.clone(),
            owner_digest: observation.owner_digest.clone(),
            audience_digest: observation.audience_digest.clone(),
            original_receipt_digest: json_digest(
                &serde_json::to_value(receipt)
                    .map_err(|_| "connector_reconciliation_receipt_encode_failed")?,
            ),
            reason,
            state: ConnectorReconciliationState::Pending,
            reconciliation_required: true,
            automatic_retry_allowed: false,
            safe_actions: safe_actions(),
            forbidden_actions: forbidden_actions(),
            evidence: None,
            observed_outcome: None,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn attach_evidence(
        &self,
        source: ConnectorReconciliationSource,
        receipt: ProviderReceipt,
        observation: EffectObservation,
        evidence_ref_digests: Vec<String>,
    ) -> Result<Self, String> {
        self.validate()?;
        if self.state != ConnectorReconciliationState::Pending {
            return Err("connector_reconciliation_case_not_pending".to_owned());
        }
        if receipt.connector_id != self.connector_id
            || receipt.binding_id != self.binding_id
            || receipt.account_id != self.account_id
            || receipt.operation != self.operation
            || receipt.final_payload_sha256 != self.payload_sha256
            || json_digest(&json!({"idempotency_key": receipt.idempotency_key}))
                != self.idempotency_key_digest
            || observation.invocation_id != self.invocation_id
            || observation.attempt != self.attempt
            || observation.owner_digest != self.owner_digest
            || observation.audience_digest != self.audience_digest
        {
            return Err("connector_reconciliation_evidence_binding_mismatch".to_owned());
        }
        let evidence = ConnectorReconciliationEvidence::new(
            source,
            receipt,
            observation,
            evidence_ref_digests,
        )?;
        let mut next = self.clone();
        next.state = ConnectorReconciliationState::EvidenceAttached;
        next.evidence = Some(evidence);
        next.observed_outcome = next.evidence.as_ref().map(|item| item.receipt.outcome);
        next.case_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn commit_reconciled(&self) -> Result<Self, String> {
        self.validate()?;
        if self.state != ConnectorReconciliationState::EvidenceAttached {
            return Err("connector_reconciliation_evidence_required".to_owned());
        }
        let mut next = self.clone();
        next.state = ConnectorReconciliationState::Reconciled;
        next.case_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn human_inbox_item(&self) -> Result<ConnectorHumanInboxItem, String> {
        self.validate()?;
        let mut item = ConnectorHumanInboxItem {
            schema: CONNECTOR_HUMAN_INBOX_SCHEMA.to_owned(),
            case_digest: self.case_digest.clone(),
            state: self.state,
            reason: self.reason,
            safe_actions: self.safe_actions.clone(),
            forbidden_actions: self.forbidden_actions.clone(),
            item_digest: String::new(),
        };
        item.item_digest = item.digest();
        item.validate()?;
        Ok(item)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_RECONCILIATION_SCHEMA
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !provider_payload_hash_valid(&self.payload_sha256)
            || !valid_digest(&self.idempotency_key_digest)
            || !valid_digest(&self.owner_digest)
            || !valid_digest(&self.audience_digest)
            || !valid_digest(&self.original_receipt_digest)
            || !valid_digest(&self.case_digest)
            || !self.reconciliation_required
            || self.automatic_retry_allowed
            || self.safe_actions.len() > MAX_ACTIONS
            || self.forbidden_actions.len() > MAX_ACTIONS
            || self.safe_actions != safe_actions()
            || self.forbidden_actions != forbidden_actions()
        {
            return Err("connector_reconciliation_case_invalid".to_owned());
        }
        for (value, field) in [
            (&self.connector_id, "connector_reconciliation_connector_id"),
            (&self.binding_id, "connector_reconciliation_binding_id"),
            (&self.account_id, "connector_reconciliation_account_id"),
            (&self.operation, "connector_reconciliation_operation"),
        ] {
            required(value, field, 256)?;
        }
        match self.state {
            ConnectorReconciliationState::Pending => {
                if self.evidence.is_some() || self.observed_outcome.is_some() {
                    return Err("connector_reconciliation_pending_evidence_invalid".to_owned());
                }
            }
            ConnectorReconciliationState::EvidenceAttached
            | ConnectorReconciliationState::Reconciled => {
                let evidence = self
                    .evidence
                    .as_ref()
                    .ok_or_else(|| "connector_reconciliation_evidence_missing".to_owned())?;
                evidence.validate()?;
                if evidence.receipt.connector_id != self.connector_id
                    || evidence.receipt.binding_id != self.binding_id
                    || evidence.receipt.account_id != self.account_id
                    || evidence.receipt.operation != self.operation
                    || evidence.receipt.final_payload_sha256 != self.payload_sha256
                    || json_digest(&json!({
                        "idempotency_key": evidence.receipt.idempotency_key
                    })) != self.idempotency_key_digest
                    || evidence.observation.invocation_id != self.invocation_id
                    || evidence.observation.attempt != self.attempt
                    || evidence.observation.owner_digest != self.owner_digest
                    || evidence.observation.audience_digest != self.audience_digest
                {
                    return Err("connector_reconciliation_evidence_binding_mismatch".to_owned());
                }
                if self.observed_outcome != Some(evidence.receipt.outcome)
                    || self.observed_outcome == Some(ProviderOutcome::Unknown)
                {
                    return Err("connector_reconciliation_outcome_invalid".to_owned());
                }
            }
        }
        if self.case_digest != self.digest() {
            return Err("connector_reconciliation_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "operation": self.operation,
            "idempotency_key_digest": self.idempotency_key_digest,
            "payload_sha256": self.payload_sha256,
            "owner_digest": self.owner_digest,
            "audience_digest": self.audience_digest,
            "original_receipt_digest": self.original_receipt_digest,
            "reason": self.reason,
            "state": self.state,
            "reconciliation_required": self.reconciliation_required,
            "automatic_retry_allowed": self.automatic_retry_allowed,
            "safe_actions": self.safe_actions,
            "forbidden_actions": self.forbidden_actions,
            "evidence": self.evidence,
            "observed_outcome": self.observed_outcome,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHumanInboxItem {
    pub schema: String,
    pub case_digest: String,
    pub state: ConnectorReconciliationState,
    pub reason: ConnectorReconciliationReason,
    pub safe_actions: Vec<ConnectorReconciliationAction>,
    pub forbidden_actions: Vec<ConnectorReconciliationAction>,
    pub item_digest: String,
}

impl ConnectorHumanInboxItem {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_HUMAN_INBOX_SCHEMA
            || !valid_digest(&self.case_digest)
            || self.safe_actions != safe_actions()
            || self.forbidden_actions != forbidden_actions()
            || !valid_digest(&self.item_digest)
            || self.item_digest != self.digest()
        {
            return Err("connector_human_inbox_item_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "case_digest": self.case_digest,
            "state": self.state,
            "reason": self.reason,
            "safe_actions": self.safe_actions,
            "forbidden_actions": self.forbidden_actions,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
