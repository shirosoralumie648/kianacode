//! Harness surface/scenario integration evidence contract.
//!
//! Rows classify CI/source evidence; they do not execute a provider, grant a capability or turn
//! a Desktop view into an execution receipt.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const HARNESS_INTEGRATION_SCHEMA: &str = "kiana.harness-integration-matrix.v1";
pub const HARNESS_INTEGRATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessSurface {
    Cli,
    Workbench,
    Web,
    Desktop,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessScenario {
    ShortTask,
    ToolCall,
    Repair,
    Steer,
    Cancel,
    Approval,
    Compaction,
    Restart,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessCaseStatus {
    Verified,
    NotImplemented,
    NotApplicable,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessIntegrationCase {
    pub surface: HarnessSurface,
    pub scenario: HarnessScenario,
    pub status: HarnessCaseStatus,
    pub reason: String,
    pub spine_digest: String,
    pub provider_mode: String,
    pub operator_approved: bool,
    pub receipt_digest: Option<String>,
    pub cancel_fence_verified: bool,
    pub stream_evidence: bool,
    pub desktop_state_receipt: bool,
    pub result_unknown: bool,
    pub case_digest: String,
}

impl HarnessIntegrationCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        surface: HarnessSurface,
        scenario: HarnessScenario,
        status: HarnessCaseStatus,
        reason: impl Into<String>,
        spine_digest: impl Into<String>,
        provider_mode: impl Into<String>,
        operator_approved: bool,
        receipt_digest: Option<String>,
        cancel_fence_verified: bool,
        stream_evidence: bool,
        desktop_state_receipt: bool,
        result_unknown: bool,
    ) -> Result<Self, String> {
        let mut case = Self {
            surface,
            scenario,
            status,
            reason: reason.into(),
            spine_digest: spine_digest.into(),
            provider_mode: provider_mode.into(),
            operator_approved,
            receipt_digest,
            cancel_fence_verified,
            stream_evidence,
            desktop_state_receipt,
            result_unknown,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.reason.len() > 512 || self.reason.contains('\0') || self.provider_mode.is_empty() {
            return Err("harness_integration_case_header_invalid".to_owned());
        }
        validate_digest(&self.spine_digest, "harness_integration_spine_digest")?;
        validate_digest(&self.case_digest, "harness_integration_case_digest")?;
        if let Some(receipt) = &self.receipt_digest {
            validate_digest(receipt, "harness_integration_receipt_digest")?;
        }
        if self.provider_mode == "live_opt_in" && !self.operator_approved {
            return Err("harness_integration_live_approval_missing".to_owned());
        }
        if self.status == HarnessCaseStatus::Verified {
            if self.receipt_digest.is_none() || !self.stream_evidence {
                return Err("harness_integration_verified_evidence_missing".to_owned());
            }
            if self.scenario == HarnessScenario::Cancel && !self.cancel_fence_verified {
                return Err("harness_integration_cancel_fence_missing".to_owned());
            }
            if self.surface == HarnessSurface::Desktop && !self.desktop_state_receipt {
                return Err("harness_integration_desktop_receipt_missing".to_owned());
            }
        } else if self.reason.trim().is_empty() {
            return Err("harness_integration_nonverified_reason_required".to_owned());
        }
        if self.case_digest != self.digest() {
            return Err("harness_integration_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "surface": self.surface,
            "scenario": self.scenario,
            "status": self.status,
            "reason": self.reason,
            "spine_digest": self.spine_digest,
            "provider_mode": self.provider_mode,
            "operator_approved": self.operator_approved,
            "receipt_digest": self.receipt_digest,
            "cancel_fence_verified": self.cancel_fence_verified,
            "stream_evidence": self.stream_evidence,
            "desktop_state_receipt": self.desktop_state_receipt,
            "result_unknown": self.result_unknown,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessIntegrationMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub spine_digest: String,
    pub cases: Vec<HarnessIntegrationCase>,
    pub matrix_digest: String,
}

impl HarnessIntegrationMatrix {
    pub fn new(
        spine_digest: impl Into<String>,
        cases: Vec<HarnessIntegrationCase>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: HARNESS_INTEGRATION_SCHEMA.to_owned(),
            version: HARNESS_INTEGRATION_VERSION,
            spine_digest: spine_digest.into(),
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HARNESS_INTEGRATION_SCHEMA || self.version != HARNESS_INTEGRATION_VERSION
        {
            return Err("harness_integration_matrix_header_invalid".to_owned());
        }
        validate_digest(
            &self.spine_digest,
            "harness_integration_matrix_spine_digest",
        )?;
        validate_digest(&self.matrix_digest, "harness_integration_matrix_digest")?;
        if self.cases.is_empty() || self.cases.len() > 128 {
            return Err("harness_integration_case_count_invalid".to_owned());
        }
        let mut keys = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.spine_digest != self.spine_digest {
                return Err("harness_integration_spine_drift".to_owned());
            }
            if !keys.insert((case.surface, case.scenario)) {
                return Err("harness_integration_case_duplicate".to_owned());
            }
        }
        for surface in [
            HarnessSurface::Cli,
            HarnessSurface::Workbench,
            HarnessSurface::Web,
            HarnessSurface::Desktop,
        ] {
            for scenario in [
                HarnessScenario::ShortTask,
                HarnessScenario::ToolCall,
                HarnessScenario::Repair,
                HarnessScenario::Steer,
                HarnessScenario::Cancel,
                HarnessScenario::Approval,
                HarnessScenario::Compaction,
                HarnessScenario::Restart,
            ] {
                if !keys.contains(&(surface, scenario)) {
                    return Err("harness_integration_coverage_missing".to_owned());
                }
            }
        }
        if self.matrix_digest != self.digest() {
            return Err("harness_integration_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "spine_digest": self.spine_digest,
            "cases": self.cases,
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
