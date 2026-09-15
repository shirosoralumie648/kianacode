//! Provider-independent evaluation contracts.
//!
//! Eval records describe normalized, replayable evidence. They are not a promotion authority and
//! cannot approve capabilities or rewrite a receipt.

use crate::{canonical_journal_bytes, json_digest, EventId, SchemaVersion, TraceStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const EVAL_CASE_SCHEMA: &str = "kiana.eval-case.v1";
pub const EVAL_RESULT_SCHEMA: &str = "kiana.eval-result.v1";
pub const EVAL_SUITE_SCHEMA: &str = "kiana.eval-suite.v1";
pub const EVAL_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EVAL_CASES: usize = 256;
pub const MAX_EVAL_FAILURES: usize = 32;
pub const MAX_EVAL_EVENT_KINDS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalVerdict {
    Pass,
    Fail,
    Blocked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalCostKind {
    NotMeasured,
    Estimated,
    Measured,
}

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCaseSpec {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: String,
    pub input_digest: String,
    pub expected_status: TraceStatus,
    pub expected_event_kinds: Vec<String>,
    pub require_audit: bool,
    pub require_metrics: bool,
    pub require_spans: bool,
    pub require_receipt: bool,
    pub forbidden_effect: bool,
    pub forbidden_secrets: bool,
    pub cost_kind: EvalCostKind,
    #[serde(default)]
    pub latency_bucket: Option<String>,
}

impl EvalCaseSpec {
    pub fn new(
        case_id: impl Into<String>,
        input_digest: impl Into<String>,
        expected_status: TraceStatus,
        expected_event_kinds: Vec<String>,
    ) -> Result<Self, String> {
        let spec = Self {
            schema: EVAL_CASE_SCHEMA.to_owned(),
            version: EVAL_SCHEMA_VERSION,
            case_id: case_id.into(),
            input_digest: input_digest.into(),
            expected_status,
            expected_event_kinds,
            require_audit: true,
            require_metrics: true,
            require_spans: true,
            require_receipt: true,
            forbidden_effect: true,
            forbidden_secrets: true,
            cost_kind: EvalCostKind::NotMeasured,
            latency_bucket: None,
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_CASE_SCHEMA || !self.version.is_compatible_with(&EVAL_SCHEMA_VERSION)
        {
            return Err("eval_case_schema_invalid".to_owned());
        }
        nonempty(&self.case_id, "eval_case_id", 128)?;
        digest(&self.input_digest, "eval_input_digest")?;
        if self.expected_event_kinds.len() > MAX_EVAL_EVENT_KINDS {
            return Err("eval_event_kind_limit".to_owned());
        }
        let mut kinds = BTreeSet::new();
        for kind in &self.expected_event_kinds {
            nonempty(kind, "eval_event_kind", 128)?;
            if !kinds.insert(kind) {
                return Err("eval_event_kind_duplicate".to_owned());
            }
        }
        if let Some(bucket) = &self.latency_bucket {
            nonempty(bucket, "eval_latency_bucket", 64)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCaseResult {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: String,
    pub verdict: EvalVerdict,
    pub observed_status: TraceStatus,
    pub normalized_event_digest: String,
    #[serde(default)]
    pub audit_digest: Option<String>,
    #[serde(default)]
    pub metric_digest: Option<String>,
    #[serde(default)]
    pub span_digest: Option<String>,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    pub effect_observed: bool,
    pub secret_detected: bool,
    pub replay_diverged: bool,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    #[serde(default)]
    pub failures: Vec<String>,
    pub cost_kind: EvalCostKind,
    #[serde(default)]
    pub latency_bucket: Option<String>,
    pub result_digest: String,
}

impl EvalCaseResult {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        verdict: EvalVerdict,
        observed_status: TraceStatus,
        normalized_event_digest: impl Into<String>,
        audit_digest: Option<String>,
        metric_digest: Option<String>,
        span_digest: Option<String>,
        receipt_digest: Option<String>,
        effect_observed: bool,
        secret_detected: bool,
        replay_diverged: bool,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        failures: Vec<String>,
        cost_kind: EvalCostKind,
        latency_bucket: Option<String>,
    ) -> Result<Self, String> {
        let mut result = Self {
            schema: EVAL_RESULT_SCHEMA.to_owned(),
            version: EVAL_SCHEMA_VERSION,
            case_id: case_id.into(),
            verdict,
            observed_status,
            normalized_event_digest: normalized_event_digest.into(),
            audit_digest,
            metric_digest,
            span_digest,
            receipt_digest,
            effect_observed,
            secret_detected,
            replay_diverged,
            source_cursor,
            source_event_ids,
            failures,
            cost_kind,
            latency_bucket,
            result_digest: String::new(),
        };
        result.result_digest = result.digest();
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_RESULT_SCHEMA
            || !self.version.is_compatible_with(&EVAL_SCHEMA_VERSION)
        {
            return Err("eval_result_schema_invalid".to_owned());
        }
        nonempty(&self.case_id, "eval_case_id", 128)?;
        digest(&self.normalized_event_digest, "eval_event_digest")?;
        for value in [
            self.audit_digest.as_deref(),
            self.metric_digest.as_deref(),
            self.span_digest.as_deref(),
            self.receipt_digest.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            digest(value, "eval_projection_digest")?;
        }
        if self.source_cursor == 0 || self.source_event_ids.is_empty() {
            return Err("eval_source_binding_invalid".to_owned());
        }
        if self.source_event_ids.len() > 256 {
            return Err("eval_source_event_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !ids.insert(event_id.to_string()) {
                return Err("eval_source_event_duplicate".to_owned());
            }
        }
        if self.failures.len() > MAX_EVAL_FAILURES {
            return Err("eval_failure_limit".to_owned());
        }
        for failure in &self.failures {
            nonempty(failure, "eval_failure", 256)?;
        }
        if self.secret_detected || self.replay_diverged {
            if self.verdict == EvalVerdict::Pass {
                return Err("eval_pass_safety_conflict".to_owned());
            }
        }
        if self.verdict == EvalVerdict::Pass && !self.failures.is_empty() {
            return Err("eval_pass_evidence_conflict".to_owned());
        }
        if let Some(bucket) = &self.latency_bucket {
            nonempty(bucket, "eval_latency_bucket", 64)?;
        }
        digest(&self.result_digest, "eval_result_digest")?;
        if self.result_digest != self.digest() {
            return Err("eval_result_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("result_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalSuiteReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub suite_id: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub cases: Vec<EvalCaseResult>,
    pub promote: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub suite_digest: String,
}

impl EvalSuiteReport {
    pub fn new(
        suite_id: impl Into<String>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        cases: Vec<EvalCaseResult>,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut report = Self {
            schema: EVAL_SUITE_SCHEMA.to_owned(),
            version: EVAL_SCHEMA_VERSION,
            suite_id: suite_id.into(),
            source_cursor,
            source_event_ids,
            promote: cases.iter().all(|case| case.verdict == EvalVerdict::Pass),
            cases,
            limitations,
            suite_digest: String::new(),
        };
        report.suite_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EVAL_SUITE_SCHEMA
            || !self.version.is_compatible_with(&EVAL_SCHEMA_VERSION)
        {
            return Err("eval_suite_schema_invalid".to_owned());
        }
        nonempty(&self.suite_id, "eval_suite_id", 128)?;
        if self.source_cursor == 0 || self.source_event_ids.is_empty() {
            return Err("eval_suite_source_binding_invalid".to_owned());
        }
        if self.cases.is_empty() || self.cases.len() > MAX_EVAL_CASES {
            return Err("eval_suite_case_limit".to_owned());
        }
        let mut case_ids = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !case_ids.insert(&case.case_id) {
                return Err("eval_suite_case_duplicate".to_owned());
            }
            if case.source_cursor > self.source_cursor {
                return Err("eval_case_cursor_ahead".to_owned());
            }
        }
        let computed_promote = self
            .cases
            .iter()
            .all(|case| case.verdict == EvalVerdict::Pass);
        if self.promote != computed_promote {
            return Err("eval_suite_promote_mismatch".to_owned());
        }
        for limitation in &self.limitations {
            nonempty(limitation, "eval_suite_limitation", 256)?;
        }
        digest(&self.suite_digest, "eval_suite_digest")?;
        if self.suite_digest != self.digest() {
            return Err("eval_suite_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert("suite_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}
