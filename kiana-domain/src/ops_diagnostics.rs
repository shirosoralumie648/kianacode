//! Read-only operator status, doctor and preflight diagnostics.
//!
//! The reducer consumes server-owned, already redacted facts from the configuration, startup,
//! health and observability projections. It never repairs a fact, opens a store, probes a
//! provider or changes admission. Missing and unknown evidence stay visible in the result and
//! map to non-zero CI exit codes.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, QualityReproductionCommand, SchemaVersion,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const OPS_DIAGNOSTICS_INPUT_SCHEMA: &str = "kiana.ops-diagnostics-input.v1";
pub const OPS_DIAGNOSTIC_FACT_SCHEMA: &str = "kiana.ops-diagnostic-fact.v1";
pub const OPS_DIAGNOSTICS_REPORT_SCHEMA: &str = "kiana.ops-diagnostics-report.v1";
pub const OPS_DIAGNOSTICS_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const OPS_DIAGNOSTICS_MAX_FACTS: usize = 8;
pub const OPS_DIAGNOSTICS_MAX_LIMITATIONS: usize = 16;
pub const OPS_DIAGNOSTICS_MAX_TEXT: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsDiagnosticsMode {
    Status,
    Doctor,
    Preflight,
}

impl OpsDiagnosticsMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Doctor => "doctor",
            Self::Preflight => "preflight",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsDiagnosticAxis {
    Config,
    Startup,
    Health,
    Observability,
}

impl OpsDiagnosticAxis {
    pub const ALL: [Self; 4] = [
        Self::Config,
        Self::Startup,
        Self::Health,
        Self::Observability,
    ];

    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Config => 1,
            Self::Startup => 2,
            Self::Health => 3,
            Self::Observability => 4,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Startup => "startup",
            Self::Health => "health",
            Self::Observability => "observability",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsAxisStatus {
    Ready,
    Degraded,
    Blocked,
    Unknown,
    Missing,
}

impl OpsAxisStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
            Self::Missing => "missing",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsDiagnosticsStatus {
    Ready,
    Degraded,
    Blocked,
    Unknown,
}

impl OpsDiagnosticsStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    pub const fn exit_code(self) -> i32 {
        match self {
            Self::Ready => 0,
            Self::Degraded => 10,
            Self::Blocked => 20,
            Self::Unknown => 30,
        }
    }

