//! H35 Harness evaluation bindings.
//!
//! These are immutable, provider-independent evaluation records. They bind a GoldenTrace to
//! the exact source/model/prompt/tool/workspace inputs, describe an offline replay report, and
//! keep safety and result-integrity regressions stronger than performance improvements. They do
//! not execute a model, invoke a Broker, or become a promotion authority.

use crate::{
    canonical_journal_bytes, json_digest, redact_text, EvalVerdict, GoldenTraceId, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const HARNESS_TRACE_BINDING_SCHEMA: &str = "kiana.harness-trace-binding.v1";
pub const HARNESS_REPLAY_REPORT_SCHEMA: &str = "kiana.harness-replay-report.v1";
pub const HARNESS_METRIC_SET_SCHEMA: &str = "kiana.harness-metric-set.v1";
pub const HARNESS_EVAL_COMPARISON_SCHEMA: &str = "kiana.harness-eval-comparison.v1";
pub const HARNESS_EVAL_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_HARNESS_EVAL_OBSERVATIONS: usize = 32;
pub const MAX_HARNESS_EVAL_LIMITATIONS: usize = 16;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.contains('\0')
        || redact_text(value) != value
    {
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

fn optional_digest(value: Option<&str>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        digest(value, field)?;
    }
    Ok(())
}

fn limitations(values: &[String]) -> Result<(), String> {
    if values.len() > MAX_HARNESS_EVAL_LIMITATIONS {
        return Err("harness_eval_limitation_limit".to_owned());
    }
    for value in values {
        required(value, "harness_eval_limitation", 256)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessReplayMode {
    OfflineNoEffects,
    LiveProviderNotEvaluated,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessTraceBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: GoldenTraceId,
    pub trace_digest: String,
    pub source_snapshot: String,
    pub model_version: String,
    pub prompt_version: String,
    pub tool_directory_hash: String,
    pub input_hash: String,
    pub workspace_hash: String,
    pub runtime_version: String,
    pub source_cursor: u64,
    #[serde(default)]
    pub receipt_hash: Option<String>,
    pub replay_mode: HarnessReplayMode,
    pub binding_digest: String,
}

impl HarnessTraceBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: GoldenTraceId,
        trace_digest: impl Into<String>,
        source_snapshot: impl Into<String>,
        model_version: impl Into<String>,
        prompt_version: impl Into<String>,
        tool_directory_hash: impl Into<String>,
        input_hash: impl Into<String>,
        workspace_hash: impl Into<String>,
        runtime_version: impl Into<String>,
        source_cursor: u64,
        receipt_hash: Option<String>,
        replay_mode: HarnessReplayMode,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: HARNESS_TRACE_BINDING_SCHEMA.to_owned(),
            version: HARNESS_EVAL_SCHEMA_VERSION,
            trace_id,
            trace_digest: trace_digest.into(),
            source_snapshot: source_snapshot.into(),
            model_version: model_version.into(),
            prompt_version: prompt_version.into(),
            tool_directory_hash: tool_directory_hash.into(),
            input_hash: input_hash.into(),
            workspace_hash: workspace_hash.into(),
            runtime_version: runtime_version.into(),
            source_cursor,
            receipt_hash,
            replay_mode,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HARNESS_TRACE_BINDING_SCHEMA
            || !self
                .version
                .is_compatible_with(&HARNESS_EVAL_SCHEMA_VERSION)
            || self.trace_id.as_uuid().is_nil()
            || self.source_cursor == 0
        {
            return Err("harness_trace_binding_header_invalid".to_owned());
        }
        digest(&self.trace_digest, "harness_trace_digest")?;
        digest(&self.tool_directory_hash, "harness_tool_directory_hash")?;
        digest(&self.input_hash, "harness_input_hash")?;
        digest(&self.workspace_hash, "harness_workspace_hash")?;
        optional_digest(self.receipt_hash.as_deref(), "harness_receipt_hash")?;
        for (value, field, max) in [
            (&self.source_snapshot, "harness_source_snapshot", 256),
            (&self.model_version, "harness_model_version", 128),
            (&self.prompt_version, "harness_prompt_version", 128),
            (&self.runtime_version, "harness_runtime_version", 128),
        ] {
            required(value, field, max)?;
        }
        digest(&self.binding_digest, "harness_binding_digest")?;
        if self.binding_digest != self.digest() {
            return Err("harness_trace_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "trace_id": self.trace_id,
            "trace_digest": self.trace_digest,
            "source_snapshot": self.source_snapshot,
            "model_version": self.model_version,
            "prompt_version": self.prompt_version,
            "tool_directory_hash": self.tool_directory_hash,
            "input_hash": self.input_hash,
            "workspace_hash": self.workspace_hash,
            "runtime_version": self.runtime_version,
            "source_cursor": self.source_cursor,
            "receipt_hash": self.receipt_hash,
            "replay_mode": self.replay_mode,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessReplayDifferenceKind {
    MissingResult,
    ExtraSideEffect,
    EventMismatch,
    ReceiptMismatch,
    ProtocolMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessReplayDifference {
    pub index: u64,
    pub kind: HarnessReplayDifferenceKind,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub observed: Option<String>,
    pub reason: String,
}

impl HarnessReplayDifference {
    pub fn validate(&self) -> Result<(), String> {
        if self.index == 0 {
            return Err("harness_replay_difference_index_invalid".to_owned());
        }
        for value in [self.expected.as_deref(), self.observed.as_deref()]
            .into_iter()
            .flatten()
        {
            required(value, "harness_replay_difference_value", 512)?;
        }
        required(&self.reason, "harness_replay_difference_reason", 256)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessReplayReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub binding_digest: String,
    pub replay_mode: HarnessReplayMode,
    pub source_cursor: u64,
    pub normalized_event_digest: String,
    pub side_effect_count: u64,
    pub network_call_count: u64,
    pub process_call_count: u64,
    pub missing_result: bool,
    pub extra_side_effect: bool,
    #[serde(default)]
    pub first_difference: Option<HarnessReplayDifference>,
    pub verdict: EvalVerdict,
    pub report_digest: String,
}

impl HarnessReplayReport {
    pub fn new(
        binding: &HarnessTraceBinding,
        source_cursor: u64,
        normalized_event_digest: impl Into<String>,
        side_effect_count: u64,
        network_call_count: u64,
        process_call_count: u64,
        missing_result: bool,
        extra_side_effect: bool,
        first_difference: Option<HarnessReplayDifference>,
    ) -> Result<Self, String> {
        binding.validate()?;
        if source_cursor == 0 {
            return Err("harness_replay_source_cursor_invalid".to_owned());
        }
        if missing_result || extra_side_effect {
            first_difference
                .as_ref()
                .ok_or_else(|| "harness_replay_difference_required".to_owned())?
                .validate()?;
        }
        let normalized_event_digest = normalized_event_digest.into();
        digest(&normalized_event_digest, "harness_normalized_event_digest")?;
        let verdict = replay_verdict(
            binding.replay_mode,
            side_effect_count,
            network_call_count,
            process_call_count,
            missing_result,
            extra_side_effect,
            first_difference.is_some(),
        );
        let mut report = Self {
            schema: HARNESS_REPLAY_REPORT_SCHEMA.to_owned(),
            version: HARNESS_EVAL_SCHEMA_VERSION,
            binding_digest: binding.binding_digest.clone(),
            replay_mode: binding.replay_mode,
            source_cursor,
            normalized_event_digest,
            side_effect_count,
            network_call_count,
            process_call_count,
            missing_result,
            extra_side_effect,
            first_difference,
            verdict,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HARNESS_REPLAY_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&HARNESS_EVAL_SCHEMA_VERSION)
            || self.source_cursor == 0
        {
            return Err("harness_replay_report_header_invalid".to_owned());
        }
        digest(&self.binding_digest, "harness_binding_digest")?;
        digest(
            &self.normalized_event_digest,
            "harness_normalized_event_digest",
        )?;
        if let Some(difference) = &self.first_difference {
            difference.validate()?;
        }
        if (self.missing_result || self.extra_side_effect) && self.first_difference.is_none() {
            return Err("harness_replay_difference_required".to_owned());
        }
        let expected = replay_verdict(
            self.replay_mode,
            self.side_effect_count,
            self.network_call_count,
            self.process_call_count,
            self.missing_result,
            self.extra_side_effect,
            self.first_difference.is_some(),
        );
        if self.verdict != expected {
            return Err("harness_replay_verdict_mismatch".to_owned());
        }
        digest(&self.report_digest, "harness_replay_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("harness_replay_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "binding_digest": self.binding_digest,
            "replay_mode": self.replay_mode,
            "source_cursor": self.source_cursor,
            "normalized_event_digest": self.normalized_event_digest,
            "side_effect_count": self.side_effect_count,
            "network_call_count": self.network_call_count,
            "process_call_count": self.process_call_count,
            "missing_result": self.missing_result,
            "extra_side_effect": self.extra_side_effect,
            "first_difference": self.first_difference,
            "verdict": self.verdict,
        }))
    }
}

fn replay_verdict(
    mode: HarnessReplayMode,
    side_effect_count: u64,
    network_call_count: u64,
    process_call_count: u64,
    missing_result: bool,
    extra_side_effect: bool,
    has_difference: bool,
) -> EvalVerdict {
    if mode != HarnessReplayMode::OfflineNoEffects
        || side_effect_count > 0
        || network_call_count > 0
        || process_call_count > 0
        || extra_side_effect
    {
        EvalVerdict::Blocked
    } else if missing_result || has_difference {
        EvalVerdict::Fail
    } else {
        EvalVerdict::Pass
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessEvalMetric {
    TaskCompletion,
    ProtocolIntegrity,
    DuplicateSideEffect,
    PermissionDenial,
    CompactionSemanticPreservation,
    NoProgressLoop,
    CancellationLatency,
    PeakMemory,
    PeakOutput,
    RequestCost,
    UsageCompleteness,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessEvalMetricStatus {
    Pass,
    Fail,
    Blocked,
    NotMeasured,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessEvalMetricObservation {
    pub metric: HarnessEvalMetric,
    pub status: HarnessEvalMetricStatus,
    pub observed: u64,
    #[serde(default)]
    pub budget: Option<u64>,
    pub sample_count: u32,
    pub usage_complete: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

impl HarnessEvalMetricObservation {
    pub fn new(
        metric: HarnessEvalMetric,
        status: HarnessEvalMetricStatus,
        observed: u64,
        budget: Option<u64>,
        sample_count: u32,
        usage_complete: bool,
        reason: Option<String>,
    ) -> Result<Self, String> {
        let observation = Self {
            metric,
            status,
            observed,
            budget,
            sample_count,
            usage_complete,
            reason,
        };
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.sample_count == 0
            || self.budget.is_some_and(|budget| budget == 0)
            || (self.status == HarnessEvalMetricStatus::Pass
                && self.budget.is_some_and(|budget| self.observed > budget))
            || (matches!(
                self.status,
                HarnessEvalMetricStatus::Fail
                    | HarnessEvalMetricStatus::Blocked
                    | HarnessEvalMetricStatus::NotMeasured
            ) && self.reason.as_deref().is_none_or(str::is_empty))
            || ((self.metric == HarnessEvalMetric::RequestCost
                || self.metric == HarnessEvalMetric::UsageCompleteness)
                && self.status == HarnessEvalMetricStatus::Pass
                && !self.usage_complete)
        {
            return Err("harness_eval_metric_invalid".to_owned());
        }
        if let Some(reason) = &self.reason {
            required(reason, "harness_eval_metric_reason", 256)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessEvalMetricSet {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_binding_digest: String,
    pub case_id: String,
    pub observations: Vec<HarnessEvalMetricObservation>,
    pub usage_complete: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub promote: bool,
    pub metric_set_digest: String,
}

impl HarnessEvalMetricSet {
    pub fn new(
        trace_binding_digest: impl Into<String>,
        case_id: impl Into<String>,
        observations: Vec<HarnessEvalMetricObservation>,
        usage_complete: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let promote = usage_complete
            && !observations.is_empty()
            && observations
                .iter()
                .all(|observation| observation.status == HarnessEvalMetricStatus::Pass);
        let mut set = Self {
            schema: HARNESS_METRIC_SET_SCHEMA.to_owned(),
            version: HARNESS_EVAL_SCHEMA_VERSION,
            trace_binding_digest: trace_binding_digest.into(),
            case_id: case_id.into(),
            observations,
            usage_complete,
            limitations,
            promote,
            metric_set_digest: String::new(),
        };
        set.metric_set_digest = set.digest();
        set.validate()?;
        Ok(set)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HARNESS_METRIC_SET_SCHEMA
            || !self
                .version
                .is_compatible_with(&HARNESS_EVAL_SCHEMA_VERSION)
            || self.observations.is_empty()
            || self.observations.len() > MAX_HARNESS_EVAL_OBSERVATIONS
        {
            return Err("harness_eval_metric_set_header_invalid".to_owned());
        }
        digest(&self.trace_binding_digest, "harness_trace_binding_digest")?;
        required(&self.case_id, "harness_eval_case_id", 128)?;
        let mut metrics = BTreeSet::new();
        for observation in &self.observations {
            observation.validate()?;
            if !metrics.insert(observation.metric) {
                return Err("harness_eval_metric_duplicate".to_owned());
            }
        }
        limitations(&self.limitations)?;
        if !self.usage_complete
            && !self
                .limitations
                .iter()
                .any(|value| value == "usage_incomplete")
        {
            return Err("harness_eval_usage_limitation_required".to_owned());
        }
        let promote = self.usage_complete
            && self
                .observations
                .iter()
                .all(|observation| observation.status == HarnessEvalMetricStatus::Pass);
        if self.promote != promote {
            return Err("harness_eval_metric_promote_mismatch".to_owned());
        }
        digest(&self.metric_set_digest, "harness_metric_set_digest")?;
        if self.metric_set_digest != self.digest() {
            return Err("harness_eval_metric_set_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "trace_binding_digest": self.trace_binding_digest,
            "case_id": self.case_id,
            "observations": self.observations,
            "usage_complete": self.usage_complete,
            "limitations": self.limitations,
            "promote": self.promote,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialOrd, Ord, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessExecutionMode {
    Serial,
    Parallel,
}

#[derive(Clone, Copy, Debug, Eq, PartialOrd, Ord, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessContextStrategy {
    OldTruncation,
    NewSummary,
}

#[derive(Clone, Copy, Debug, Eq, PartialOrd, Ord, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessVerificationStrategy {
    Baseline,
    Extra,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessEvalVariant {
    pub execution: HarnessExecutionMode,
    pub context: HarnessContextStrategy,
    pub verification: HarnessVerificationStrategy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessEvalComparison {
    pub schema: String,
    pub version: SchemaVersion,
    pub task_set_digest: String,
    pub baseline_variant: HarnessEvalVariant,
    pub candidate_variant: HarnessEvalVariant,
    pub baseline_report_digest: String,
    pub candidate_report_digest: String,
    pub baseline_verdict: EvalVerdict,
    pub candidate_verdict: EvalVerdict,
    pub security_regression: bool,
    pub result_integrity_regression: bool,
    pub performance_improved: bool,
    pub promote: bool,
    #[serde(default)]
    pub reason: Option<String>,
    pub comparison_digest: String,
}

impl HarnessEvalComparison {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_set_digest: impl Into<String>,
        baseline_variant: HarnessEvalVariant,
        candidate_variant: HarnessEvalVariant,
        baseline_report_digest: impl Into<String>,
        candidate_report_digest: impl Into<String>,
        baseline_verdict: EvalVerdict,
        candidate_verdict: EvalVerdict,
        security_regression: bool,
        result_integrity_regression: bool,
        performance_improved: bool,
        reason: Option<String>,
    ) -> Result<Self, String> {
        let promote = candidate_verdict == EvalVerdict::Pass
            && !security_regression
            && !result_integrity_regression;
        let mut comparison = Self {
            schema: HARNESS_EVAL_COMPARISON_SCHEMA.to_owned(),
            version: HARNESS_EVAL_SCHEMA_VERSION,
            task_set_digest: task_set_digest.into(),
            baseline_variant,
            candidate_variant,
            baseline_report_digest: baseline_report_digest.into(),
            candidate_report_digest: candidate_report_digest.into(),
            baseline_verdict,
            candidate_verdict,
            security_regression,
            result_integrity_regression,
            performance_improved,
            promote,
            reason,
            comparison_digest: String::new(),
        };
        comparison.comparison_digest = comparison.digest();
        comparison.validate()?;
        Ok(comparison)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HARNESS_EVAL_COMPARISON_SCHEMA
            || !self
                .version
                .is_compatible_with(&HARNESS_EVAL_SCHEMA_VERSION)
            || self.baseline_variant == self.candidate_variant
        {
            return Err("harness_eval_comparison_header_invalid".to_owned());
        }
        digest(&self.task_set_digest, "harness_task_set_digest")?;
        digest(
            &self.baseline_report_digest,
            "harness_baseline_report_digest",
        )?;
        digest(
            &self.candidate_report_digest,
            "harness_candidate_report_digest",
        )?;
        let promote = self.candidate_verdict == EvalVerdict::Pass
            && !self.security_regression
            && !self.result_integrity_regression;
        if self.promote != promote {
            return Err("harness_eval_comparison_promote_mismatch".to_owned());
        }
        if !self.promote && self.reason.as_deref().is_none_or(str::is_empty) {
            return Err("harness_eval_comparison_reason_required".to_owned());
        }
        if let Some(reason) = &self.reason {
            required(reason, "harness_eval_comparison_reason", 512)?;
        }
        digest(&self.comparison_digest, "harness_comparison_digest")?;
        if self.comparison_digest != self.digest() {
            return Err("harness_eval_comparison_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "task_set_digest": self.task_set_digest,
            "baseline_variant": self.baseline_variant,
            "candidate_variant": self.candidate_variant,
            "baseline_report_digest": self.baseline_report_digest,
            "candidate_report_digest": self.candidate_report_digest,
            "baseline_verdict": self.baseline_verdict,
            "candidate_verdict": self.candidate_verdict,
            "security_regression": self.security_regression,
            "result_integrity_regression": self.result_integrity_regression,
            "performance_improved": self.performance_improved,
            "promote": self.promote,
            "reason": self.reason,
        }))
    }
}
