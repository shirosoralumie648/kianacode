//! EQ-42 blocking-rule precedence over weighted quality scores.
//!
//! Two surfaces live here:
//!
//! * the standalone [`BlockingRuleEvaluator`] from the first slice, which reports findings over a
//!   caller-supplied rule matrix, and
//! * the typed binder [`build_gate_blocking_decision`], which takes an EQ-41
//!   [`QualityGateConfig`] plus candidate rule evidence and either returns a
//!   [`QualityGateDecision`] bound to that exact configuration version or refuses with a stable
//!   [`BlockingRuleRefusal`] code.
//!
//! The binder is the authoritative path: the configuration's declared `blocking_rules` decide
//! which rules are evaluated, a triggered rule can only become `Reject` or `Rollback`, and a score
//! below the threshold can never become `Pass`.  Nothing here persists a decision, grants
//! authority, changes a candidate's route or executes a target.

use crate::gate::{
    quality_gate_decision_binding, GateVerdict, QualityGateConfig, QualityGateDecision,
    MAX_GATE_EVIDENCE_REFS, MAX_GATE_SCORE_MILLI,
};
use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Versioned wire schema for the standalone blocking-rule finding evaluator.
pub const BLOCKING_RULE_INPUT_SCHEMA: &str = "kiana.quality-blocking-rule-input.v1";
/// Stable registry identifier for the standalone blocking-rule evaluator.
pub const BLOCKING_RULE_EVALUATOR_ID: &str = "blocking-rules";
/// Versioned wire schema for one rule-evidence row.
pub const BLOCKING_RULE_EVIDENCE_SCHEMA: &str = "kiana.quality-blocking-rule.v1";

/// The closed EQ-42 rule vocabulary this evaluator knows how to evaluate.
///
/// A configuration that declares a rule outside this set is refused
/// ([`BLOCKING_RULE_CONFIG_RULE_UNKNOWN`]); evidence naming a rule outside this set is refused
/// ([`BLOCKING_RULE_UNKNOWN`]).
pub const BLOCKING_RULE_KNOWN_RULES: [&str; 6] = [
    "safety",
    "evidence",
    "replay",
    "forbidden_effect",
    "fixture_integrity",
    "infra",
];
const REQUIRED_RULES: &[&str] = &BLOCKING_RULE_KNOWN_RULES;
const RULE_SCHEMA: &str = BLOCKING_RULE_EVIDENCE_SCHEMA;

/// Stable refusal: the gate configuration did not validate.
pub const BLOCKING_RULE_CONFIG_INVALID: &str = "blocking_rule_config_invalid";
/// Stable refusal: the gate configuration declares a rule this evaluator does not know.
pub const BLOCKING_RULE_CONFIG_RULE_UNKNOWN: &str = "blocking_rule_config_rule_unknown";
/// Stable refusal: a rule declared by the configuration has no evidence row.
pub const BLOCKING_RULE_MISSING: &str = "blocking_rule_missing";
/// Stable refusal: evidence names a rule outside the evaluator vocabulary.
pub const BLOCKING_RULE_UNKNOWN: &str = "blocking_rule_unknown";
/// Stable refusal: evidence names the same rule twice.
pub const BLOCKING_RULE_DUPLICATE: &str = "blocking_rule_duplicate";
/// Stable refusal: evidence and the configuration disagree about the evaluated rule set.
pub const BLOCKING_RULE_CONFIG_DRIFT: &str = "blocking_rule_config_drift";
/// Stable refusal: a rule evidence row carries the wrong schema.
pub const BLOCKING_RULE_EVIDENCE_INVALID: &str = "blocking_rule_evidence_invalid";
/// Stable refusal: the decision-input envelope is malformed.
pub const BLOCKING_RULE_INPUT_INVALID: &str = "blocking_rule_input_invalid";
/// Stable refusal: the declared blocking rules contain a duplicate entry.
pub const BLOCKING_RULE_CONFIG_MATRIX_INCOMPLETE: &str = "blocking_rule_config_matrix_incomplete";
/// Stable refusal: a triggered rule cannot be reported without finding references.
pub const BLOCKING_RULE_FINDING_REF_MISSING: &str = "blocking_rule_finding_ref_missing";
/// Stable refusal: a finding reference is empty, padded, oversized or otherwise unbounded.
pub const BLOCKING_RULE_FINDING_REF_INVALID: &str = "blocking_rule_finding_ref_invalid";
/// Stable refusal: a score or threshold is outside `0 < threshold <= score <= MAX`.
pub const BLOCKING_RULE_SCORE_OUT_OF_BOUNDS: &str = "blocking_rule_score_out_of_bounds";
/// Stable refusal: a triggered rule was proposed as `Pass` or `NeedsShadow`.
pub const BLOCKING_RULE_TRIGGERED_FORCES_REJECT: &str = "blocking_rule_triggered_forces_reject";
/// Stable refusal: clean evidence below the threshold was proposed as `Pass`.
pub const BLOCKING_RULE_BELOW_THRESHOLD: &str = "blocking_rule_score_below_threshold";
/// Stable refusal: the proposed verdict disagrees with the evidence-derived verdict.
pub const BLOCKING_RULE_VERDICT_FORGED: &str = "blocking_rule_verdict_forged";
/// Stable refusal: the bound decision could not be built from the validated inputs.
pub const BLOCKING_RULE_DECISION_INVALID: &str = "blocking_rule_decision_invalid";
/// Stable refusal: the built decision did not bind the exact configuration version.
pub const BLOCKING_RULE_DECISION_UNBOUND: &str = "blocking_rule_decision_unbound";
/// Stable refusal: a supplied decision does not match the decision the evidence yields.
pub const BLOCKING_RULE_DECISION_FORGED: &str = "blocking_rule_decision_forged";

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

