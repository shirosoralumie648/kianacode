//! Company Integrator conflict and MergeReceipt contract.
//!
//! A merge receipt proves only that a fixed base and reviewed child outputs were combined and
//! revalidated. It never implies Project acceptance, push or external release.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_INTEGRATION_SCHEMA: &str = "kiana.company-integration.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyIntegrationPlan {
    pub schema: String,
    pub integration_id: String,
    pub project_id: String,
    pub base_revision: u64,
    pub output_contract: String,
    pub child_output_digests: BTreeMap<String, String>,
    pub conflict_ids: Vec<String>,
    pub integrator_ref: String,
    pub approval_ref: String,
    pub digest: String,
}

impl CompanyIntegrationPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_INTEGRATION_SCHEMA
            || self.base_revision == 0
            || self.child_output_digests.is_empty()
        {
            return Err("company_integration_plan_header_invalid");
        }
        for (value, field) in [
            (&self.integration_id, "company_integration_id_required"),
            (&self.project_id, "company_integration_project_required"),
            (
                &self.output_contract,
                "company_integration_contract_required",
            ),
            (
                &self.integrator_ref,
                "company_integration_integrator_required",
            ),
            (&self.approval_ref, "company_integration_approval_required"),
        ] {
            required(value, field)?;
        }
        for (child, output) in &self.child_output_digests {
            required(child, "company_integration_child_required")?;
            digest(output, "company_integration_child_digest_invalid")?;
        }
        let mut conflicts = BTreeSet::new();
        for conflict in &self.conflict_ids {
            required(conflict, "company_integration_conflict_invalid")?;
            if !conflicts.insert(conflict) {
                return Err("company_integration_conflict_duplicate");
            }
        }
        digest(&self.digest, "company_integration_plan_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_integration_plan_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "integration_id": self.integration_id,
            "project_id": self.project_id,
            "base_revision": self.base_revision,
            "output_contract": self.output_contract,
            "child_output_digests": self.child_output_digests,
            "conflict_ids": self.conflict_ids,
            "integrator_ref": self.integrator_ref,
            "approval_ref": self.approval_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyConflictDecision {
    pub conflict_id: String,
    pub path: String,
    pub resolution: String,
    pub reviewer_ref: String,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl CompanyConflictDecision {
    fn validate(&self) -> Result<(), &'static str> {
        for (value, field) in [
            (&self.conflict_id, "company_conflict_id_required"),
            (&self.path, "company_conflict_path_required"),
            (&self.resolution, "company_conflict_resolution_required"),
            (&self.reviewer_ref, "company_conflict_reviewer_required"),
        ] {
            required(value, field)?;
        }
        if self.evidence_refs.is_empty() {
            return Err("company_conflict_evidence_required");
        }
        digest(&self.digest, "company_conflict_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_conflict_digest_mismatch");
        }
        Ok(())
    }

    fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "conflict_id": self.conflict_id,
            "path": self.path,
            "resolution": self.resolution,
            "reviewer_ref": self.reviewer_ref,
            "evidence_refs": self.evidence_refs,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyMergeReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub integration_id: String,
    pub plan_digest: String,
    pub base_revision: u64,
    pub result_revision: u64,
    pub child_output_digests: BTreeMap<String, String>,
    pub conflict_decisions: Vec<CompanyConflictDecision>,
    pub revalidated: bool,
    pub accepted: bool,
    pub pushed_or_published: bool,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl CompanyMergeReceipt {
    pub fn validate_against(&self, plan: &CompanyIntegrationPlan) -> Result<(), &'static str> {
        plan.validate()?;
        if self.schema != COMPANY_INTEGRATION_SCHEMA
            || self.integration_id != plan.integration_id
            || self.plan_digest != plan.digest
            || self.base_revision != plan.base_revision
            || self.result_revision <= self.base_revision
            || self.child_output_digests != plan.child_output_digests
            || !self.revalidated
            || self.pushed_or_published
            || self.evidence_refs.is_empty()
        {
            return Err("company_merge_receipt_binding_invalid");
        }
        required(&self.receipt_id, "company_merge_receipt_id_required")?;
        let expected_conflicts = plan.conflict_ids.iter().cloned().collect::<BTreeSet<_>>();
        let actual_conflicts = self
            .conflict_decisions
            .iter()
            .map(|decision| decision.conflict_id.clone())
            .collect::<BTreeSet<_>>();
        if actual_conflicts != expected_conflicts {
            return Err("company_merge_conflict_coverage_required");
        }
        for decision in &self.conflict_decisions {
            decision.validate()?;
        }
        if self.accepted && (!expected_conflicts.is_empty() && self.conflict_decisions.is_empty()) {
            return Err("company_merge_unreviewed_conflict");
        }
        digest(&self.plan_digest, "company_merge_plan_digest_invalid")?;
        digest(&self.digest, "company_merge_receipt_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_merge_receipt_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "integration_id": self.integration_id,
            "plan_digest": self.plan_digest,
            "base_revision": self.base_revision,
            "result_revision": self.result_revision,
            "child_output_digests": self.child_output_digests,
            "conflict_decisions": self.conflict_decisions,
            "revalidated": self.revalidated,
            "accepted": self.accepted,
            "pushed_or_published": self.pushed_or_published,
            "evidence_refs": self.evidence_refs,
        }))
    }
}
