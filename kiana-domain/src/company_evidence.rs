//! Immutable Company execution evidence bound to an actual run and invocation.
//!
//! This contract is deliberately stricter than the legacy business `EvidenceBundle`: a completed
//! model turn or self-reported text is not enough.  The bundle must carry a terminal runtime
//! receipt, nonzero test matches, exact source/workspace revisions and artifact ownership.

use crate::{
    json_digest, normalize_role_path, ExecutionStatus, InvocationId, RequestId, RunId,
    RuntimeReceiptRef,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const COMPANY_EVIDENCE_SCHEMA: &str = "kiana.company-evidence-bundle.v1";
pub const COMPANY_EVIDENCE_READY_SCHEMA: &str = "kiana.company-evidence-ready.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEvidenceFileChange {
    pub path: String,
    pub operation: String,
    pub content_hash: String,
}

impl CompanyEvidenceFileChange {
    pub fn validate(&self) -> Result<(), &'static str> {
        normalize_role_path(&self.path).ok_or("company_evidence_file_path_invalid")?;
        required(&self.operation, "company_evidence_file_operation_invalid")?;
        digest(&self.content_hash, "company_evidence_file_hash_invalid")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEvidenceTestResult {
    pub command: String,
    pub exit_code: i32,
    pub matched_tests: u64,
    pub source_revision: String,
    pub output_digest: String,
}

impl CompanyEvidenceTestResult {
    pub fn validate(&self, expected_source_revision: &str) -> Result<(), &'static str> {
        required(&self.command, "company_evidence_test_command_invalid")?;
        required(
            &self.source_revision,
            "company_evidence_test_source_invalid",
        )?;
        digest(&self.output_digest, "company_evidence_test_output_invalid")?;
        if self.exit_code != 0 {
            return Err("company_evidence_test_exit_nonzero");
        }
        if self.matched_tests == 0 {
            return Err("company_evidence_test_zero_matches");
        }
        if self.source_revision != expected_source_revision {
            return Err("company_evidence_test_source_revision_mismatch");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEvidenceArtifact {
    pub artifact_ref: String,
    pub project_id: String,
    pub packet_id: String,
    pub run_id: RunId,
    pub content_hash: String,
}

impl CompanyEvidenceArtifact {
    pub fn validate(
        &self,
        project_id: &str,
        packet_id: &str,
        run_id: RunId,
    ) -> Result<(), &'static str> {
        if !self.artifact_ref.starts_with("artifact:")
            || self.artifact_ref.len() <= "artifact:".len()
            || self.project_id != project_id
            || self.packet_id != packet_id
            || self.run_id != run_id
        {
            return Err("company_evidence_foreign_artifact");
        }
        digest(&self.content_hash, "company_evidence_artifact_hash_invalid")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEvidenceBundle {
    pub schema: String,
    pub bundle_id: String,
    pub project_id: String,
    pub packet_id: String,
    pub packet_version: u64,
    pub run_id: RunId,
    pub invocation_id: InvocationId,
    pub request_id: RequestId,
    pub operation: String,
    pub command_digest: String,
    pub source_revision: String,
    pub workspace_revision: String,
    pub exit_code: Option<i32>,
    pub runtime_receipt: RuntimeReceiptRef,
    pub file_changes: Vec<CompanyEvidenceFileChange>,
    pub artifacts: Vec<CompanyEvidenceArtifact>,
    pub tests: Vec<CompanyEvidenceTestResult>,
    #[serde(default)]
    pub model_claimed: bool,
    #[serde(default)]
    pub result_unknown: bool,
    pub digest: String,
}

impl CompanyEvidenceBundle {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_EVIDENCE_SCHEMA
            || self.packet_version == 0
            || self.run_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.exit_code.is_none()
            || self.model_claimed
            || self.result_unknown
        {
            return Err("company_evidence_runtime_or_identity_invalid");
        }
        required(&self.bundle_id, "company_evidence_bundle_id_required")?;
        required(&self.project_id, "company_evidence_project_required")?;
        required(&self.packet_id, "company_evidence_packet_required")?;
        required(&self.operation, "company_evidence_operation_required")?;
        required(&self.source_revision, "company_evidence_source_required")?;
        required(
            &self.workspace_revision,
            "company_evidence_workspace_required",
        )?;
        digest(
            &self.command_digest,
            "company_evidence_command_digest_invalid",
        )?;
        self.runtime_receipt
            .validate()
            .map_err(|_| "company_evidence_receipt_invalid")?;
        if self.runtime_receipt.request_id != self.request_id
            || self.runtime_receipt.status != ExecutionStatus::Completed
        {
            return Err("company_evidence_receipt_binding_invalid");
        }
        if self.file_changes.is_empty() && self.artifacts.is_empty() {
            return Err("company_evidence_output_missing");
        }
        for change in &self.file_changes {
            change.validate()?;
        }
        for artifact in &self.artifacts {
            artifact.validate(&self.project_id, &self.packet_id, self.run_id)?;
        }
        if self.tests.is_empty() {
            return Err("company_evidence_tests_missing");
        }
        for test in &self.tests {
            test.validate(&self.source_revision)?;
        }
        if self.digest != self.canonical_digest() {
            return Err("company_evidence_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "bundle_id": self.bundle_id,
            "project_id": self.project_id,
            "packet_id": self.packet_id,
            "packet_version": self.packet_version,
            "run_id": self.run_id,
            "invocation_id": self.invocation_id,
            "request_id": self.request_id,
            "operation": self.operation,
            "command_digest": self.command_digest,
            "source_revision": self.source_revision,
            "workspace_revision": self.workspace_revision,
            "exit_code": self.exit_code,
            "runtime_receipt": self.runtime_receipt,
            "file_changes": self.file_changes,
            "artifacts": self.artifacts,
            "tests": self.tests,
            "model_claimed": self.model_claimed,
            "result_unknown": self.result_unknown,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyEvidenceReady {
    pub schema: String,
    pub bundle_id: String,
    pub bundle_digest: String,
    pub run_id: RunId,
    pub invocation_id: InvocationId,
    pub source_cursor: u64,
    pub ready_at: u64,
    pub digest: String,
}

impl CompanyEvidenceReady {
    pub fn from_bundle(
        bundle: &CompanyEvidenceBundle,
        source_cursor: u64,
        ready_at: u64,
    ) -> Result<Self, &'static str> {
        bundle.validate()?;
        if source_cursor == 0 || ready_at == 0 {
            return Err("company_evidence_ready_cursor_invalid");
        }
        let mut ready = Self {
            schema: COMPANY_EVIDENCE_READY_SCHEMA.to_owned(),
            bundle_id: bundle.bundle_id.clone(),
            bundle_digest: bundle.digest.clone(),
            run_id: bundle.run_id,
            invocation_id: bundle.invocation_id,
            source_cursor,
            ready_at,
            digest: String::new(),
        };
        ready.digest = ready.canonical_digest();
        ready.validate()?;
        Ok(ready)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_EVIDENCE_READY_SCHEMA
            || self.run_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.ready_at == 0
        {
            return Err("company_evidence_ready_invalid");
        }
        required(&self.bundle_id, "company_evidence_ready_bundle_required")?;
        digest(
            &self.bundle_digest,
            "company_evidence_ready_bundle_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("company_evidence_ready_digest_mismatch");
        }
        Ok(())
    }

    fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "bundle_id": self.bundle_id,
            "bundle_digest": self.bundle_digest,
            "run_id": self.run_id,
            "invocation_id": self.invocation_id,
            "source_cursor": self.source_cursor,
            "ready_at": self.ready_at,
        }))
    }
}