/// Versioned wire schema for one gate-bound blocking-rule decision request.
pub const BLOCKING_RULE_DECISION_INPUT_SCHEMA: &str =
    "kiana.quality-blocking-rule-decision-input.v1";
/// Maximum rule evidence rows accepted in one decision request.
pub const MAX_BLOCKING_RULE_EVIDENCE: usize = 16;

/// One typed request that binds candidate rule evidence to an EQ-41 configuration version.
///
/// Every field a decision needs beyond the rule verdict is supplied by the caller (identity,
/// candidate, evidence reference, cursor and decision time); the binder never invents them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockingRuleDecisionInput {
    /// Wire schema; must equal [`BLOCKING_RULE_DECISION_INPUT_SCHEMA`].
    pub schema: String,
    /// The exact published gate configuration version the decision must bind.
    pub config: QualityGateConfig,
    /// Identity of the decision to build.
    pub decision_id: String,
    /// Candidate identity the decision is about.
    pub candidate_id: String,
    /// Digest of the exact candidate version the decision is about.
    pub candidate_digest: String,
    /// Rule evidence rows; every rule the configuration declares must appear exactly once.
    pub rules: Vec<BlockingRuleEvidence>,
    /// Weighted score of the candidate, per mille.
    pub weighted_score_milli: u64,
    /// Per-mille weighted-score floor declared for this decision.
    pub score_threshold_milli: u64,
    /// The verdict the caller proposes; the binder refuses to record anything weaker.
    pub proposed_verdict: GateVerdict,
    /// Bounded reference to the evidence the decision cites.
    pub evidence_ref: String,
    /// Durable evidence cursor the decision is made at.
    pub source_cursor: u64,
    /// Decision time in Unix milliseconds.
    pub decided_at_unix_ms: u64,
}

impl BlockingRuleDecisionInput {
    /// Validate the envelope schema and the score bounds.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != BLOCKING_RULE_DECISION_INPUT_SCHEMA {
            return Err(BLOCKING_RULE_INPUT_INVALID);
        }
        if self.score_threshold_milli == 0
            || self.score_threshold_milli > MAX_GATE_SCORE_MILLI
            || self.weighted_score_milli > MAX_GATE_SCORE_MILLI
        {
            return Err(BLOCKING_RULE_SCORE_OUT_OF_BOUNDS);
        }
        if self.rules.len() > MAX_BLOCKING_RULE_EVIDENCE {
            return Err(BLOCKING_RULE_EVIDENCE_INVALID);
        }
        Ok(())
    }
}

