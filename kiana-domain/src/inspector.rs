//! Redacted Context/Memory Inspector projection and governed user correction.
//!
//! Inspector data is a read-only manifest.  It intentionally carries digests and bounded status
//! fields instead of source locators or body text.  Corrections are ordinary MemoryMutation
//! intents and must return through the existing approval/event/mutation path.

use crate::{
    json_digest, EvidenceStatus, Freshness, MemoryMutation, MemoryMutationOperation,
    ProjectionLagView, RetrievalReceipt, RetrievalReceiptStage, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const INSPECTOR_SNAPSHOT_SCHEMA: &str = "kiana.context-memory-inspector.v1";
pub const USER_CORRECTION_SCHEMA: &str = "kiana.user-memory-correction.v1";
pub const INSPECTOR_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_INSPECTOR_SOURCES: usize = 512;
const MAX_INSPECTOR_OMISSIONS: usize = 512;

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectorSourceEntry {
    pub source_id: String,
    pub source_revision: String,
    pub stage: RetrievalReceiptStage,
    pub freshness: Freshness,
    pub evidence: EvidenceStatus,
    pub source_digest: String,
    pub locator_digest: String,
}

impl InspectorSourceEntry {
    fn validate(&self) -> Result<(), String> {
        required(&self.source_id, "inspector_source_id", 512)?;
        required(&self.source_revision, "inspector_source_revision", 256)?;
        digest(&self.source_digest, "inspector_source_digest")?;
        digest(&self.locator_digest, "inspector_locator_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectorOmission {
    pub source_id: String,
    pub reason: String,
}

impl InspectorOmission {
    fn validate(&self) -> Result<(), String> {
        required(&self.source_id, "inspector_omission_source_id", 512)?;
        if !matches!(
            self.reason.as_str(),
            "token_budget" | "task_lens" | "unavailable" | "denied" | "stale"
        ) {
            return Err("inspector_omission_reason_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMemoryInspectorSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub inspector_id: String,
    pub receipt_digest: String,
    pub context_plan_digest: String,
    pub projection: ProjectionLagView,
    pub data_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_plan_digest: Option<String>,
    pub sources: Vec<InspectorSourceEntry>,
    pub omissions: Vec<InspectorOmission>,
    pub candidate_ids: Vec<String>,
    pub candidate_state_digests: Vec<String>,
    pub deletion_state: String,
    pub rebuild_state: String,
    pub snapshot_digest: String,
}

impl ContextMemoryInspectorSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inspector_id: impl Into<String>,
        receipt_digest: impl Into<String>,
        context_plan_digest: impl Into<String>,
        projection: ProjectionLagView,
        data_epoch: u64,
        invalidation_plan_digest: Option<String>,
        sources: Vec<InspectorSourceEntry>,
        omissions: Vec<InspectorOmission>,
        candidate_ids: Vec<String>,
        candidate_state_digests: Vec<String>,
        deletion_state: impl Into<String>,
        rebuild_state: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: INSPECTOR_SNAPSHOT_SCHEMA.to_owned(),
            version: INSPECTOR_VERSION,
            inspector_id: inspector_id.into(),
            receipt_digest: receipt_digest.into(),
            context_plan_digest: context_plan_digest.into(),
            projection,
            data_epoch,
            invalidation_plan_digest,
            sources,
            omissions,
            candidate_ids,
            candidate_state_digests,
            deletion_state: deletion_state.into(),
            rebuild_state: rebuild_state.into(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_receipt(
        inspector_id: impl Into<String>,
        receipt: &RetrievalReceipt,
        context_plan_digest: impl Into<String>,
        projection: ProjectionLagView,
        data_epoch: u64,
        invalidation_plan_digest: Option<String>,
        deletion_state: impl Into<String>,
        rebuild_state: impl Into<String>,
    ) -> Result<Self, String> {
        receipt.validate()?;
        let sources = receipt
            .entries
            .iter()
            .map(|entry| InspectorSourceEntry {
                source_id: entry.source_snapshot.source.source_id.clone(),
                source_revision: entry.source_revision.clone(),
                stage: entry.stage,
                freshness: entry.source_snapshot.freshness,
                evidence: entry.source_snapshot.evidence,
                source_digest: entry.source_snapshot.source.content_digest.clone(),
                locator_digest: json_digest(&json!({
                    "locator": entry.source_snapshot.source.locator
                })),
            })
            .collect();
        let omissions = receipt
            .omissions
            .iter()
            .map(|omission| InspectorOmission {
                source_id: omission.candidate_id.clone(),
                reason: omission.reason.clone(),
            })
            .collect();
        Self::new(
            inspector_id,
            receipt.receipt_digest.clone(),
            context_plan_digest,
            projection,
            data_epoch,
            invalidation_plan_digest,
            sources,
            omissions,
            Vec::new(),
            Vec::new(),
            deletion_state,
            rebuild_state,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != INSPECTOR_SNAPSHOT_SCHEMA
            || self.version != INSPECTOR_VERSION
            || self.sources.len() > MAX_INSPECTOR_SOURCES
            || self.omissions.len() > MAX_INSPECTOR_OMISSIONS
            || self.data_epoch == 0
            || !matches!(
                self.deletion_state.as_str(),
                "active" | "pending" | "preserved_invalid" | "unknown"
            )
            || !matches!(
                self.rebuild_state.as_str(),
                "ready" | "pending" | "rebuild_required" | "unknown"
            )
        {
            return Err("inspector_snapshot_header_invalid".to_owned());
        }
        required(&self.inspector_id, "inspector_id", 256)?;
        digest(&self.receipt_digest, "inspector_receipt_digest")?;
        digest(&self.context_plan_digest, "inspector_context_plan_digest")?;
        self.projection.validate()?;
        if let Some(invalidation) = &self.invalidation_plan_digest {
            digest(invalidation, "inspector_invalidation_digest")?;
        } else if self.deletion_state == "preserved_invalid" {
            return Err("inspector_invalidation_digest_required".to_owned());
        }
        self.validate_collections()
    }

    fn validate_collections(&self) -> Result<(), String> {
        let mut sources = BTreeSet::new();
        for source in &self.sources {
            source.validate()?;
            if !sources.insert((source.source_id.clone(), source.stage)) {
                return Err("inspector_source_duplicate".to_owned());
            }
        }
        for omission in &self.omissions {
            omission.validate()?;
        }
        if self.candidate_ids.len() > 256
            || self
                .candidate_ids
                .iter()
                .any(|candidate| candidate.trim().is_empty() || candidate.len() > 512)
            || self.candidate_state_digests.len() > 256
            || self
                .candidate_state_digests
                .iter()
                .any(|value| digest(value, "inspector_candidate_state_digest").is_err())
        {
            return Err("inspector_candidate_projection_invalid".to_owned());
        }
        digest(&self.snapshot_digest, "inspector_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("inspector_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "inspector_id": self.inspector_id,
            "receipt_digest": self.receipt_digest,
            "context_plan_digest": self.context_plan_digest,
            "projection": self.projection,
            "data_epoch": self.data_epoch,
            "invalidation_plan_digest": self.invalidation_plan_digest,
            "sources": self.sources,
            "omissions": self.omissions,
            "candidate_ids": self.candidate_ids,
            "candidate_state_digests": self.candidate_state_digests,
            "deletion_state": self.deletion_state,
            "rebuild_state": self.rebuild_state,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserMemoryCorrection {
    pub schema: String,
    pub version: SchemaVersion,
    pub correction_id: String,
    pub inspector_digest: String,
    pub mutation: MemoryMutation,
    pub operator_approval: bool,
    pub correction_digest: String,
}

impl UserMemoryCorrection {
    pub fn new(
        correction_id: impl Into<String>,
        inspector: &ContextMemoryInspectorSnapshot,
        mutation: MemoryMutation,
        operator_approval: bool,
    ) -> Result<Self, String> {
        inspector.validate()?;
        mutation.validate()?;
        if !matches!(
            mutation.operation,
            MemoryMutationOperation::Update
                | MemoryMutationOperation::Delete
                | MemoryMutationOperation::Revoke
        ) {
            return Err("user_correction_operation_not_allowed".to_owned());
        }
        let mut correction = Self {
            schema: USER_CORRECTION_SCHEMA.to_owned(),
            version: INSPECTOR_VERSION,
            correction_id: correction_id.into(),
            inspector_digest: inspector.snapshot_digest.clone(),
            mutation,
            operator_approval,
            correction_digest: String::new(),
        };
        correction.correction_digest = correction.digest();
        correction.validate_against(inspector)?;
        Ok(correction)
    }

    pub fn validate_against(
        &self,
        inspector: &ContextMemoryInspectorSnapshot,
    ) -> Result<(), String> {
        inspector.validate()?;
        if self.schema != USER_CORRECTION_SCHEMA
            || self.version != INSPECTOR_VERSION
            || self.inspector_digest != inspector.snapshot_digest
            || !self.operator_approval
        {
            return Err("user_correction_governance_required".to_owned());
        }
        required(&self.correction_id, "user_correction_id", 256)?;
        self.mutation.validate()?;
        if !matches!(
            self.mutation.operation,
            MemoryMutationOperation::Update
                | MemoryMutationOperation::Delete
                | MemoryMutationOperation::Revoke
        ) {
            return Err("user_correction_operation_not_allowed".to_owned());
        }
        digest(&self.correction_digest, "user_correction_digest")?;
        if self.correction_digest != self.digest() {
            return Err("user_correction_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "correction_id": self.correction_id,
            "inspector_digest": self.inspector_digest,
            "mutation": self.mutation,
            "operator_approval": self.operator_approval,
        }))
    }
}
