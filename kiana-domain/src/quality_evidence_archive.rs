//! EQ-51 source/CI-only evaluation evidence archive contract.
//!
//! The archive is an index over already rendered EQ-48 report artifacts and EQ-21 trace-diff
//! facts.  It does not read or write a filesystem, execute an evaluator, or promote a quality
//! result.  Every archive keeps the complete `CURRENT_STATUS.md` evidence fields so a CI run can
//! be copied into the status ledger without inventing missing facts.
//!
//! The artifact schema bindings deliberately reuse the EQ-48 `QualityReport`,
//! `QualityEvidenceManifest` and `QualityReproductionCommand` outputs; EQ-21's trace-diff schema
//! is referenced by value so the domain crate does not depend on the quality crate.

use crate::{json_digest, redact_text, scan_secret_sentinels, SecretScanChannel};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const QUALITY_EVIDENCE_ARCHIVE_SCHEMA: &str = "kiana.quality-evidence-archive.v1";
pub const QUALITY_EVIDENCE_ARCHIVE_ROOT: &str = "artifacts/eq51/";
pub const QUALITY_EVIDENCE_ARCHIVE_MAX_TEXT: usize = 8 * 1024;
pub const QUALITY_EVIDENCE_ARCHIVE_TRACE_DIFF_SCHEMA: &str = "kiana.quality-trace-diff.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > QUALITY_EVIDENCE_ARCHIVE_MAX_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(field.to_owned());
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))?;
    if contains_absolute_path(value) || contains_path_traversal(value) {
        return Err(format!("{field}_path_unsafe"));
    }
    Ok(())
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

fn contains_absolute_path(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        let candidate = token.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        let candidate = candidate
            .split_once('=')
            .map_or(candidate, |(_, value)| value);
        candidate.starts_with('/')
            || candidate.starts_with('\\')
            || candidate.starts_with("file://")
            || (candidate.len() >= 3
                && candidate.as_bytes()[0].is_ascii_alphabetic()
                && candidate.as_bytes()[1] == b':'
                && matches!(candidate.as_bytes()[2], b'/' | b'\\'))
    })
}

fn contains_path_traversal(value: &str) -> bool {
    value.split(['/', '\\']).any(|part| part == "..")
}

fn safe_argv(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > QUALITY_EVIDENCE_ARCHIVE_MAX_TEXT
        || value.contains(['\0', '\r', '\n'])
        || redact_text(value) != value
        || contains_absolute_path(value)
        || contains_path_traversal(value)
    {
        return Err("quality_evidence_argv_unsafe".to_owned());
    }
    scan_secret_sentinels(SecretScanChannel::Argv, value)
        .map_err(|_| "quality_evidence_argv_secret_detected".to_owned())
}