/// Build one EQ-41 [`QualityGateDecision`] from a configuration version and rule evidence.
///
/// The configuration decides which rules are evaluated: every rule it declares must have exactly
/// one evidence row, every evidence row must name a declared rule, and the evaluator must know
/// every declared rule.  A triggered rule forces `Reject` (or `Rollback` when the caller proposes
/// it); the caller can never record `Pass` over a triggered rule or over a below-threshold score.
/// The returned decision is built through [`QualityGateDecision::new`], so it copies and binds the
/// exact config identity, version, config digest and suite digest, and its verdict is re-checked
/// by [`verify_gate_blocking_decision`].
pub fn build_gate_blocking_decision(
    input: &BlockingRuleDecisionInput,
) -> Result<QualityGateDecision, &'static str> {
    derive_gate_blocking_decision(input)
}

/// Re-derive the decision from the evidence and refuse a supplied decision that disagrees.
///
/// This is the forgery fence: a decision whose verdict, blockers, regressions or binding axes do
/// not match what the evidence yields is refused with [`BLOCKING_RULE_DECISION_FORGED`].
pub fn verify_gate_blocking_decision(
    input: &BlockingRuleDecisionInput,
    decision: &QualityGateDecision,
) -> Result<(), &'static str> {
    input.validate()?;
    let expected = derive_gate_blocking_decision(input)?;
    if decision.decision_digest != expected.decision_digest
        || decision.verdict != expected.verdict
        || decision.blocking_findings != expected.blocking_findings
        || decision.known_regressions != expected.known_regressions
        || decision.score_delta_milli != expected.score_delta_milli
    {
        return Err(BLOCKING_RULE_DECISION_FORGED);
    }
    if decision.gate_id != input.config.gate_id
        || decision.gate_version != input.config.version
        || decision.config_digest != input.config.config_digest
        || decision.suite_digest != input.config.suite_digest
    {
        return Err(BLOCKING_RULE_DECISION_UNBOUND);
    }
    Ok(())
}

/// One derivation pass shared by the builder and the verifier.  It never calls itself and never
/// calls the public verifier, so the two entry points stay acyclic.
fn derive_gate_blocking_decision(
    input: &BlockingRuleDecisionInput,
) -> Result<QualityGateDecision, &'static str> {
    input.validate()?;
    input.config.validate().map_err(|reason| {
        if reason == "gate_config_blocking_rule_unknown" {
            BLOCKING_RULE_CONFIG_RULE_UNKNOWN
        } else {
            BLOCKING_RULE_CONFIG_INVALID
        }
    })?;
    let declared = declared_rules(input)?;
    let evidence = evidence_matrix(input, &declared)?;
    require_complete_matrix(&declared)?;
    let triggered = evidence.iter().any(|(_, row)| row.triggered);
    let verdict = resolve_verdict(input, triggered)?;
    let blockers = blocking_findings(&declared, &evidence);
    let known_regressions = if triggered {
        blockers.clone()
    } else {
        Vec::new()
    };
    let decision = QualityGateDecision::new(
        input.decision_id.clone(),
        &input.config,
        input.candidate_id.clone(),
        input.candidate_digest.clone(),
        verdict,
        input.weighted_score_milli as i64,
        blockers,
        known_regressions,
        input.evidence_ref.clone(),
        input.source_cursor,
        input.decided_at_unix_ms,
    )
    .map_err(|_| BLOCKING_RULE_DECISION_INVALID)?;
    quality_gate_decision_binding(&input.config, &decision)
        .map_err(|_| BLOCKING_RULE_DECISION_UNBOUND)?;
    Ok(decision)
}

/// Validate and de-duplicate the rule set declared by the configuration version.
///
/// Completeness of the EQ-42 matrix is deliberately **not** checked here: it is a property of the
/// decision request (see [`require_complete_matrix`]), so a configuration that legitimately
/// declares a subset still produces [`BLOCKING_RULE_MISSING`] rather than a config error.
fn declared_rules(input: &BlockingRuleDecisionInput) -> Result<BTreeSet<String>, &'static str> {
    let mut declared = BTreeSet::new();
    for rule in &input.config.blocking_rules {
        if !BLOCKING_RULE_KNOWN_RULES.contains(&rule.as_str()) {
            return Err(BLOCKING_RULE_CONFIG_RULE_UNKNOWN);
        }
        if !declared.insert(rule.clone()) {
            return Err(BLOCKING_RULE_CONFIG_DRIFT);
        }
    }
    Ok(declared)
}

