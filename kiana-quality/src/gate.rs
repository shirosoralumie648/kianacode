//! EQ-41 immutable QualityGate configuration and decision binding.
//!
//! A decision records the exact gate configuration digest used for it. Updating configuration
//! creates a new version; it cannot mutate an old decision or its digest.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const GATE_INPUT_SCHEMA: &str = "kiana.quality-gate-input.v1";
pub const GATE_EVALUATOR_ID: &str = "quality-gate";
const CONFIG_SCHEMA: &str = "kiana.quality-gate-config.v1";
const DECISION_SCHEMA: &str = "kiana.quality-gate-decision.v1";
const MAX_TEXT_BYTES: usize = 512;
const MAX_RULES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    Pass,
    Reject,
    NeedsShadow,
    Rollback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityGateConfig {
    pub schema: String,
    pub gate_id: String,
    pub version: u64,
    pub suite_digest: String,
    pub thresholds_digest: String,
    pub blocking_rules: Vec<String>,
    pub config_digest: String,
}

impl QualityGateConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONFIG_SCHEMA
            || !valid_text(&self.gate_id)
            || self.version == 0
            || !valid_digest(&self.suite_digest)
            || !valid_digest(&self.thresholds_digest)
            || self.blocking_rules.is_empty()
            || self.blocking_rules.len() > MAX_RULES
            || self
                .blocking_rules
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.blocking_rules.iter().collect::<BTreeSet<_>>().len()
                != self.blocking_rules.len()
            || self.blocking_rules.iter().any(|rule| !valid_text(rule))
            || !valid_digest(&self.config_digest)
            || self.config_digest != self.digest()
        {
            return Err("gate_config_invalid");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "gate_id": self.gate_id,
            "version": self.version,
            "suite_digest": self.suite_digest,
            "thresholds_digest": self.thresholds_digest,
            "blocking_rules": self.blocking_rules,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityGateDecision {
    pub schema: String,
    pub decision_id: String,
    pub gate_id: String,
    pub config_digest: String,
    pub candidate_digest: String,
    pub verdict: GateVerdict,
    pub blocking_findings: Vec<String>,
    pub decided_at_unix_ms: u64,
    pub decision_digest: String,
}

impl QualityGateDecision {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DECISION_SCHEMA
            || !valid_text(&self.decision_id)
            || !valid_text(&self.gate_id)
            || !valid_digest(&self.config_digest)
            || !valid_digest(&self.candidate_digest)
            || self.decided_at_unix_ms == 0
            || self.blocking_findings.len() > MAX_RULES
            || self
                .blocking_findings
                .iter()
                .any(|finding| !valid_text(finding))
            || !valid_digest(&self.decision_digest)
            || self.decision_digest != self.digest()
        {
            return Err("gate_decision_invalid");
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "decision_id": self.decision_id,
            "gate_id": self.gate_id,
            "config_digest": self.config_digest,
            "candidate_digest": self.candidate_digest,
            "verdict": self.verdict,
            "blocking_findings": self.blocking_findings,
            "decided_at_unix_ms": self.decided_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateInput {
    pub schema: String,
    pub config: QualityGateConfig,
    pub decision: QualityGateDecision,
    pub updated_config: Option<QualityGateConfig>,
}

impl GateInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_INPUT_SCHEMA {
            return Err("gate_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GateEvaluator;

impl DeterministicEvaluator for GateEvaluator {
    fn evaluator_id(&self) -> &str {
        GATE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: GateInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("gate"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_gate(&decoded)
    }
}

pub fn evaluate_gate(input: &GateInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    if input.config.validate().is_err() {
        emit(
            &mut findings,
            "gate.config_invalid",
            "immutable_valid_config",
            "invalid",
        )?;
    }
    if input.decision.validate().is_err() {
        emit(
            &mut findings,
            "gate.decision_invalid",
            "immutable_valid_decision",
            "invalid",
        )?;
    }
    if input.config.validate().is_ok() && input.decision.validate().is_ok() {
        if input.decision.gate_id != input.config.gate_id
            || input.decision.config_digest != input.config.config_digest
        {
            emit(
                &mut findings,
                "gate.decision_config_mismatch",
                "decision_binds_config",
                "mismatch",
            )?;
        }
        if input.decision.verdict == GateVerdict::Pass
            && !input.decision.blocking_findings.is_empty()
        {
            emit(
                &mut findings,
                "gate.pass_with_blocking_findings",
                "pass_has_no_blockers",
                "blocked",
            )?;
        }
    }
    if let Some(updated) = &input.updated_config {
        if updated.validate().is_err() {
            emit(
                &mut findings,
                "gate.updated_config_invalid",
                "new_valid_config_version",
                "invalid",
            )?;
        } else if input.config.validate().is_ok()
            && (updated.gate_id != input.config.gate_id
                || updated.version <= input.config.version
                || updated.config_digest == input.config.config_digest)
        {
            emit(
                &mut findings,
                "gate.config_update_mutated_old_version",
                "new_distinct_config_version",
                "invalid",
            )?;
        }
        if input.decision.validate().is_ok()
            && input.decision.config_digest != input.config.config_digest
        {
            emit(
                &mut findings,
                "gate.old_decision_not_immutable",
                "old_decision_keeps_original_config",
                "drift",
            )?;
        }
    }
    Ok(findings)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            serde_json::Value::String(expected.to_owned()),
            serde_json::Value::String(actual.to_owned()),
            format!("quality gate finding: {code}"),
            "gate:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
