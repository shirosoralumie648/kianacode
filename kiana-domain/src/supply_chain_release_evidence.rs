//! Supply-chain release handoff evidence.
//!
//! A gate report is not a release action. This contract binds a release decision to the gate,
//! artifact digests, approval, rollback plan and (when applicable) an independent publish receipt.
//! It does not sign, upload, install or publish anything.

use crate::{json_digest, SchemaVersion, SupplyChainGateStatus};
use serde::{Deserialize, Serialize};

pub const SUPPLY_CHAIN_RELEASE_EVIDENCE_SCHEMA: &str = "kiana.supply-chain-release-evidence.v1";
pub const SUPPLY_CHAIN_RELEASE_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_ARTIFACTS: usize = 128;
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyChainReleaseDisposition {
    NotReleased,
    Approved,
    Published,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyChainReleaseEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub release_id: String,
    pub source_snapshot_digest: String,
    pub gate_report_digest: String,
    pub gate_status: SupplyChainGateStatus,
    pub disposition: SupplyChainReleaseDisposition,
    pub artifact_digests: Vec<String>,
    pub operator_approval_ref: Option<String>,
    pub publish_receipt_digest: Option<String>,
    pub rollback_plan_digest: String,
    pub result_unknown: bool,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl SupplyChainReleaseEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        release_id: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        gate_report_digest: impl Into<String>,
        gate_status: SupplyChainGateStatus,
        disposition: SupplyChainReleaseDisposition,
        mut artifact_digests: Vec<String>,
        operator_approval_ref: Option<String>,
        publish_receipt_digest: Option<String>,
        rollback_plan_digest: impl Into<String>,
        result_unknown: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        artifact_digests.sort();
        artifact_digests.dedup();
        let mut evidence = Self {
            schema: SUPPLY_CHAIN_RELEASE_EVIDENCE_SCHEMA.to_owned(),
            version: SUPPLY_CHAIN_RELEASE_EVIDENCE_VERSION,
            release_id: release_id.into(),
            source_snapshot_digest: source_snapshot_digest.into(),
            gate_report_digest: gate_report_digest.into(),
            gate_status,
            disposition,
            artifact_digests,
            operator_approval_ref,
            publish_receipt_digest,
            rollback_plan_digest: rollback_plan_digest.into(),
            result_unknown,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SUPPLY_CHAIN_RELEASE_EVIDENCE_SCHEMA
            || self.version != SUPPLY_CHAIN_RELEASE_EVIDENCE_VERSION
        {
            return Err("supply_chain_release_evidence_header_invalid".to_owned());
        }
        bounded(&self.release_id, "supply_chain_release_id", 256)?;
        for (value, field) in [
            (
                &self.source_snapshot_digest,
                "supply_chain_release_source_snapshot_digest",
            ),
            (
                &self.gate_report_digest,
                "supply_chain_release_gate_report_digest",
            ),
            (
                &self.rollback_plan_digest,
                "supply_chain_release_rollback_plan_digest",
            ),
        ] {
            digest(value, field)?;
        }
        if self.artifact_digests.is_empty() || self.artifact_digests.len() > MAX_ARTIFACTS {
            return Err("supply_chain_release_artifact_count_invalid".to_owned());
        }
        if self
            .artifact_digests
            .iter()
            .any(|value| digest(value, "supply_chain_release_artifact_digest").is_err())
            || self
                .artifact_digests
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("supply_chain_release_artifacts_invalid".to_owned());
        }
        if let Some(value) = &self.operator_approval_ref {
            bounded(value, "supply_chain_release_operator_approval_ref", 256)?;
        }
        if let Some(value) = &self.publish_receipt_digest {
            digest(value, "supply_chain_release_publish_receipt_digest")?;
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("supply_chain_release_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "supply_chain_release_limitation", 256)?;
        }
        if self.gate_status == SupplyChainGateStatus::Blocked
            && matches!(
                self.disposition,
                SupplyChainReleaseDisposition::Approved | SupplyChainReleaseDisposition::Published
            )
        {
            return Err("supply_chain_blocked_release".to_owned());
        }
        if self.result_unknown && self.disposition == SupplyChainReleaseDisposition::Published {
            return Err("supply_chain_unknown_publish_cannot_verify".to_owned());
        }
        if matches!(
            self.disposition,
            SupplyChainReleaseDisposition::Approved | SupplyChainReleaseDisposition::Published
        ) {
            let Some(approval_ref) = self.operator_approval_ref.as_deref() else {
                return Err("supply_chain_release_approval_or_gate_missing".to_owned());
            };
            if self.gate_status != SupplyChainGateStatus::Ready {
                return Err("supply_chain_release_approval_or_gate_missing".to_owned());
            }
            if approval_ref.trim() != approval_ref
                || !approval_ref.to_ascii_lowercase().starts_with("approval:")
            {
                return Err("supply_chain_release_approval_ref_invalid".to_owned());
            }
        }
        if self.disposition == SupplyChainReleaseDisposition::Published
            && self.publish_receipt_digest.is_none()
        {
            return Err("supply_chain_publish_receipt_missing".to_owned());
        }
        if self.disposition != SupplyChainReleaseDisposition::Published
            && self.limitations.is_empty()
        {
            return Err("supply_chain_release_nonpublished_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("supply_chain_release_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "release_id": self.release_id,
            "source_snapshot_digest": self.source_snapshot_digest,
            "gate_report_digest": self.gate_report_digest,
            "gate_status": self.gate_status,
            "disposition": self.disposition,
            "artifact_digests": self.artifact_digests,
            "operator_approval_ref": self.operator_approval_ref,
            "publish_receipt_digest": self.publish_receipt_digest,
            "rollback_plan_digest": self.rollback_plan_digest,
            "result_unknown": self.result_unknown,
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

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