    pub const fn is_healthy(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsDiagnosticFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub axis: OpsDiagnosticAxis,
    pub status: OpsAxisStatus,
    pub source_cursor: u64,
    pub evidence_digest: Option<String>,
    pub reason: String,
    pub remediation: String,
    pub fact_digest: String,
}

impl OpsDiagnosticFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        axis: OpsDiagnosticAxis,
        status: OpsAxisStatus,
        source_cursor: u64,
        evidence_digest: Option<String>,
        reason: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: OPS_DIAGNOSTIC_FACT_SCHEMA.to_owned(),
            version: OPS_DIAGNOSTICS_VERSION,
            axis,
            status,
            source_cursor,
            evidence_digest,
            reason: reason.into(),
            remediation: remediation.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_DIAGNOSTIC_FACT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.source_cursor == 0
        {
            return Err("ops_diagnostic_fact_header_invalid".to_owned());
        }
        if let Some(digest) = &self.evidence_digest {
            valid_digest(digest, "ops_diagnostic_evidence_digest")?;
        }
        safe_text(&self.reason, "ops_diagnostic_reason")?;
        safe_text(&self.remediation, "ops_diagnostic_remediation")?;
        match self.status {
            OpsAxisStatus::Ready => {
                if self.evidence_digest.is_none()
                    || self.reason != "ok"
                    || self.remediation != "none"
                {
                    return Err("ops_diagnostic_ready_evidence_invalid".to_owned());
                }
            }
            OpsAxisStatus::Degraded | OpsAxisStatus::Blocked | OpsAxisStatus::Unknown => {
                if self.reason == "ok" || self.remediation == "none" {
                    return Err("ops_diagnostic_non_ready_explanation_missing".to_owned());
                }
            }
            OpsAxisStatus::Missing => {
                if self.evidence_digest.is_some()
                    || self.reason == "ok"
                    || self.remediation == "none"
                {
                    return Err("ops_diagnostic_missing_evidence_invalid".to_owned());
                }
            }
        }
        valid_digest(&self.fact_digest, "ops_diagnostic_fact_digest")?;
        if self.fact_digest != self.digest() {
            return Err("ops_diagnostic_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "axis": self.axis,
            "status": self.status,
            "source_cursor": self.source_cursor,
            "evidence_digest": self.evidence_digest,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsDiagnosticsInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub mode: OpsDiagnosticsMode,
    pub source_cursor: u64,
    pub observed_at_ms: u64,
    pub facts: Vec<OpsDiagnosticFact>,
    pub limitations: Vec<String>,
    pub input_digest: String,
}

impl OpsDiagnosticsInput {
    pub fn new(
        mode: OpsDiagnosticsMode,
        source_cursor: u64,
        observed_at_ms: u64,
        facts: Vec<OpsDiagnosticFact>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut input = Self {
            schema: OPS_DIAGNOSTICS_INPUT_SCHEMA.to_owned(),
            version: OPS_DIAGNOSTICS_VERSION,
            mode,
            source_cursor,
            observed_at_ms,
            facts,
            limitations,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OPS_DIAGNOSTICS_INPUT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.source_cursor == 0
            || self.observed_at_ms == 0
            || self.facts.len() != OpsDiagnosticAxis::ALL.len()
            || self.facts.len() > OPS_DIAGNOSTICS_MAX_FACTS
            || self.limitations.len() > OPS_DIAGNOSTICS_MAX_LIMITATIONS
        {
            return Err("ops_diagnostics_input_header_invalid".to_owned());
        }
        let mut axes = BTreeSet::new();
        let mut expected_ordinal = 1;
        for fact in &self.facts {
            fact.validate()?;
            if fact.source_cursor != self.source_cursor
                || fact.axis.ordinal() != expected_ordinal
                || !axes.insert(fact.axis)
            {
                return Err("ops_diagnostics_fact_order_or_cursor_invalid".to_owned());
            }
            expected_ordinal += 1;
        }
        if axes.len() != OpsDiagnosticAxis::ALL.len() {
            return Err("ops_diagnostics_axis_set_invalid".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "ops_diagnostics_limitation")?;
        }
        valid_digest(&self.input_digest, "ops_diagnostics_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("ops_diagnostics_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "mode": self.mode,
            "source_cursor": self.source_cursor,
            "observed_at_ms": self.observed_at_ms,
            "facts": self.facts,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsDiagnosticsReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub mode: OpsDiagnosticsMode,
    pub status: OpsDiagnosticsStatus,
    pub healthy: bool,
    pub exit_code: i32,
    pub source_cursor: u64,
    pub diagnostics: Vec<OpsDiagnosticFact>,
    pub limitations: Vec<String>,
    pub reproduction: QualityReproductionCommand,
    pub report_digest: String,
}

impl OpsDiagnosticsReport {
    pub fn evaluate(input: &OpsDiagnosticsInput) -> Result<Self, String> {
        input.validate()?;
        let status = derive_status(&input.facts);
        let mut limitations = input.limitations.clone();
        limitations.push("read_only_diagnostics_no_fact_mutation".to_owned());
        limitations.push("adapter_supplied_evidence_source_only".to_owned());
        let reproduction = reproduction_command(input.mode)?;
        let mut report = Self {
            schema: OPS_DIAGNOSTICS_REPORT_SCHEMA.to_owned(),
            version: OPS_DIAGNOSTICS_VERSION,
            mode: input.mode,
            status,
            healthy: status.is_healthy(),
            exit_code: status.exit_code(),
            source_cursor: input.source_cursor,
            diagnostics: input.facts.clone(),
            limitations,
            reproduction,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(input)?;
        Ok(report)
    }

    pub fn validate_against(&self, input: &OpsDiagnosticsInput) -> Result<(), String> {
        input.validate()?;
        let expected_reproduction = reproduction_command(self.mode)?;
        if self.schema != OPS_DIAGNOSTICS_REPORT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.mode != input.mode
            || self.source_cursor != input.source_cursor
            || self.diagnostics != input.facts
            || self.status != derive_status(&input.facts)
            || self.healthy != self.status.is_healthy()
            || self.exit_code != self.status.exit_code()
            || self.limitations.len() != input.limitations.len() + 2
            || self.reproduction != expected_reproduction
        {
            return Err("ops_diagnostics_report_binding_invalid".to_owned());
        }
        for limitation in &self.limitations {
            safe_text(limitation, "ops_diagnostics_report_limitation")?;
        }
        if self.report_digest != self.digest() {
            return Err("ops_diagnostics_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "mode": self.mode,
            "status": self.status,
            "healthy": self.healthy,
            "exit_code": self.exit_code,
            "source_cursor": self.source_cursor,
            "diagnostics": self.diagnostics,
            "limitations": self.limitations,
            "reproduction": self.reproduction,
        }))
    }

    pub fn render_json(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        serde_json::to_string_pretty(self)
            .map_err(|_| "ops_diagnostics_json_encode_failed".to_owned())
    }

    pub fn render_human(&self) -> Result<String, String> {
        self.validate_digest_only()?;
        let mut lines = vec![
            format!("mode: {}", self.mode.as_str()),
            format!("status: {}", self.status.as_str()),
            format!("healthy: {}", self.healthy),
            format!("exit_code: {}", self.exit_code),
            format!("source_cursor: {}", self.source_cursor),
        ];
        for fact in &self.diagnostics {
            lines.push(format!(
                "{}: {} ({}) remediation={}",
                fact.axis.as_str(),
                fact.status.as_str(),
                fact.reason,
                fact.remediation
            ));
        }
        lines.push(format!(
            "reproduction: {}",
            self.reproduction
                .render()
                .map_err(|_| "ops_diagnostics_reproduction_render_failed".to_owned())?
        ));
        lines.push(format!("limitations: {}", self.limitations.join("; ")));
        Ok(lines.join("\n"))
    }

    fn validate_digest_only(&self) -> Result<(), String> {
        let expected_reproduction = reproduction_command(self.mode)?;
        if self.schema != OPS_DIAGNOSTICS_REPORT_SCHEMA
            || self.version != OPS_DIAGNOSTICS_VERSION
            || self.diagnostics.len() != OpsDiagnosticAxis::ALL.len()
            || self.status != derive_status(&self.diagnostics)
            || self.healthy != self.status.is_healthy()
            || self.exit_code != self.status.exit_code()
            || self.reproduction != expected_reproduction
        {
            return Err("ops_diagnostics_report_shape_invalid".to_owned());
        }
        valid_digest(&self.report_digest, "ops_diagnostics_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("ops_diagnostics_report_digest_mismatch".to_owned());
        }
        for fact in &self.diagnostics {
            fact.validate()?;
        }
        for limitation in &self.limitations {
            safe_text(limitation, "ops_diagnostics_report_limitation")?;
        }
        self.reproduction
            .validate()
            .map_err(|_| "ops_diagnostics_reproduction_invalid".to_owned())
    }
}

fn derive_status(facts: &[OpsDiagnosticFact]) -> OpsDiagnosticsStatus {
    if facts
        .iter()
        .any(|fact| fact.status == OpsAxisStatus::Unknown)
    {
        OpsDiagnosticsStatus::Unknown
    } else if facts
        .iter()
        .any(|fact| matches!(fact.status, OpsAxisStatus::Blocked | OpsAxisStatus::Missing))
    {
        OpsDiagnosticsStatus::Blocked
    } else if facts
        .iter()
        .any(|fact| fact.status == OpsAxisStatus::Degraded)
    {
        OpsDiagnosticsStatus::Degraded
    } else {
        OpsDiagnosticsStatus::Ready
    }
}

fn reproduction_command(mode: OpsDiagnosticsMode) -> Result<QualityReproductionCommand, String> {
    let mut command = QualityReproductionCommand {
        schema: crate::QUALITY_REPRODUCTION_SCHEMA.to_owned(),
        program: "kiana".to_owned(),
        args: vec![
            "ops".to_owned(),
            mode.as_str().to_owned(),
            "--json".to_owned(),
        ],
        env_keys: Vec::new(),
        cwd: "[REDACTED_PATH]".to_owned(),
        command_digest: String::new(),
    };
    command.command_digest = command.canonical_digest();
    command.validate()?;
    Ok(command)
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > OPS_DIAGNOSTICS_MAX_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("file://")
        || value.split_whitespace().any(|token| {
            token.starts_with('/')
                || token.starts_with('\\')
                || (token.len() >= 3
                    && token.as_bytes()[0].is_ascii_alphabetic()
                    && token.as_bytes()[1] == b':'
                    && matches!(token.as_bytes()[2], b'/' | b'\\'))
        })
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
