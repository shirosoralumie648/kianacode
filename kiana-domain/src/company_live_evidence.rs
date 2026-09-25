//! CompanyOS live-model closeout evidence contract.
//!
//! This manifest separates a fake golden path from an explicitly authorized live run. It records
//! the business proof boundary but never executes a model, confirms an external delivery or
//! upgrades a fixture into a real-world outcome.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const COMPANY_LIVE_EVIDENCE_SCHEMA: &str = "kiana.company-live-evidence.v1";
pub const COMPANY_LIVE_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyLiveMode {
    FakeCassette,
    LiveOptIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyLiveProofLevel {
    Source,
    LocalBehavior,
    Live,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyLiveStatus {
    Verified,
    Partial,
    Blocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyLiveRole {
    Planner,
    Builder,
    Reviewer,
    Closer,
}

impl CompanyLiveRole {
    const ALL: [Self; 4] = [Self::Planner, Self::Builder, Self::Reviewer, Self::Closer];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyLiveRoleRoute {
    pub role: CompanyLiveRole,
    pub model_id: String,
    pub route_digest: String,
    pub request_count: u32,
    pub attempt_receipt_digest: Option<String>,
}

impl CompanyLiveRoleRoute {
    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.model_id, "company_live_role_model", 256)?;
        digest(&self.route_digest, "company_live_role_route_digest")?;
        if self.request_count > 128 {
            return Err("company_live_role_request_count_invalid".to_owned());
        }
        if let Some(value) = &self.attempt_receipt_digest {
            digest(value, "company_live_attempt_receipt_digest")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyLiveCloseoutEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub provider_revision: String,
    pub configuration_digest: String,
    pub budget_digest: String,
    pub mode: CompanyLiveMode,
    pub proof_level: CompanyLiveProofLevel,
    pub status: CompanyLiveStatus,
    pub role_routes: Vec<CompanyLiveRoleRoute>,
    pub run_receipt_digest: Option<String>,
    pub review_receipt_digest: Option<String>,
    pub delivery_receipt_digest: Option<String>,
    pub closing_receipt_digest: Option<String>,
    pub artifact_digest: Option<String>,
    pub usage_digest: Option<String>,
    pub independent_reviewer: bool,
    pub local_delivery_confirmed: bool,
    pub outcome_recorded: bool,
    pub result_unknown: bool,
    pub operator_approval_ref: Option<String>,
    pub live_provider_evidence_digest: Option<String>,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl CompanyLiveCloseoutEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: impl Into<String>,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        provider_revision: impl Into<String>,
        configuration_digest: impl Into<String>,
        budget_digest: impl Into<String>,
        mode: CompanyLiveMode,
        proof_level: CompanyLiveProofLevel,
        status: CompanyLiveStatus,
        role_routes: Vec<CompanyLiveRoleRoute>,
        run_receipt_digest: Option<String>,
        review_receipt_digest: Option<String>,
        delivery_receipt_digest: Option<String>,
        closing_receipt_digest: Option<String>,
        artifact_digest: Option<String>,
        usage_digest: Option<String>,
        independent_reviewer: bool,
        local_delivery_confirmed: bool,
        outcome_recorded: bool,
        result_unknown: bool,
        operator_approval_ref: Option<String>,
        live_provider_evidence_digest: Option<String>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: COMPANY_LIVE_EVIDENCE_SCHEMA.to_owned(),
            version: COMPANY_LIVE_EVIDENCE_VERSION,
            project_id: project_id.into(),
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            provider_revision: provider_revision.into(),
            configuration_digest: configuration_digest.into(),
            budget_digest: budget_digest.into(),
            mode,
            proof_level,
            status,
            role_routes,
            run_receipt_digest,
            review_receipt_digest,
            delivery_receipt_digest,
            closing_receipt_digest,
            artifact_digest,
            usage_digest,
            independent_reviewer,
            local_delivery_confirmed,
            outcome_recorded,
            result_unknown,
            operator_approval_ref,
            live_provider_evidence_digest,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_LIVE_EVIDENCE_SCHEMA
            || self.version != COMPANY_LIVE_EVIDENCE_VERSION
        {
            return Err("company_live_evidence_header_invalid".to_owned());
        }
        bounded(&self.project_id, "company_live_project_id", 256)?;
        bounded(&self.provider_id, "company_live_provider_id", 128)?;
        bounded(&self.model_id, "company_live_model_id", 256)?;
        revision(&self.provider_revision, "company_live_provider_revision")?;
        digest(
            &self.configuration_digest,
            "company_live_configuration_digest",
        )?;
        digest(&self.budget_digest, "company_live_budget_digest")?;
        if self.role_routes.is_empty() || self.role_routes.len() > CompanyLiveRole::ALL.len() {
            return Err("company_live_role_route_count_invalid".to_owned());
        }
        let mut roles = BTreeSet::new();
        for route in &self.role_routes {
            route.validate()?;
            if !roles.insert(route.role) {
                return Err("company_live_role_route_duplicate".to_owned());
            }
        }
        for (value, field) in [
            (&self.run_receipt_digest, "company_live_run_receipt_digest"),
            (
                &self.review_receipt_digest,
                "company_live_review_receipt_digest",
            ),
            (
                &self.delivery_receipt_digest,
                "company_live_delivery_receipt_digest",
            ),
            (
                &self.closing_receipt_digest,
                "company_live_closing_receipt_digest",
            ),
            (&self.artifact_digest, "company_live_artifact_digest"),
            (&self.usage_digest, "company_live_usage_digest"),
            (
                &self.live_provider_evidence_digest,
                "company_live_provider_evidence_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if let Some(value) = &self.operator_approval_ref {
            bounded(value, "company_live_operator_approval_ref", 256)?;
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("company_live_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "company_live_limitation", 256)?;
        }
        match self.mode {
            CompanyLiveMode::FakeCassette => {
                if self.proof_level == CompanyLiveProofLevel::Live
                    || self.operator_approval_ref.is_some()
                    || self.live_provider_evidence_digest.is_some()
                {
                    return Err("company_live_fake_cannot_claim_live".to_owned());
                }
                if self.status == CompanyLiveStatus::Verified {
                    return Err("company_live_fake_cannot_verify".to_owned());
                }
            }
            CompanyLiveMode::LiveOptIn => {
                let Some(approval_ref) = self.operator_approval_ref.as_deref() else {
                    return Err("company_live_opt_in_evidence_missing".to_owned());
                };
                if approval_ref.trim() != approval_ref
                    || !approval_ref.to_ascii_lowercase().starts_with("approval:")
                {
                    return Err("company_live_opt_in_approval_ref_invalid".to_owned());
                }
                if self.provider_id.trim() != self.provider_id
                    || self.provider_id.eq_ignore_ascii_case("fake")
                    || self.provider_revision == "none"
                {
                    return Err("company_live_opt_in_provider_identity_missing".to_owned());
                }
                if self.live_provider_evidence_digest.is_none() {
                    return Err("company_live_opt_in_evidence_missing".to_owned());
                }
            }
        }
        if self.result_unknown && self.status == CompanyLiveStatus::Verified {
            return Err("company_live_unknown_cannot_verify".to_owned());
        }
        if self.status == CompanyLiveStatus::Verified {
            if self.mode != CompanyLiveMode::LiveOptIn
                || self.proof_level != CompanyLiveProofLevel::Live
                || self.provider_id == "fake"
                || roles.len() != CompanyLiveRole::ALL.len()
                || CompanyLiveRole::ALL
                    .iter()
                    .any(|role| !roles.contains(role))
                || self
                    .role_routes
                    .iter()
                    .any(|route| route.request_count == 0 || route.attempt_receipt_digest.is_none())
                || self.run_receipt_digest.is_none()
                || self.review_receipt_digest.is_none()
                || self.delivery_receipt_digest.is_none()
                || self.closing_receipt_digest.is_none()
                || self.artifact_digest.is_none()
                || self.usage_digest.is_none()
                || !self.independent_reviewer
                || !self.local_delivery_confirmed
                || !self.outcome_recorded
            {
                return Err("company_live_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("company_live_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("company_live_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_id": self.project_id,
            "provider_id": self.provider_id,
            "model_id": self.model_id,
            "provider_revision": self.provider_revision,
            "configuration_digest": self.configuration_digest,
            "budget_digest": self.budget_digest,
            "mode": self.mode,
            "proof_level": self.proof_level,
            "status": self.status,
            "role_routes": self.role_routes,
            "run_receipt_digest": self.run_receipt_digest,
            "review_receipt_digest": self.review_receipt_digest,
            "delivery_receipt_digest": self.delivery_receipt_digest,
            "closing_receipt_digest": self.closing_receipt_digest,
            "artifact_digest": self.artifact_digest,
            "usage_digest": self.usage_digest,
            "independent_reviewer": self.independent_reviewer,
            "local_delivery_confirmed": self.local_delivery_confirmed,
            "outcome_recorded": self.outcome_recorded,
            "result_unknown": self.result_unknown,
            "operator_approval_ref": self.operator_approval_ref,
            "live_provider_evidence_digest": self.live_provider_evidence_digest,
            "limitations": self.limitations,
        }))
    }
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn revision(value: &str, field: &str) -> Result<(), String> {
    if value == "none" {
        return Ok(());
    }
    digest(value, field)
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