fn safe_path(value: &str) -> Result<(), String> {
    if value.len() > 512
        || value.is_empty()
        || value.contains(['\0', '\r', '\n'])
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.starts_with("file://")
        || value
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !value.starts_with(QUALITY_EVIDENCE_ARCHIVE_ROOT)
        || redact_text(value) != value
    {
        return Err("quality_evidence_archive_path_invalid".to_owned());
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| "quality_evidence_archive_path_secret_detected".to_owned())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityArchiveFeatureStatus {
    Implemented,
    Partial,
    Deferred,
    NotSupported,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityArchiveProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Live,
    Physical,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityArchiveArtifactKind {
    Report,
    TraceDiff,
    Evidence,
    Reproduction,
}

impl QualityArchiveArtifactKind {
    pub const fn archive_name(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::TraceDiff => "trace_diff",
            Self::Evidence => "evidence",
            Self::Reproduction => "reproduction",
        }
    }

    pub const fn expected_path(self) -> &'static str {
        match self {
            Self::Report => "artifacts/eq51/report.json",
            Self::TraceDiff => "artifacts/eq51/trace-diff.json",
            Self::Evidence => "artifacts/eq51/evidence-manifest.json",
            Self::Reproduction => "artifacts/eq51/reproduction.txt",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidenceArchiveArtifact {
    pub kind: QualityArchiveArtifactKind,
    pub schema: String,
    pub reference: String,
    pub path: String,
    pub digest: String,
}

impl QualityEvidenceArchiveArtifact {
    pub fn validate(&self) -> Result<(), String> {
        if self.reference.trim().is_empty()
            || self.reference.len() > 256
            || self.reference.contains(['\0', '\r', '\n', '/', '\\'])
        {
            return Err("quality_evidence_artifact_reference_invalid".to_owned());
        }
        required(&self.reference, "quality_evidence_artifact_reference")?;
        safe_path(&self.path)?;
        if self.path != self.kind.expected_path() {
            return Err("quality_evidence_artifact_path_kind_mismatch".to_owned());
        }
        let expected_schema = match self.kind {
            QualityArchiveArtifactKind::Report => QUALITY_REPORT_SCHEMA,
            QualityArchiveArtifactKind::TraceDiff => QUALITY_EVIDENCE_ARCHIVE_TRACE_DIFF_SCHEMA,
            QualityArchiveArtifactKind::Evidence => QUALITY_EVIDENCE_MANIFEST_SCHEMA,
            QualityArchiveArtifactKind::Reproduction => QUALITY_REPRODUCTION_SCHEMA,
        };
        if self.schema != expected_schema {
            return Err("quality_evidence_artifact_schema_mismatch".to_owned());
        }
        digest(&self.digest, "quality_evidence_artifact_digest")?;
        Ok(())
    }
}

/// The fields map one-to-one to the evidence block required by `CURRENT_STATUS.md`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidenceBlock {
    pub source_snapshot: String,
    pub worktree_status: String,
    pub command_argv: Vec<String>,
    pub cwd_environment: String,
    pub fixture_cassette: String,
    pub exit_code: String,
    pub status_change: String,
    pub proof_level_change: String,
    pub limitations: Vec<String>,
    pub reviewer: String,
}

impl QualityEvidenceBlock {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.source_snapshot, "quality_evidence_source_snapshot")?;
        required(&self.worktree_status, "quality_evidence_worktree_status")?;
        if self.command_argv.is_empty() || self.command_argv.len() > 256 {
            return Err("quality_evidence_command_argv_invalid".to_owned());
        }
        for value in &self.command_argv {
            safe_argv(value)?;
        }
        required(&self.cwd_environment, "quality_evidence_cwd_environment")?;
        required(&self.fixture_cassette, "quality_evidence_fixture_cassette")?;
        if self.exit_code != "pending"
            && self.exit_code != "unobserved"
            && self.exit_code.parse::<i32>().is_err()
        {
            return Err("quality_evidence_exit_code_invalid".to_owned());
        }
        required(&self.exit_code, "quality_evidence_exit_code")?;
        required(&self.status_change, "quality_evidence_status_change")?;
        required(
            &self.proof_level_change,
            "quality_evidence_proof_level_change",
        )?;
        if self.limitations.is_empty() || self.limitations.len() > 32 {
            return Err("quality_evidence_limitations_required".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "quality_evidence_limitation")?;
        }
        required(&self.reviewer, "quality_evidence_reviewer")?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidenceArchive {
    pub schema: String,
    pub feature_status: QualityArchiveFeatureStatus,
    pub proof_level: QualityArchiveProofLevel,
    pub block: QualityEvidenceBlock,
    pub artifacts: Vec<QualityEvidenceArchiveArtifact>,
    pub archive_digest: String,
}

impl QualityEvidenceArchive {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_EVIDENCE_ARCHIVE_SCHEMA
            || self.artifacts.len() != 4
            || self.feature_status == QualityArchiveFeatureStatus::Implemented
        {
            return Err("quality_evidence_archive_header_invalid".to_owned());
        }
        if self.proof_level != QualityArchiveProofLevel::Source {
            return Err("quality_evidence_archive_proof_level_unavailable".to_owned());
        }
        self.block.validate()?;
        let mut kinds = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !kinds.insert(artifact.kind) {
                return Err("quality_evidence_artifact_kind_duplicate".to_owned());
            }
            if !paths.insert(artifact.path.clone()) {
                return Err("quality_evidence_artifact_path_duplicate".to_owned());
            }
        }
        for required_kind in [
            QualityArchiveArtifactKind::Report,
            QualityArchiveArtifactKind::TraceDiff,
            QualityArchiveArtifactKind::Evidence,
            QualityArchiveArtifactKind::Reproduction,
        ] {
            if !kinds.contains(&required_kind) {
                return Err(format!(
                    "quality_evidence_artifact_missing_{}",
                    required_kind.archive_name()
                ));
            }
        }
        digest(&self.archive_digest, "quality_evidence_archive_digest")?;
        if self.archive_digest != self.canonical_digest() {
            return Err("quality_evidence_archive_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "feature_status": self.feature_status,
            "proof_level": self.proof_level,
            "block": self.block,
            "artifacts": self.artifacts,
        }))
    }

    /// A stable JSON representation suitable for a CI artifact; this method never writes it.
    pub fn render_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "quality_evidence_archive_encode_failed".to_owned())
    }
}
