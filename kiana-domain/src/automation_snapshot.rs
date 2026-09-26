//! AUT-22 read-only automation snapshot for scheduler/workflow/trigger/Receipt/incident UI.
//!
//! The snapshot is a projection of committed facts. Querying it cannot claim a queue lease,
//! consume approval, dispatch a capability or mutate an incident.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const AUT22_SNAPSHOT_SCHEMA: &str = "kiana.aut22-automation-snapshot.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}
fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWorkStatus {
    Due,
    Blocked,
    Running,
    Unknown,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerSource {
    Manual,
    Event,
    Interval,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationWorkflowView {
    pub workflow_id: String,
    pub status: AutomationWorkStatus,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub due_at_unix_ms: Option<u64>,
    pub digest: String,
}

impl AutomationWorkflowView {
    fn validate(&self) -> Result<(), String> {
        required(&self.workflow_id, "aut22_workflow_id_required")?;
        required(&self.reason, "aut22_workflow_reason_required")?;
        if self.evidence_refs.len() > 64 {
            return Err("aut22_workflow_evidence_limit".to_owned());
        }
        for reference in &self.evidence_refs {
            required(reference, "aut22_workflow_evidence_invalid")?;
        }
        if self.status == AutomationWorkStatus::Due && self.due_at_unix_ms.is_none() {
            return Err("aut22_due_workflow_deadline_missing".to_owned());
        }
        if self.status == AutomationWorkStatus::Unknown && self.evidence_refs.is_empty() {
            return Err("aut22_unknown_workflow_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut22_workflow_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_workflow_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "workflow_id": self.workflow_id,
            "status": self.status,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
            "due_at_unix_ms": self.due_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationTriggerView {
    pub trigger_id: String,
    pub workflow_id: String,
    pub source: AutomationTriggerSource,
    pub occurrence_key: String,
    pub status: AutomationWorkStatus,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl AutomationTriggerView {
    fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.trigger_id, "aut22_trigger_id_required"),
            (&self.workflow_id, "aut22_trigger_workflow_required"),
            (&self.occurrence_key, "aut22_trigger_occurrence_required"),
            (&self.reason, "aut22_trigger_reason_required"),
        ] {
            required(value, field)?;
        }
        if self.status == AutomationWorkStatus::Unknown && self.evidence_refs.is_empty() {
            return Err("aut22_unknown_trigger_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut22_trigger_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_trigger_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "trigger_id": self.trigger_id,
            "workflow_id": self.workflow_id,
            "source": self.source,
            "occurrence_key": self.occurrence_key,
            "status": self.status,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationReceiptView {
    pub receipt_id: String,
    pub workflow_id: String,
    pub runtime_status: AutomationWorkStatus,
    pub business_outcome_confirmed: bool,
    pub runtime_evidence_refs: Vec<String>,
    pub business_evidence_refs: Vec<String>,
    pub limitations: Vec<String>,
    pub digest: String,
}

impl AutomationReceiptView {
    fn validate(&self) -> Result<(), String> {
        required(&self.receipt_id, "aut22_receipt_id_required")?;
        required(&self.workflow_id, "aut22_receipt_workflow_required")?;
        if self.runtime_evidence_refs.is_empty() || self.limitations.is_empty() {
            return Err("aut22_receipt_evidence_or_limitations_missing".to_owned());
        }
        if self.business_outcome_confirmed && self.business_evidence_refs.is_empty() {
            return Err("aut22_business_outcome_evidence_missing".to_owned());
        }
        digest(&self.digest, "aut22_receipt_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "receipt_id": self.receipt_id,
            "workflow_id": self.workflow_id,
            "runtime_status": self.runtime_status,
            "business_outcome_confirmed": self.business_outcome_confirmed,
            "runtime_evidence_refs": self.runtime_evidence_refs,
            "business_evidence_refs": self.business_evidence_refs,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationIncidentView {
    pub incident_id: String,
    pub workflow_id: String,
    pub unknown: bool,
    pub reconcile_required: bool,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl AutomationIncidentView {
    fn validate(&self) -> Result<(), String> {
        required(&self.incident_id, "aut22_incident_id_required")?;
        required(&self.workflow_id, "aut22_incident_workflow_required")?;
        required(&self.reason, "aut22_incident_reason_required")?;
        if self.unknown && (!self.reconcile_required || self.evidence_refs.is_empty()) {
            return Err("aut22_unknown_incident_reconcile_missing".to_owned());
        }
        digest(&self.digest, "aut22_incident_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_incident_digest_mismatch".to_owned());
        }
        Ok(())
    }
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "incident_id": self.incident_id,
            "workflow_id": self.workflow_id,
            "unknown": self.unknown,
            "reconcile_required": self.reconcile_required,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSnapshot {
    pub schema: String,
    pub snapshot_id: String,
    pub source_cursor: u64,
    pub projection_version: u64,
    pub authority_epoch: u64,
    pub workflows: Vec<AutomationWorkflowView>,
    pub triggers: Vec<AutomationTriggerView>,
    pub receipts: Vec<AutomationReceiptView>,
    pub incidents: Vec<AutomationIncidentView>,
    pub limitations: Vec<String>,
    pub digest: String,
}

impl AutomationSnapshot {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUT22_SNAPSHOT_SCHEMA
            || self.source_cursor == 0
            || self.projection_version == 0
            || self.authority_epoch == 0
            || self.limitations.is_empty()
        {
            return Err("aut22_snapshot_header_invalid".to_owned());
        }
        required(&self.snapshot_id, "aut22_snapshot_id_required")?;
        let mut workflows = BTreeSet::new();
        for workflow in &self.workflows {
            workflow.validate()?;
            if !workflows.insert(workflow.workflow_id.clone()) {
                return Err("aut22_workflow_duplicate".to_owned());
            }
        }
        let mut triggers = BTreeSet::new();
        for trigger in &self.triggers {
            trigger.validate()?;
            if !workflows.contains(&trigger.workflow_id)
                || !triggers.insert(trigger.trigger_id.clone())
            {
                return Err("aut22_trigger_binding_invalid".to_owned());
            }
        }
        let mut receipts = BTreeSet::new();
        for receipt in &self.receipts {
            receipt.validate()?;
            if !workflows.contains(&receipt.workflow_id)
                || !receipts.insert(receipt.receipt_id.clone())
            {
                return Err("aut22_receipt_binding_invalid".to_owned());
            }
        }
        for incident in &self.incidents {
            incident.validate()?;
            if !workflows.contains(&incident.workflow_id) {
                return Err("aut22_incident_binding_invalid".to_owned());
            }
        }
        digest(&self.digest, "aut22_snapshot_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("aut22_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "snapshot_id": self.snapshot_id,
            "source_cursor": self.source_cursor,
            "projection_version": self.projection_version,
            "authority_epoch": self.authority_epoch,
            "workflows": self.workflows,
            "triggers": self.triggers,
            "receipts": self.receipts,
            "incidents": self.incidents,
            "limitations": self.limitations,
        }))
    }
}
