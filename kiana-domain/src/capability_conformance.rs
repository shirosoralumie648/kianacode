//! Capability combination matrix and evidence closeout contract.
//!
//! A matrix row is an evidence classification, not an execution permission. Unsupported rows
//! remain visible with a reason; they are never counted as verified.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CAPABILITY_CONFORMANCE_SCHEMA: &str = "kiana.capability-conformance.v1";
pub const CAPABILITY_CONFORMANCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_CONFORMANCE_CASES: usize = 512;
pub const MAX_CONFORMANCE_REASON: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceBackend {
    Linux,
    Macos,
    Windows,
    Container,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceProfile {
    ReadOnly,
    WorkspaceWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceTool {
    Shell,
    ApplyPatch,
    StdioMcp,
    HttpMcp,
    DynamicTool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceScenario {
    Success,
    Cancel,
    Failure,
    LongTask,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceCaseStatus {
    Verified,
    NotApplicable,
    NotImplemented,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceCase {
    pub case_id: String,
    pub backend: ConformanceBackend,
    pub profile: ConformanceProfile,
    pub tool: ConformanceTool,
    pub scenario: ConformanceScenario,
    pub status: ConformanceCaseStatus,
    pub reason: String,
    #[serde(default)]
    pub evidence_digest: Option<String>,
    pub grant_digest: String,
    pub observed_grant_digest: String,
    pub catalog_epoch: u64,
    pub credential_epoch: u64,
    pub data_epoch: u64,
    pub effect_fence_verified: bool,
    pub resume_fence_verified: bool,
}

impl ConformanceCase {
    pub fn validate(&self) -> Result<(), String> {
        if self.case_id.trim().is_empty()
            || self.case_id.len() > 256
            || self.case_id.contains('\0')
            || self.catalog_epoch == 0
            || self.credential_epoch == 0
            || self.data_epoch == 0
            || self.reason.len() > MAX_CONFORMANCE_REASON
            || self.reason.contains('\0')
        {
            return Err("conformance_case_header_invalid".to_owned());
        }
        validate_digest(&self.grant_digest, "conformance_grant_digest")?;
        validate_digest(
            &self.observed_grant_digest,
            "conformance_observed_grant_digest",
        )?;
        if self.status == ConformanceCaseStatus::Verified {
            let evidence = self
                .evidence_digest
                .as_deref()
                .ok_or_else(|| "conformance_verified_evidence_missing".to_owned())?;
            validate_digest(evidence, "conformance_evidence_digest")?;
            if !self.effect_fence_verified
                || (self.scenario == ConformanceScenario::Resume && !self.resume_fence_verified)
            {
                return Err("extension_transport_and_resume_cannot_bypass_effect_fences".to_owned());
            }
            if self.grant_digest != self.observed_grant_digest {
                return Err("backend_switch_cannot_expand_existing_grant".to_owned());
            }
        }
        if matches!(
            self.status,
            ConformanceCaseStatus::NotApplicable
                | ConformanceCaseStatus::NotImplemented
                | ConformanceCaseStatus::Blocked
        ) && self.reason.trim().is_empty()
        {
            return Err("conformance_nonverified_reason_required".to_owned());
        }
        Ok(())
    }

    pub fn key(&self) -> String {
        format!(
            "{:?}:{:?}:{:?}:{:?}",
            self.backend, self.profile, self.tool, self.scenario
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceMatrixStatus {
    Complete,
    Partial,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceMatrixReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: ConformanceMatrixStatus,
    pub verified_count: u32,
    pub not_applicable_count: u32,
    pub not_implemented_count: u32,
    pub blocked_count: u32,
    pub matrix_digest: String,
    pub blocking_reasons: Vec<String>,
    pub report_digest: String,
}

impl ConformanceMatrixReport {
    pub fn evaluate(cases: &[ConformanceCase]) -> Result<Self, String> {
        if cases.is_empty() || cases.len() > MAX_CONFORMANCE_CASES {
            return Err("conformance_matrix_bounds_invalid".to_owned());
        }
        let mut keys = BTreeSet::new();
        for case in cases {
            case.validate()?;
            if !keys.insert(case.key()) {
                return Err("conformance_matrix_duplicate_case".to_owned());
            }
        }
        let verified_count = cases
            .iter()
            .filter(|case| case.status == ConformanceCaseStatus::Verified)
            .count() as u32;
        let not_applicable_count = cases
            .iter()
            .filter(|case| case.status == ConformanceCaseStatus::NotApplicable)
            .count() as u32;
        let not_implemented_count = cases
            .iter()
            .filter(|case| case.status == ConformanceCaseStatus::NotImplemented)
            .count() as u32;
        let blocked_count = cases
            .iter()
            .filter(|case| case.status == ConformanceCaseStatus::Blocked)
            .count() as u32;
        let blocking_reasons = cases
            .iter()
            .filter(|case| {
                matches!(
                    case.status,
                    ConformanceCaseStatus::Blocked | ConformanceCaseStatus::NotImplemented
                )
            })
            .map(|case| format!("{}:{}", case.case_id, case.reason))
            .collect::<Vec<_>>();
        let status = if blocked_count > 0 {
            ConformanceMatrixStatus::Blocked
        } else if not_applicable_count > 0
            || not_implemented_count > 0
            || verified_count != cases.len() as u32
        {
            ConformanceMatrixStatus::Partial
        } else {
            ConformanceMatrixStatus::Complete
        };
        let mut report = Self {
            schema: CAPABILITY_CONFORMANCE_SCHEMA.to_owned(),
            version: CAPABILITY_CONFORMANCE_VERSION,
            status,
            verified_count,
            not_applicable_count,
            not_implemented_count,
            blocked_count,
            matrix_digest: json_digest(&serde_json::to_value(cases).unwrap_or_default()),
            blocking_reasons,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CAPABILITY_CONFORMANCE_SCHEMA
            || self.version != CAPABILITY_CONFORMANCE_VERSION
            || self.verified_count
                + self.not_applicable_count
                + self.not_implemented_count
                + self.blocked_count
                == 0
            || self.blocking_reasons.len() > MAX_CONFORMANCE_CASES
        {
            return Err("conformance_matrix_report_invalid".to_owned());
        }
        validate_digest(&self.matrix_digest, "conformance_matrix_digest")?;
        validate_digest(&self.report_digest, "conformance_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("conformance_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "verified_count": self.verified_count,
            "not_applicable_count": self.not_applicable_count,
            "not_implemented_count": self.not_implemented_count,
            "blocked_count": self.blocked_count,
            "matrix_digest": self.matrix_digest,
            "blocking_reasons": self.blocking_reasons,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