/// Require the declared set to cover every rule EQ-42 evaluates.
fn require_complete_matrix(declared: &BTreeSet<String>) -> Result<(), &'static str> {
    for required in REQUIRED_RULES {
        if !declared.contains(*required) {
            return Err(BLOCKING_RULE_MISSING);
        }
    }
    Ok(())
}

/// Require exactly one valid evidence row per declared rule.
fn evidence_matrix(
    input: &BlockingRuleDecisionInput,
    declared: &BTreeSet<String>,
) -> Result<Vec<(String, BlockingRuleEvidence)>, &'static str> {
    let mut rows = Vec::with_capacity(declared.len());
    let mut seen = BTreeSet::new();
    for evidence in &input.rules {
        if evidence.schema != RULE_SCHEMA || !valid_ref(&evidence.rule) {
            return Err(BLOCKING_RULE_EVIDENCE_INVALID);
        }
        if !BLOCKING_RULE_KNOWN_RULES.contains(&evidence.rule.as_str()) {
            return Err(BLOCKING_RULE_UNKNOWN);
        }
        if !declared.contains(&evidence.rule) {
            return Err(BLOCKING_RULE_CONFIG_DRIFT);
        }
        if !seen.insert(evidence.rule.clone()) {
            return Err(BLOCKING_RULE_DUPLICATE);
        }
        if evidence.triggered && evidence.finding_refs.is_empty() {
            return Err(BLOCKING_RULE_FINDING_REF_MISSING);
        }
        if evidence.finding_refs.len() > MAX_GATE_EVIDENCE_REFS
            || !evidence.finding_refs.iter().all(|value| valid_ref(value))
        {
            return Err(BLOCKING_RULE_FINDING_REF_INVALID);
        }
        rows.push((evidence.rule.clone(), evidence.clone()));
    }
    for required in declared {
        if !seen.contains(required) {
            return Err(BLOCKING_RULE_MISSING);
        }
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(rows)
}

/// Decide the only verdict the evidence permits, and refuse a proposal that is weaker.
fn resolve_verdict(
    input: &BlockingRuleDecisionInput,
    triggered: bool,
) -> Result<GateVerdict, &'static str> {
    let permitted = if triggered {
        GateVerdict::Reject
    } else if input.weighted_score_milli < input.score_threshold_milli {
        GateVerdict::NeedsShadow
    } else {
        GateVerdict::Pass
    };
    let proposed = input.proposed_verdict;
    if triggered && matches!(proposed, GateVerdict::Pass | GateVerdict::NeedsShadow) {
        return Err(BLOCKING_RULE_TRIGGERED_FORCES_REJECT);
    }
    if !triggered
        && proposed == GateVerdict::Pass
        && input.weighted_score_milli < input.score_threshold_milli
    {
        return Err(BLOCKING_RULE_BELOW_THRESHOLD);
    }
    let same_rank_direction = match (permitted, proposed) {
        (GateVerdict::Pass, GateVerdict::Pass)
        | (GateVerdict::NeedsShadow, GateVerdict::NeedsShadow)
        | (GateVerdict::Reject, GateVerdict::Reject)
        | (GateVerdict::Rollback, GateVerdict::Rollback) => true,
        (GateVerdict::Reject, GateVerdict::Rollback) => true,
        (GateVerdict::NeedsShadow, GateVerdict::Reject | GateVerdict::Rollback) => true,
        _ => false,
    };
    if !same_rank_direction {
        return Err(BLOCKING_RULE_VERDICT_FORGED);
    }
    Ok(proposed)
}

/// One stable, sorted blocker token per triggered rule.
fn blocking_findings(
    declared: &BTreeSet<String>,
    evidence: &[(String, BlockingRuleEvidence)],
) -> Vec<String> {
    let mut blockers = Vec::new();
    for rule in declared {
        let Some((_, row)) = evidence.iter().find(|(name, _)| name == rule) else {
            continue;
        };
        if row.triggered {
            blockers.push(format!("blocking:{rule}"));
        }
    }
    blockers.sort();
    blockers.dedup();
    blockers
}

fn valid_ref(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= crate::gate::MAX_GATE_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
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
