//! Read-only release preflight contract.
//!
//! The report checks reproducibility, lockfile/build identity, target artifacts, SBOM/signature
//! evidence and migration/backup gates. It never publishes an artifact or changes migration state.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const RELEASE_PREFLIGHT_SCHEMA: &str = "kiana.release-preflight.v1";
pub const RELEASE_PREFLIGHT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RELEASE_TARGETS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTargetFact {
    pub target: String,
    pub artifact_digest: String,
    pub checksum_digest: String,
    pub signature_digest: String,
    pub cargo_lock_digest: String,
    pub reproducible: bool,
    pub sbom_present: bool,
    pub signature_verified: bool,
}

impl ReleaseTargetFact {
    pub fn validate(&self) -> Result<(), String> {
        if self.target.trim().is_empty() || self.target.len() > 128 || self.target.contains('\0') {
            return Err("release_target_invalid".to_owned());
        }
        for (value, field) in [
            (&self.artifact_digest, "release_artifact_digest"),
            (&self.checksum_digest, "release_checksum_digest"),
            (&self.signature_digest, "release_signature_digest"),
            (&self.cargo_lock_digest, "release_target_cargo_lock_digest"),
        ] {
            validate_digest(value, field)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePreflightFacts {
    pub release_manifest_digest: String,
    pub expected_cargo_lock_digest: String,
    pub observed_cargo_lock_digest: String,
    pub build_digest: String,
    pub expected_build_digest: String,
    pub reproducible_build: bool,
    pub source_tree_clean: bool,
    pub sbom_present: bool,
    pub signature_verified: bool,
    pub migration_registry_digest: String,
    pub migration_preflight_ready: bool,
    pub verified_backup: bool,
    pub targets: Vec<ReleaseTargetFact>,
}

impl ReleasePreflightFacts {
    pub fn validate(&self) -> Result<(), String> {
        for (value, field) in [
            (&self.release_manifest_digest, "release_manifest_digest"),
            (
                &self.expected_cargo_lock_digest,
                "release_expected_cargo_lock_digest",
            ),
            (
                &self.observed_cargo_lock_digest,
                "release_observed_cargo_lock_digest",
            ),
            (&self.build_digest, "release_build_digest"),
            (&self.expected_build_digest, "release_expected_build_digest"),
            (
                &self.migration_registry_digest,
                "release_migration_registry_digest",
            ),
        ] {
            validate_digest(value, field)?;
        }
        if self.targets.is_empty() || self.targets.len() > MAX_RELEASE_TARGETS {
            return Err("release_target_matrix_invalid".to_owned());
        }
        let mut names = std::collections::BTreeSet::new();
        for target in &self.targets {
            target.validate()?;
            if !names.insert(target.target.as_str()) {
                return Err("release_target_duplicate".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or_default())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleasePreflightStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePreflightReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: ReleasePreflightStatus,
    pub publish_allowed: bool,
    pub release_manifest_digest: String,
    pub fact_digest: String,
    pub reason: String,
    pub remediation: String,
    pub target_count: usize,
    pub report_digest: String,
}

impl ReleasePreflightReport {
    pub fn evaluate(facts: &ReleasePreflightFacts) -> Result<Self, String> {
        facts.validate()?;
        let decision = if !facts.source_tree_clean {
            (
                ReleasePreflightStatus::Blocked,
                "release_source_tree_dirty",
                "build from a clean pinned source tree",
            )
        } else if !facts.reproducible_build {
            (
                ReleasePreflightStatus::Blocked,
                "release_build_not_reproducible",
                "rebuild with the pinned toolchain and deterministic inputs",
            )
        } else if facts.build_digest != facts.expected_build_digest {
            (
                ReleasePreflightStatus::Blocked,
                "release_build_digest_mismatch",
                "reconcile the manifest and produced build digest",
            )
        } else if facts.observed_cargo_lock_digest != facts.expected_cargo_lock_digest {
            (
                ReleasePreflightStatus::Blocked,
                "release_cargo_lock_digest_mismatch",
                "use the exact reviewed Cargo.lock",
            )
        } else if !facts.sbom_present {
            (
                ReleasePreflightStatus::Blocked,
                "release_sbom_missing",
                "generate and attach the SBOM before release",
            )
        } else if !facts.signature_verified {
            (
                ReleasePreflightStatus::Blocked,
                "release_signature_unverified",
                "verify the release signature against a trusted key",
            )
        } else if !facts.migration_preflight_ready {
            (
                ReleasePreflightStatus::Blocked,
                "release_migration_preflight_blocked",
                "resolve every migration preflight axis before release",
            )
        } else if !facts.verified_backup {
            (
                ReleasePreflightStatus::Blocked,
                "release_backup_unverified",
                "create and verify the release rollback backup",
            )
        } else if facts.targets.iter().any(|target| {
            target.cargo_lock_digest != facts.observed_cargo_lock_digest
                || !target.reproducible
                || !target.sbom_present
                || !target.signature_verified
        }) {
            (
                ReleasePreflightStatus::Blocked,
                "release_target_matrix_mismatch",
                "rebuild or quarantine every target with incomplete evidence",
            )
        } else {
            (
                ReleasePreflightStatus::Ready,
                "ok",
                "publish only after the external release action supplies its receipt",
            )
        };
        let mut report = Self {
            schema: RELEASE_PREFLIGHT_SCHEMA.to_owned(),
            version: RELEASE_PREFLIGHT_VERSION,
            status: decision.0,
            publish_allowed: decision.0 == ReleasePreflightStatus::Ready,
            release_manifest_digest: facts.release_manifest_digest.clone(),
            fact_digest: facts.digest(),
            reason: decision.1.to_owned(),
            remediation: decision.2.to_owned(),
            target_count: facts.targets.len(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_PREFLIGHT_SCHEMA
            || self.version != RELEASE_PREFLIGHT_VERSION
            || self.reason.trim().is_empty()
            || self.remediation.trim().is_empty()
            || self.target_count == 0
            || self.publish_allowed != (self.status == ReleasePreflightStatus::Ready)
        {
            return Err("release_preflight_report_invalid".to_owned());
        }
        for (value, field) in [
            (&self.release_manifest_digest, "release_manifest_digest"),
            (&self.fact_digest, "release_fact_digest"),
            (&self.report_digest, "release_report_digest"),
        ] {
            validate_digest(value, field)?;
        }
        if self.report_digest != self.digest() {
            return Err("release_preflight_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "publish_allowed": self.publish_allowed,
            "release_manifest_digest": self.release_manifest_digest,
            "fact_digest": self.fact_digest,
            "reason": self.reason,
            "remediation": self.remediation,
            "target_count": self.target_count,
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
