//! Release supply-chain evidence and compliance gate.
//!
//! The gate binds package, checksum, signature, SBOM, license and secret-scan evidence to one
//! reviewed source/lock/policy snapshot. It is a read-only decision contract; it does not sign,
//! publish, install or claim that a CI fixture is an external release.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SUPPLY_CHAIN_SCHEMA: &str = "kiana.supply-chain-gate.v1";
pub const SUPPLY_CHAIN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SUPPLY_CHAIN_ARTIFACTS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyChainArtifactKind {
    Binary,
    Archive,
    DesktopPackage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyChainArtifactEvidence {
    pub artifact_id: String,
    pub kind: SupplyChainArtifactKind,
    pub target: String,
    pub artifact_digest: String,
    pub checksum_digest: String,
    pub signature_digest: String,
    pub package_manifest_digest: String,
    pub checksum_verified: bool,
    pub signature_verified: bool,
    pub sbom_bound: bool,
    pub license_compliant: bool,
    pub secret_scan_passed: bool,
}

impl SupplyChainArtifactEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact_id: impl Into<String>,
        kind: SupplyChainArtifactKind,
        target: impl Into<String>,
        artifact_digest: impl Into<String>,
        checksum_digest: impl Into<String>,
        signature_digest: impl Into<String>,
        package_manifest_digest: impl Into<String>,
        checksum_verified: bool,
        signature_verified: bool,
        sbom_bound: bool,
        license_compliant: bool,
        secret_scan_passed: bool,
    ) -> Result<Self, String> {
        let artifact = Self {
            artifact_id: artifact_id.into(),
            kind,
            target: target.into(),
            artifact_digest: artifact_digest.into(),
            checksum_digest: checksum_digest.into(),
            signature_digest: signature_digest.into(),
            package_manifest_digest: package_manifest_digest.into(),
            checksum_verified,
            signature_verified,
            sbom_bound,
            license_compliant,
            secret_scan_passed,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.artifact_id, "supply_chain_artifact_id")?;
        bounded(&self.target, "supply_chain_artifact_target")?;
        for (value, field) in [
            (&self.artifact_digest, "supply_chain_artifact_digest"),
            (&self.checksum_digest, "supply_chain_checksum_digest"),
            (&self.signature_digest, "supply_chain_signature_digest"),
            (
                &self.package_manifest_digest,
                "supply_chain_package_manifest_digest",
            ),
        ] {
            validate_digest(value, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyChainEvidence {
    pub source_tree_digest: String,
    pub cargo_lock_digest: String,
    pub sbom_digest: String,
    pub license_report_digest: String,
    pub secret_scan_digest: String,
    pub signing_policy_digest: String,
    pub compliance_policy_digest: String,
    pub source_tree_clean: bool,
    pub dependency_scan_passed: bool,
    pub license_unknown_count: u32,
    pub secret_scan_passed: bool,
    pub reviewer: String,
    pub artifacts: Vec<SupplyChainArtifactEvidence>,
    pub evidence_digest: String,
}

impl SupplyChainEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_tree_digest: impl Into<String>,
        cargo_lock_digest: impl Into<String>,
        sbom_digest: impl Into<String>,
        license_report_digest: impl Into<String>,
        secret_scan_digest: impl Into<String>,
        signing_policy_digest: impl Into<String>,
        compliance_policy_digest: impl Into<String>,
        source_tree_clean: bool,
        dependency_scan_passed: bool,
        license_unknown_count: u32,
        secret_scan_passed: bool,
        reviewer: impl Into<String>,
        artifacts: Vec<SupplyChainArtifactEvidence>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            source_tree_digest: source_tree_digest.into(),
            cargo_lock_digest: cargo_lock_digest.into(),
            sbom_digest: sbom_digest.into(),
            license_report_digest: license_report_digest.into(),
            secret_scan_digest: secret_scan_digest.into(),
            signing_policy_digest: signing_policy_digest.into(),
            compliance_policy_digest: compliance_policy_digest.into(),
            source_tree_clean,
            dependency_scan_passed,
            license_unknown_count,
            secret_scan_passed,
            reviewer: reviewer.into(),
            artifacts,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.source_tree_digest, "supply_chain_source_tree_digest"),
            (&self.cargo_lock_digest, "supply_chain_cargo_lock_digest"),
            (&self.sbom_digest, "supply_chain_sbom_digest"),
            (
                &self.license_report_digest,
                "supply_chain_license_report_digest",
            ),
            (&self.secret_scan_digest, "supply_chain_secret_scan_digest"),
            (
                &self.signing_policy_digest,
                "supply_chain_signing_policy_digest",
            ),
            (
                &self.compliance_policy_digest,
                "supply_chain_compliance_policy_digest",
            ),
            (&self.evidence_digest, "supply_chain_evidence_digest"),
        ] {
            validate_digest(value, field)?;
        }
        bounded(&self.reviewer, "supply_chain_reviewer")?;
        if self.artifacts.is_empty() || self.artifacts.len() > MAX_SUPPLY_CHAIN_ARTIFACTS {
            return Err("supply_chain_artifact_count_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !ids.insert(artifact.artifact_id.as_str()) {
                return Err("supply_chain_artifact_duplicate".to_owned());
            }
        }
        if self.evidence_digest != self.digest() {
            return Err("supply_chain_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "source_tree_digest": self.source_tree_digest,
            "cargo_lock_digest": self.cargo_lock_digest,
            "sbom_digest": self.sbom_digest,
            "license_report_digest": self.license_report_digest,
            "secret_scan_digest": self.secret_scan_digest,
            "signing_policy_digest": self.signing_policy_digest,
            "compliance_policy_digest": self.compliance_policy_digest,
            "source_tree_clean": self.source_tree_clean,
            "dependency_scan_passed": self.dependency_scan_passed,
            "license_unknown_count": self.license_unknown_count,
            "secret_scan_passed": self.secret_scan_passed,
            "reviewer": self.reviewer,
            "artifacts": self.artifacts,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupplyChainGateStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupplyChainGateReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: SupplyChainGateStatus,
    pub publish_allowed: bool,
    pub evidence_digest: String,
    pub artifact_count: usize,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl SupplyChainGateReport {
    pub fn evaluate(evidence: &SupplyChainEvidence) -> Result<Self, String> {
        evidence.validate()?;
        let has_desktop_package = evidence
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == SupplyChainArtifactKind::DesktopPackage);
        let decision = if !evidence.source_tree_clean {
            (
                SupplyChainGateStatus::Blocked,
                "supply_chain_source_tree_dirty",
                "rebuild and review from a clean source snapshot",
            )
        } else if !evidence.dependency_scan_passed {
            (
                SupplyChainGateStatus::Blocked,
                "supply_chain_dependency_scan_failed",
                "resolve advisory and dependency policy findings",
            )
        } else if evidence.license_unknown_count > 0 {
            (
                SupplyChainGateStatus::Blocked,
                "supply_chain_license_unknown",
                "review every unknown dependency license before packaging",
            )
        } else if !evidence.secret_scan_passed {
            (
                SupplyChainGateStatus::Blocked,
                "supply_chain_secret_scan_failed",
                "remove or quarantine secret findings and rerun the scan",
            )
        } else if !has_desktop_package {
            (
                SupplyChainGateStatus::Blocked,
                "supply_chain_desktop_package_missing",
                "produce and bind the signed desktop package evidence",
            )
        } else if evidence.artifacts.iter().any(|artifact| {
            !artifact.checksum_verified
                || !artifact.signature_verified
                || !artifact.sbom_bound
                || !artifact.license_compliant
                || !artifact.secret_scan_passed
        }) {
            (
                SupplyChainGateStatus::Blocked,
                "supply_chain_artifact_evidence_incomplete",
                "attach verified checksum, signature, SBOM, license and secret-scan evidence for every artifact",
            )
        } else {
            (
                SupplyChainGateStatus::Ready,
                "ok",
                "publish only after the external release action records its receipt",
            )
        };
        let mut report = Self {
            schema: SUPPLY_CHAIN_SCHEMA.to_owned(),
            version: SUPPLY_CHAIN_VERSION,
            status: decision.0,
            publish_allowed: decision.0 == SupplyChainGateStatus::Ready,
            evidence_digest: evidence.evidence_digest.clone(),
            artifact_count: evidence.artifacts.len(),
            reason: decision.1.to_owned(),
            remediation: decision.2.to_owned(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SUPPLY_CHAIN_SCHEMA || self.version != SUPPLY_CHAIN_VERSION {
            return Err("supply_chain_report_header_invalid".to_owned());
        }
        validate_digest(&self.evidence_digest, "supply_chain_report_evidence_digest")?;
        validate_digest(&self.report_digest, "supply_chain_report_digest")?;
        bounded(&self.reason, "supply_chain_report_reason")?;
        bounded(&self.remediation, "supply_chain_report_remediation")?;
        if self.artifact_count == 0
            || (self.status == SupplyChainGateStatus::Ready && !self.publish_allowed)
            || (self.status == SupplyChainGateStatus::Blocked && self.publish_allowed)
        {
            return Err("supply_chain_report_decision_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("supply_chain_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "publish_allowed": self.publish_allowed,
            "evidence_digest": self.evidence_digest,
            "artifact_count": self.artifact_count,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
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
