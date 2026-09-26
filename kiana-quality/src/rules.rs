//! EQ-42 blocking-rule precedence over weighted quality scores.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BLOCKING_RULE_INPUT_SCHEMA: &str = "kiana.quality-blocking-rule-input.v1";
pub const BLOCKING_RULE_EVALUATOR_ID: &str = "blocking-rules";
const RULE_SCHEMA: &str = "kiana.quality-blocking-rule.v1";
const REQUIRED_RULES: &[&str] = &[
    "safety",
    "evidence",
    "replay",
    "forbidden_effect",
    "fixture_integrity",
    "infra",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingVerdict {
    Pass,
    Reject,
    NeedsReview,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockingRuleEvidence {
    pub schema: String,
    pub rule: String,
    pub triggered: bool,
    pub finding_refs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockingRuleInput {
    pub schema: String,
    pub rules: Vec<BlockingRuleEvidence>,
    pub weighted_score_milli: u64,
    pub score_threshold_milli: u64,
    pub verdict: BlockingVerdict,
}

impl BlockingRuleInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BLOCKING_RULE_INPUT_SCHEMA {
            return Err("blocking_rule_input_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BlockingRuleEvaluator;

impl DeterministicEvaluator for BlockingRuleEvaluator {
    fn evaluator_id(&self) -> &str {
        BLOCKING_RULE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: BlockingRuleInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("blocking_rules"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_blocking_rules(&decoded)
    }
}

pub fn evaluate_blocking_rules(input: &BlockingRuleInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let mut rules = BTreeSet::<String>::new();
    for evidence in &input.rules {
        if evidence.schema != RULE_SCHEMA {
            emit(
                &mut findings,
                "blocking.rule_schema_invalid",
                "typed_rule_schema",
                "invalid",
            )?;
        }
        if !REQUIRED_RULES.contains(&evidence.rule.as_str()) || !rules.insert(evidence.rule.clone())
        {
            emit(
                &mut findings,
                "blocking.rule_identity_invalid",
                "unique_required_rule",
                "invalid_or_duplicate",
            )?;
        }
        if evidence.triggered && evidence.finding_refs.is_empty() {
            emit(
                &mut findings,
                "blocking.finding_ref_missing",
                "triggered_rule_has_finding",
                "missing",
            )?;
        }
    }
    for required in REQUIRED_RULES {
        if !rules.contains(*required) {
            emit(
                &mut findings,
                "blocking.rule_missing",
                "complete_blocking_rule_matrix",
                "missing",
            )?;
        }
    }
    let triggered = input.rules.iter().any(|rule| rule.triggered);
    if triggered && input.verdict == BlockingVerdict::Pass {
        emit(
            &mut findings,
            "blocking.rule_precedes_score",
            "blocking_rule_rejects_pass",
            "pass",
        )?;
    }
    if !triggered
        && input.weighted_score_milli < input.score_threshold_milli
        && input.verdict == BlockingVerdict::Pass
    {
        emit(
            &mut findings,
            "blocking.score_threshold_invalid",
            "pass_meets_weighted_threshold",
            "below",
        )?;
    }
    Ok(findings)
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
            format!("blocking rule finding: {code}"),
            "blocking:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
