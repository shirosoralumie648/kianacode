//! EQ-48 machine-readable evaluation report artifacts.
//!
//! This module is an output contract only.  It adapts already committed evaluation evidence to
//! JSON, JUnit, a human summary, an evidence manifest and a reproduction command.  It never
//! evaluates a provider, reads a fixture, writes an artifact or changes ControlPlane authority.
//! Every textual field is checked after the shared secret redactor and absolute paths are denied
//! at the serialization boundary.

use crate::{json_digest, redact_text, scan_secret_sentinels, SecretScanChannel};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const QUALITY_REPORT_SCHEMA: &str = "kiana.quality-report.v1";
pub const QUALITY_EVIDENCE_MANIFEST_SCHEMA: &str = "kiana.quality-evidence-manifest.v1";
pub const QUALITY_REPRODUCTION_SCHEMA: &str = "kiana.quality-reproduction-command.v1";
pub const QUALITY_REPORT_MAX_CASES: usize = 4_096;
pub const QUALITY_REPORT_MAX_EVIDENCE: usize = 4_096;
pub const QUALITY_REPORT_MAX_TEXT: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityReportStatus {
    Passed,
    Failed,
    Blocked,
}

impl QualityReportStatus {
    fn from_cases(cases: &[QualityReportCase]) -> Result<Self, &'static str> {
        if cases.is_empty() {
            return Err("quality_report_cases_required");
        }
        if cases
            .iter()
            .any(|case| case.status == QualityReportStatus::Blocked)
        {
            Ok(Self::Blocked)
        } else if cases
            .iter()
            .any(|case| case.status == QualityReportStatus::Failed)
        {
            Ok(Self::Failed)
        } else {
            Ok(Self::Passed)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityReportCase {
    pub case_id: String,
    pub status: QualityReportStatus,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_message: Option<String>,
    pub evidence_refs: Vec<String>,
}

impl QualityReportCase {
    pub fn validate(&self) -> Result<(), String> {
        safe_text(&self.case_id, "quality_report_case_id", 256)?;
        if self.case_id.contains('/') || self.case_id.contains('\\') {
            return Err("quality_report_case_id_path_like".to_owned());
        }
        if self.evidence_refs.is_empty() || self.evidence_refs.len() > QUALITY_REPORT_MAX_EVIDENCE {
            return Err("quality_report_case_evidence_refs_invalid".to_owned());
        }
        let mut refs = BTreeSet::new();
        for evidence_ref in &self.evidence_refs {
            safe_ref(evidence_ref, "quality_report_case_evidence_ref")?;
            if !refs.insert(evidence_ref) {
                return Err("quality_report_case_evidence_ref_duplicate".to_owned());
            }
        }
        match self.status {
            QualityReportStatus::Passed
                if self.failure_code.is_some() || self.failure_message.is_some() =>
            {
                return Err("quality_report_pass_failure_conflict".to_owned())
            }
            QualityReportStatus::Failed | QualityReportStatus::Blocked
                if self.failure_code.is_none() || self.failure_message.is_none() =>
            {
                return Err("quality_report_failure_evidence_required".to_owned())
            }
            _ => {}
        }
        if let Some(code) = &self.failure_code {
            safe_text(code, "quality_report_failure_code", 128)?;
            if code.contains('/') || code.contains('\\') {
                return Err("quality_report_failure_code_path_like".to_owned());
            }
        }
        if let Some(message) = &self.failure_message {
            safe_text(
                message,
                "quality_report_failure_message",
                QUALITY_REPORT_MAX_TEXT,
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidenceManifestEntry {
    pub kind: String,
    pub reference: String,
    pub digest: String,
    /// A project-relative display path.  Absolute paths are never accepted in reports.
    pub path: String,
}

impl QualityEvidenceManifestEntry {
    pub fn validate(&self) -> Result<(), String> {
        safe_text(&self.kind, "quality_evidence_kind", 128)?;
        if self.kind.contains('/') || self.kind.contains('\\') {
            return Err("quality_evidence_kind_path_like".to_owned());
        }
        safe_ref(&self.reference, "quality_evidence_reference")?;
        digest(&self.digest, "quality_evidence_digest")?;
        safe_relative_path(&self.path, "quality_evidence_path")?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityEvidenceManifest {
    pub schema: String,
    pub source_cursor: u64,
    pub entries: Vec<QualityEvidenceManifestEntry>,
    pub manifest_digest: String,
}

impl QualityEvidenceManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_EVIDENCE_MANIFEST_SCHEMA
            || self.source_cursor == 0
            || self.entries.is_empty()
            || self.entries.len() > QUALITY_REPORT_MAX_EVIDENCE
        {
            return Err("quality_evidence_manifest_header_invalid".to_owned());
        }
        let mut references = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !references.insert(&entry.reference) {
                return Err("quality_evidence_reference_duplicate".to_owned());
            }
        }
        digest(&self.manifest_digest, "quality_evidence_manifest_digest")?;
        if self.manifest_digest != self.canonical_digest() {
            return Err("quality_evidence_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source_cursor": self.source_cursor,
            "entries": self.entries,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityReproductionCommand {
    pub schema: String,
    pub program: String,
    pub args: Vec<String>,
    /// Environment names are allowed; values are intentionally absent and never serialized.
    pub env_keys: Vec<String>,
    /// A safe project-relative cwd or the explicit redaction marker.
    pub cwd: String,
    pub command_digest: String,
}

impl QualityReproductionCommand {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_REPRODUCTION_SCHEMA
            || self.args.len() > 256
            || self.env_keys.len() > 128
        {
            return Err("quality_reproduction_header_invalid".to_owned());
        }
        safe_program(&self.program)?;
        for arg in &self.args {
            safe_text(arg, "quality_reproduction_arg", QUALITY_REPORT_MAX_TEXT)?;
        }
        let mut envs = BTreeSet::new();
        for key in &self.env_keys {
            if key.is_empty()
                || key.len() > 128
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !envs.insert(key)
            {
                return Err("quality_reproduction_env_key_invalid".to_owned());
            }
        }
        safe_relative_path(&self.cwd, "quality_reproduction_cwd")?;
        digest(&self.command_digest, "quality_reproduction_digest")?;
        if self.command_digest != self.canonical_digest() {
            return Err("quality_reproduction_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "program": self.program,
            "args": self.args,
            "env_keys": self.env_keys,
            "cwd": self.cwd,
        }))
    }

    /// Render a shell-safe command without exposing environment values.
    pub fn render(&self) -> Result<String, String> {
        self.validate()?;
        let mut parts = Vec::with_capacity(self.args.len() + self.env_keys.len() + 1);
        if !self.env_keys.is_empty() {
            parts.push("env".to_owned());
            for key in &self.env_keys {
                parts.push(format!("{key}=[REDACTED]"));
            }
        }
        parts.push(shell_quote(&self.program));
        parts.extend(self.args.iter().map(|arg| shell_quote(arg)));
        Ok(parts.join(" "))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityReportSummary {
    pub total: u64,
    pub passed: u64,
    pub failed: u64,
    pub blocked: u64,
    pub duration_ms: u64,
}

impl QualityReportSummary {
    fn from_cases(cases: &[QualityReportCase], duration_ms: u64) -> Self {
        Self {
            total: cases.len() as u64,
            passed: cases
                .iter()
                .filter(|case| case.status == QualityReportStatus::Passed)
                .count() as u64,
            failed: cases
                .iter()
                .filter(|case| case.status == QualityReportStatus::Failed)
                .count() as u64,
            blocked: cases
                .iter()
                .filter(|case| case.status == QualityReportStatus::Blocked)
                .count() as u64,
            duration_ms,
        }
    }

    fn validate(&self, cases: &[QualityReportCase]) -> Result<(), String> {
        let expected = Self::from_cases(cases, self.duration_ms);
        if *self != expected {
            return Err("quality_report_summary_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityReport {
    pub schema: String,
    pub suite_ref: String,
    pub status: QualityReportStatus,
    pub summary: QualityReportSummary,
    pub cases: Vec<QualityReportCase>,
    pub evidence: QualityEvidenceManifest,
    pub reproduction: QualityReproductionCommand,
    pub limitations: Vec<String>,
    pub report_digest: String,
}

impl QualityReport {
    /// Adapt the existing provider-independent suite projection without creating a second
    /// evaluator or execution loop.  The caller supplies the already redacted reproduction
    /// command; case and event evidence remain digest/reference only.
    pub fn from_eval_suite_report(
        source: &crate::EvalSuiteReport,
        reproduction: QualityReproductionCommand,
    ) -> Result<Self, String> {
        source.validate()?;
        reproduction.validate()?;
        let mut cases = Vec::with_capacity(source.cases.len());
        let mut entries = Vec::with_capacity(source.cases.len());
        for case in &source.cases {
            let (status, failure_code, failure_message) = match case.verdict {
                crate::EvalVerdict::Pass => (QualityReportStatus::Passed, None, None),
                crate::EvalVerdict::Fail => {
                    let failure = case
                        .failures
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "evaluation_failed".to_owned());
                    let code = failure
                        .split(':')
                        .next()
                        .filter(|value| !value.is_empty())
                        .unwrap_or("evaluation_failed")
                        .to_owned();
                    (
                        QualityReportStatus::Failed,
                        Some(code),
                        Some(redact_report_text(&failure)),
                    )
                }
                crate::EvalVerdict::Blocked => {
                    let failure = case
                        .failures
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "evaluation_blocked".to_owned());
                    let code = failure
                        .split(':')
                        .next()
                        .filter(|value| !value.is_empty())
                        .unwrap_or("evaluation_blocked")
                        .to_owned();
                    (
                        QualityReportStatus::Blocked,
                        Some(code),
                        Some(redact_report_text(&failure)),
                    )
                }
            };
            let evidence_ref = format!("evidence:{}", case.result_digest);
            cases.push(QualityReportCase {
                case_id: case.case_id.clone(),
                status,
                duration_ms: 0,
                failure_code,
                failure_message,
                evidence_refs: vec![evidence_ref.clone()],
            });
            entries.push(QualityEvidenceManifestEntry {
                kind: "eval_case".to_owned(),
                reference: evidence_ref,
                digest: case.result_digest.clone(),
                path: format!("evidence/{}", case.result_digest),
            });
        }
        let mut evidence = QualityEvidenceManifest {
            schema: QUALITY_EVIDENCE_MANIFEST_SCHEMA.to_owned(),
            source_cursor: source.source_cursor,
            entries,
            manifest_digest: String::new(),
        };
        evidence.manifest_digest = evidence.canonical_digest();
        let mut report = Self {
            schema: QUALITY_REPORT_SCHEMA.to_owned(),
            suite_ref: format!("suite:{}", source.suite_id),
            status: QualityReportStatus::from_cases(&cases).map_err(str::to_owned)?,
            summary: QualityReportSummary::from_cases(&cases, 0),
            cases,
            evidence,
            reproduction,
            limitations: if source.limitations.is_empty() {
                vec!["source evidence only".to_owned()]
            } else {
                source
                    .limitations
                    .iter()
                    .map(|limitation| redact_report_text(limitation))
                    .collect()
            },
            report_digest: String::new(),
        };
        report.report_digest = report.canonical_digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_REPORT_SCHEMA
            || self.cases.is_empty()
            || self.cases.len() > QUALITY_REPORT_MAX_CASES
            || self.limitations.is_empty()
        {
            return Err("quality_report_header_invalid".to_owned());
        }
        safe_ref(&self.suite_ref, "quality_report_suite_ref")?;
        let mut ids = BTreeSet::new();
        let mut evidence_refs = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !ids.insert(&case.case_id) {
                return Err("quality_report_case_duplicate".to_owned());
            }
            evidence_refs.extend(case.evidence_refs.iter().cloned());
        }
        self.summary.validate(&self.cases)?;
        if self.status != QualityReportStatus::from_cases(&self.cases).map_err(str::to_owned)? {
            return Err("quality_report_status_mismatch".to_owned());
        }
        self.evidence.validate()?;
        let manifest_refs = self
            .evidence
            .entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect::<BTreeSet<_>>();
        if evidence_refs
            .iter()
            .any(|reference| !manifest_refs.contains(reference))
        {
            return Err("quality_report_evidence_reference_missing".to_owned());
        }
        self.reproduction.validate()?;
        for limitation in &self.limitations {
            safe_text(
                limitation,
                "quality_report_limitation",
                QUALITY_REPORT_MAX_TEXT,
            )?;
        }
        digest(&self.report_digest, "quality_report_digest")?;
        if self.report_digest != self.canonical_digest() {
            return Err("quality_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "suite_ref": self.suite_ref,
            "status": self.status,
            "summary": self.summary,
            "cases": self.cases,
            "evidence": self.evidence,
            "reproduction": self.reproduction,
            "limitations": self.limitations,
        }))
    }

    pub fn render_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "quality_report_json_encode_failed".to_owned())
    }

    pub fn render_junit(&self) -> Result<String, String> {
        self.validate()?;
        let failures = self.summary.failed;
        let errors = self.summary.blocked;
        let mut output = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"0\" time=\"{}\">\n",
            xml_escape(&self.suite_ref),
            self.summary.total,
            failures,
            errors,
            self.summary.duration_ms as f64 / 1000.0
        );
        for case in &self.cases {
            output.push_str(&format!(
                "  <testcase name=\"{}\" classname=\"{}\" time=\"{}\">",
                xml_escape(&case.case_id),
                xml_escape(&self.suite_ref),
                case.duration_ms as f64 / 1000.0
            ));
            match case.status {
                QualityReportStatus::Passed => output.push_str("</testcase>\n"),
                QualityReportStatus::Failed => output.push_str(&format!(
                    "<failure type=\"{}\">{}</failure></testcase>\n",
                    xml_escape(case.failure_code.as_deref().unwrap_or("failed")),
                    xml_escape(
                        case.failure_message
                            .as_deref()
                            .unwrap_or("evaluation failed")
                    )
                )),
                QualityReportStatus::Blocked => output.push_str(&format!(
                    "<error type=\"{}\">{}</error></testcase>\n",
                    xml_escape(case.failure_code.as_deref().unwrap_or("blocked")),
                    xml_escape(
                        case.failure_message
                            .as_deref()
                            .unwrap_or("evaluation blocked")
                    )
                )),
            }
        }
        output.push_str("</testsuite>\n");
        Ok(output)
    }

    pub fn render_human_summary(&self) -> Result<String, String> {
        self.validate()?;
        let status = match self.status {
            QualityReportStatus::Passed => "passed",
            QualityReportStatus::Failed => "failed",
            QualityReportStatus::Blocked => "blocked",
        };
        let reproduction = self.reproduction.render()?;
        Ok(format!(
            "Quality report: {}\nstatus: {}\ncases: {} total, {} passed, {} failed, {} blocked\nduration_ms: {}\nevidence: {} item(s) at cursor {}\nreproduction: {}\nlimitations: {}",
            self.suite_ref,
            status,
            self.summary.total,
            self.summary.passed,
            self.summary.failed,
            self.summary.blocked,
            self.summary.duration_ms,
            self.evidence.entries.len(),
            self.evidence.source_cursor,
            reproduction,
            self.limitations.join("; ")
        ))
    }

    pub fn render_evidence_manifest(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(&self.evidence)
            .map_err(|_| "quality_evidence_manifest_encode_failed".to_owned())
    }

    pub fn render_reproduction_command(&self) -> Result<String, String> {
        self.validate()?;
        self.reproduction.render()
    }

    pub fn render_artifacts(&self) -> Result<QualityReportArtifacts, String> {
        Ok(QualityReportArtifacts {
            json_report: self.render_json()?,
            junit: self.render_junit()?,
            human_summary: self.render_human_summary()?,
            evidence_manifest: self.render_evidence_manifest()?,
            reproduction_command: self.render_reproduction_command()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityReportArtifacts {
    pub json_report: String,
    pub junit: String,
    pub human_summary: String,
    pub evidence_manifest: String,
    pub reproduction_command: String,
}

fn safe_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    let redacted = redact_report_text(value);
    if redacted != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))?;
    if contains_absolute_path(value) {
        return Err(format!("{field}_absolute_path"));
    }
    Ok(())
}

fn safe_ref(value: &str, field: &str) -> Result<(), String> {
    safe_text(value, field, 512)?;
    if value.contains("..") || value.contains(['/', '\\']) {
        return Err(format!("{field}_path_like"));
    }
    Ok(())
}

fn safe_program(value: &str) -> Result<(), String> {
    safe_text(value, "quality_reproduction_program", 256)?;
    if value.contains(['/', '\\']) || value.contains(' ') {
        return Err("quality_reproduction_program_path_like".to_owned());
    }
    Ok(())
}

fn safe_relative_path(value: &str, field: &str) -> Result<(), String> {
    safe_text(value, field, 512)?;
    if value == "." || value == "[REDACTED_PATH]" {
        return Ok(());
    }
    if contains_absolute_path(value)
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.split(['/', '\\']).any(|part| part == "..")
        || value.contains("file://")
    {
        return Err(format!("{field}_not_relative"));
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

/// Redact secret markers and path-like tokens before a report is constructed.
pub fn redact_report_text(value: &str) -> String {
    let redacted = redact_text(value);
    redacted
        .split_inclusive(char::is_whitespace)
        .map(redact_report_token)
        .collect()
}

fn redact_report_token(token: &str) -> String {
    let trimmed = token.trim_end_matches(char::is_whitespace);
    let whitespace = &token[trimmed.len()..];
    let candidate = trimmed.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let path_candidate = candidate
        .split_once('=')
        .map_or(candidate, |(_, value)| value);
    if contains_absolute_path(path_candidate) || path_candidate.starts_with("file://") {
        format!("[REDACTED_PATH]{whitespace}")
    } else {
        token.to_owned()
    }
}

fn contains_absolute_path(value: &str) -> bool {
    value.split_whitespace().any(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        let candidate = token.split_once('=').map_or(token, |(_, value)| value);
        candidate.starts_with('/')
            || candidate.starts_with('\\')
            || candidate.starts_with("file://")
            || (candidate.len() >= 3
                && candidate.as_bytes()[0].is_ascii_alphabetic()
                && candidate.as_bytes()[1] == b':'
                && matches!(candidate.as_bytes()[2], b'/' | b'\\'))
    })
}

fn shell_quote(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:/".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
