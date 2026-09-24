//! Runtime receipt and evidence references used by Company and Workflow projections.
//!
//! A runtime terminal is an observation of an execution request.  It is not a business
//! acceptance, delivery confirmation or closing decision.  These contracts keep that boundary
//! explicit while allowing the existing event and artifact references to remain replayable.

use crate::{json_digest, ExecutionStatus, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const RUNTIME_RECEIPT_REF_SCHEMA: &str = "kiana.runtime-receipt-ref.v1";
pub const RUNTIME_EVIDENCE_BUNDLE_SCHEMA: &str = "kiana.runtime-evidence-bundle.v1";
pub const WORKFLOW_INCIDENT_SCHEMA: &str = "kiana.workflow-incident.v1";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
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

pub fn validate_runtime_event_refs(refs: &[String]) -> Result<(), String> {
    if refs.is_empty() || refs.len() > 4_096 {
        return Err("runtime_event_refs_required".to_owned());
    }
    for reference in refs {
        if !reference.starts_with("event:")
            || reference.len() <= "event:".len()
            || reference.len() > 256
        {
            return Err("runtime_event_reference_invalid".to_owned());
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeReceiptRef {
    pub schema: String,
    pub request_id: RequestId,
    pub status: ExecutionStatus,
    pub event_refs: Vec<String>,
    pub receipt_digest: String,
}

impl RuntimeReceiptRef {
    pub fn new(
        request_id: RequestId,
        status: ExecutionStatus,
        event_refs: Vec<String>,
    ) -> Result<Self, String> {
        validate_runtime_event_refs(&event_refs)?;
        let mut receipt = Self {
            schema: RUNTIME_RECEIPT_REF_SCHEMA.to_owned(),
            request_id,
            status,
            event_refs,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RUNTIME_RECEIPT_REF_SCHEMA || self.request_id.as_uuid().is_nil() {
            return Err("runtime_receipt_ref_invalid".to_owned());
        }
        if !self.status.is_terminal() {
            return Err("runtime_receipt_terminal_status_required".to_owned());
        }
        validate_runtime_event_refs(&self.event_refs)?;
        digest(&self.receipt_digest, "runtime_receipt_digest")?;
        if self.receipt_digest != self.digest() {
            return Err("runtime_receipt_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "request_id": self.request_id,
            "status": self.status,
            "event_refs": self.event_refs,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeEvidenceBundle {
    pub schema: String,
    pub receipt: RuntimeReceiptRef,
    pub artifact_refs: Vec<String>,
    pub bundle_digest: String,
}

impl RuntimeEvidenceBundle {
    pub fn new(receipt: RuntimeReceiptRef, artifact_refs: Vec<String>) -> Result<Self, String> {
        let mut bundle = Self {
            schema: RUNTIME_EVIDENCE_BUNDLE_SCHEMA.to_owned(),
            receipt,
            artifact_refs,
            bundle_digest: String::new(),
        };
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RUNTIME_EVIDENCE_BUNDLE_SCHEMA || self.artifact_refs.len() > 4_096 {
            return Err("runtime_evidence_bundle_invalid".to_owned());
        }
        self.receipt.validate()?;
        for reference in &self.artifact_refs {
            if !reference.starts_with("artifact:")
                || reference.len() <= "artifact:".len()
                || reference.len() > 256
                || reference.contains('\0')
            {
                return Err("runtime_artifact_reference_invalid".to_owned());
            }
        }
        digest(&self.bundle_digest, "runtime_evidence_bundle_digest")?;
        if self.bundle_digest != self.digest() {
            return Err("runtime_evidence_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt": self.receipt,
            "artifact_refs": self.artifact_refs,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIncident {
    pub schema: String,
    pub incident_id: String,
    pub instance_id: String,
    pub node_id: String,
    pub request_id: RequestId,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub incident_digest: String,
}

impl WorkflowIncident {
    pub fn new(
        incident_id: impl Into<String>,
        instance_id: impl Into<String>,
        node_id: impl Into<String>,
        request_id: RequestId,
        reason: impl Into<String>,
        evidence_refs: Vec<String>,
    ) -> Result<Self, String> {
        let mut incident = Self {
            schema: WORKFLOW_INCIDENT_SCHEMA.to_owned(),
            incident_id: incident_id.into(),
            instance_id: instance_id.into(),
            node_id: node_id.into(),
            request_id,
            reason: reason.into(),
            evidence_refs,
            incident_digest: String::new(),
        };
        incident.incident_digest = incident.digest();
        incident.validate()?;
        Ok(incident)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKFLOW_INCIDENT_SCHEMA
            || self.request_id.as_uuid().is_nil()
            || self.evidence_refs.is_empty()
        {
            return Err("workflow_incident_invalid".to_owned());
        }
        required(&self.incident_id, "workflow_incident_id", 256)?;
        required(&self.instance_id, "workflow_incident_instance", 256)?;
        required(&self.node_id, "workflow_incident_node", 256)?;
        required(&self.reason, "workflow_incident_reason", 2_048)?;
        validate_runtime_event_refs(&self.evidence_refs)?;
        digest(&self.incident_digest, "workflow_incident_digest")?;
        if self.incident_digest != self.digest() {
            return Err("workflow_incident_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "incident_id": self.incident_id,
            "instance_id": self.instance_id,
            "node_id": self.node_id,
            "request_id": self.request_id,
            "reason": self.reason,
            "evidence_refs": self.evidence_refs,
        }))
    }
}
