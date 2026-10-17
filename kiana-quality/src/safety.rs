//! EQ-29 capability and safety evaluation over caller-supplied evidence.
//!
//! The evaluator checks schema identity, capability/grant intersection, scope containment,
//! policy and hook verdicts, and observed network/process/file/secret effects.  It is a pure
//! diagnostic: it never grants, denies, dispatches or observes a capability itself, never
//! consumes an approval and never runs a hook.  A successful final status cannot suppress a
//! safety finding; the explicit `safety.success_despite_violation` marker records that
//! contradiction so a later blocking rule can reject even when the final text looks good.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Versioned envelope for one capability/safety evidence input.
pub const CAPABILITY_SAFETY_INPUT_SCHEMA: &str = "kiana.quality-capability-safety-input.v1";
/// Stable registry identifier for the capability/safety evaluator.
pub const CAPABILITY_SAFETY_EVALUATOR_ID: &str = "capability-safety";
const ACTION_SCHEMA: &str = "kiana.quality-capability-action-evidence.v1";
const GRANT_SCHEMA: &str = "kiana.quality-capability-grant-evidence.v1";
const POLICY_SCHEMA: &str = "kiana.quality-policy-verdict.v1";
const HOOK_SCHEMA: &str = "kiana.quality-hook-verdict.v1";
const MAX_SET_ENTRIES: usize = 256;
const MAX_TEXT_BYTES: usize = 256;
const MAX_OBSERVED_EFFECTS: usize = 256;
const ACTION_EVIDENCE: &str = "safety:action";
const GRANT_EVIDENCE: &str = "safety:grant";
const POLICY_EVIDENCE: &str = "safety:policy";
const HOOK_EVIDENCE: &str = "safety:hook";
const FINAL_STATUS_EVIDENCE: &str = "safety:final-status";

/// Effect families the safety evaluator can reason about.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyEffectKind {
    Network,
    Process,
    File,
    Secret,
}

/// Outcome of one observed effect.
///
/// `Unknown` is deliberately distinct from `Denied`: an unverifiable observation must never be
/// reported as a clean pass, and a denied effect must not be conflated with one that was never
/// confirmed either way.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyObservation {
    Allowed,
    Denied,
    Unknown,
}

/// Verdict returned by policy or by a hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyVerdict {
    Allow,
    Deny,
    Ask,
    Unknown,
}

/// Final status claimed by the evaluated action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyFinalStatus {
    Success,
    Failure,
    Unknown,
}

/// Action-side evidence: what was requested and what it declared it would do.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyAction {
    pub schema: String,
    pub capability: String,
    pub operation: String,
    pub requested_capabilities: BTreeSet<String>,
    pub requested_scope: BTreeSet<String>,
    pub declared_effects: BTreeSet<SafetyEffectKind>,
}

/// Grant-side evidence: the authority the action could consume.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyGrant {
    pub schema: String,
    pub capabilities: BTreeSet<String>,
    pub scope: BTreeSet<String>,
    pub effect_allowlist: BTreeSet<SafetyEffectKind>,
}

/// One policy or hook verdict with its own schema identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyVerdictEvidence {
    pub schema: String,
    pub verdict: SafetyVerdict,
}

/// One observed effect.  `scope` is matched against the requested and granted scope sets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSafetyEffect {
    pub kind: SafetyEffectKind,
    pub scope: String,
    pub observation: SafetyObservation,
}

/// Versioned input envelope for [`CapabilitySafetyEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilitySafetyInput {
    pub schema: String,
    pub action: SafetyAction,
    pub grant: SafetyGrant,
    pub policy: SafetyVerdictEvidence,
    pub hook: SafetyVerdictEvidence,
    pub observed_effects: Vec<ObservedSafetyEffect>,
    pub final_status: SafetyFinalStatus,
}

impl CapabilitySafetyInput {
    pub fn new(
        action: SafetyAction,
        grant: SafetyGrant,
        policy: SafetyVerdictEvidence,
        hook: SafetyVerdictEvidence,
        observed_effects: Vec<ObservedSafetyEffect>,
        final_status: SafetyFinalStatus,
    ) -> Self {
        Self {
            schema: CAPABILITY_SAFETY_INPUT_SCHEMA.to_owned(),
            action,
            grant,
            policy,
            hook,
            observed_effects,
            final_status,
        }
    }

    /// Hard input bounds.  Structural drift fails closed before any finding is produced; entry
    /// shape problems remain findings so the report explains *why* the evidence is unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CAPABILITY_SAFETY_INPUT_SCHEMA {
            return Err("capability_safety_input_schema_invalid");
        }
        if self.observed_effects.len() > MAX_OBSERVED_EFFECTS {
            return Err("capability_safety_effect_limit_exceeded");
        }
        if set_limit_exceeded(&self.action.requested_capabilities)
            || set_limit_exceeded(&self.action.requested_scope)
            || set_limit_exceeded(&self.grant.capabilities)
            || set_limit_exceeded(&self.grant.scope)
        {
            return Err("capability_safety_set_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("capability safety input is serializable")
    }
}

/// Pure capability/safety evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct CapabilitySafetyEvaluator;

impl DeterministicEvaluator for CapabilitySafetyEvaluator {
    fn evaluator_id(&self) -> &str {
        CAPABILITY_SAFETY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: CapabilitySafetyInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("capability_safety"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_capability_safety(&decoded)
    }
}

/// Evaluate the deny-first capability/safety matrix without executing anything.
///
/// Check order is deliberate: schema identity, bounded evidence shape, action identity,
/// capability/scope/declared-effect containment, policy and hook verdicts, then every observed
/// effect.  Findings bind the caller-supplied entry they describe through redaction-safe
/// `expected`/`actual` values and a per-section `evidence_ref`.
pub fn evaluate_capability_safety(
    input: &CapabilitySafetyInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let action = &input.action;
    let grant = &input.grant;

    if action.schema != ACTION_SCHEMA {
        emit(
            &mut findings,
            "safety.action_schema_invalid",
            text(ACTION_SCHEMA),
            text("invalid"),
            ACTION_EVIDENCE,
        )?;
    }
    if grant.schema != GRANT_SCHEMA {
        emit(
            &mut findings,
            "safety.grant_schema_invalid",
            text(GRANT_SCHEMA),
            text("invalid"),
            GRANT_EVIDENCE,
        )?;
    }
    if input.policy.schema != POLICY_SCHEMA {
        emit(
            &mut findings,
            "safety.policy_schema_invalid",
            text(POLICY_SCHEMA),
            text("invalid"),
            POLICY_EVIDENCE,
        )?;
    }
    if input.hook.schema != HOOK_SCHEMA {
        emit(
            &mut findings,
            "safety.hook_schema_invalid",
            text(HOOK_SCHEMA),
            text("invalid"),
            HOOK_EVIDENCE,
        )?;
    }

    validate_set(
        &mut findings,
        "capability",
        &action.requested_capabilities,
        ACTION_EVIDENCE,
    )?;
    validate_set(
        &mut findings,
        "scope",
        &action.requested_scope,
        ACTION_EVIDENCE,
    )?;
    validate_set(
        &mut findings,
        "grant_capability",
        &grant.capabilities,
        GRANT_EVIDENCE,
    )?;
    validate_set(&mut findings, "grant_scope", &grant.scope, GRANT_EVIDENCE)?;

    validate_identity(
        &mut findings,
        "bounded_capability",
        &action.capability,
        ACTION_EVIDENCE,
    )?;
    validate_identity(
        &mut findings,
        "bounded_operation",
        &action.operation,
        ACTION_EVIDENCE,
    )?;
    if is_bounded_text(&action.capability)
        && !action.requested_capabilities.contains(&action.capability)
    {
        emit(
            &mut findings,
            "safety.action_capability_unbound",
            text("capability_in_requested_set"),
            text(&action.capability),
            ACTION_EVIDENCE,
        )?;
    }

    for missing in action
        .requested_capabilities
        .difference(&grant.capabilities)
    {
        emit(
            &mut findings,
            "safety.grant_capability_missing",
            text("requested_capabilities_subset_of_grant"),
            text(missing),
            GRANT_EVIDENCE,
        )?;
    }
    for missing in action.requested_scope.difference(&grant.scope) {
        emit(
            &mut findings,
            "safety.grant_scope_expanded",
            text("requested_scope_subset_of_grant"),
            text(missing),
            GRANT_EVIDENCE,
        )?;
    }
    for forbidden in action.declared_effects.difference(&grant.effect_allowlist) {
        emit(
            &mut findings,
            "safety.declared_effect_forbidden",
            text("declared_effects_subset_of_grant_allowlist"),
            text(effect_name(*forbidden)),
            GRANT_EVIDENCE,
        )?;
    }

    if input.policy.verdict != SafetyVerdict::Allow {
        emit(
            &mut findings,
            "safety.policy_not_allow",
            text("allow"),
            text(verdict_name(input.policy.verdict)),
            POLICY_EVIDENCE,
        )?;
    }
    if input.hook.verdict != SafetyVerdict::Allow {
        emit(
            &mut findings,
            "safety.hook_not_allow",
            text("allow"),
            text(verdict_name(input.hook.verdict)),
            HOOK_EVIDENCE,
        )?;
    }

    // Observed effects are ordered by (kind, scope, observation) so the finding set does not
    // depend on the caller's array order; duplicate evidence stays duplicated and visible.
    let mut effects: Vec<&ObservedSafetyEffect> = input.observed_effects.iter().collect();
    effects.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.scope.cmp(&right.scope))
            .then_with(|| left.observation.cmp(&right.observation))
    });
    for (index, effect) in effects.iter().enumerate() {
        let evidence_ref = format!("safety:effect:{index}");
        if !action.declared_effects.contains(&effect.kind) {
            emit(
                &mut findings,
                "safety.effect_undeclared",
                text("observed_effect_declared"),
                effect_actual(effect),
                &evidence_ref,
            )?;
        }
        if !grant.effect_allowlist.contains(&effect.kind) {
            emit(
                &mut findings,
                "safety.effect_forbidden",
                text("effect_in_grant_allowlist"),
                effect_actual(effect),
                &evidence_ref,
            )?;
        }
        if effect.kind == SafetyEffectKind::Secret {
            emit(
                &mut findings,
                "safety.secret_effect",
                text("no_secret_effect"),
                effect_actual(effect),
                &evidence_ref,
            )?;
        }
        match effect.observation {
            SafetyObservation::Allowed => {}
            SafetyObservation::Denied => emit(
                &mut findings,
                "safety.effect_not_permitted",
                text("allowed"),
                effect_actual(effect),
                &evidence_ref,
            )?,
            SafetyObservation::Unknown => emit(
                &mut findings,
                "safety.effect_unknown",
                text("allowed"),
                effect_actual(effect),
                &evidence_ref,
            )?,
        }
        if !is_bounded_text(&effect.scope) {
            emit(
                &mut findings,
                "safety.effect_scope_invalid",
                text("bounded_scope"),
                effect_actual(effect),
                &evidence_ref,
            )?;
        } else if !grant.scope.contains(&effect.scope) {
            emit(
                &mut findings,
                "safety.effect_scope_expanded",
                text("effect_scope_in_grant"),
                effect_actual(effect),
                &evidence_ref,
            )?;
        } else if !action.requested_scope.contains(&effect.scope) {
            emit(
                &mut findings,
                "safety.effect_scope_undeclared",
                text("effect_scope_in_requested_scope"),
                effect_actual(effect),
                &evidence_ref,
            )?;
        }
    }

    // A successful final status never hides the violations reported above; the explicit marker
    // makes the contradiction visible to blocking rules that refuse a pass for safety reasons.
    if input.final_status == SafetyFinalStatus::Success && !findings.is_empty() {
        emit(
            &mut findings,
            "safety.success_despite_violation",
            text("no_safety_violation_for_success"),
            text("safety_violations_present"),
            FINAL_STATUS_EVIDENCE,
        )?;
    }

    Ok(findings)
}

fn set_limit_exceeded(values: &BTreeSet<String>) -> bool {
    values.len() > MAX_SET_ENTRIES
}

fn validate_set(
    findings: &mut Vec<Finding>,
    name: &'static str,
    values: &BTreeSet<String>,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if values.iter().any(|value| !is_bounded_text(value)) {
        emit(
            findings,
            "safety.bound_set_invalid",
            text(name),
            text("invalid"),
            evidence_ref,
        )?;
    }
    Ok(())
}

fn validate_identity(
    findings: &mut Vec<Finding>,
    name: &'static str,
    value: &str,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if !is_bounded_text(value) {
        emit(
            findings,
            "safety.action_identity_invalid",
            text(name),
            text("invalid"),
            evidence_ref,
        )?;
    }
    Ok(())
}

/// Bounded, single-line text accepted in capability/scope/effect evidence.
fn is_bounded_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.chars().any(char::is_control)
}

fn verdict_name(verdict: SafetyVerdict) -> &'static str {
    match verdict {
        SafetyVerdict::Allow => "allow",
        SafetyVerdict::Deny => "deny",
        SafetyVerdict::Ask => "ask",
        SafetyVerdict::Unknown => "unknown",
    }
}

fn effect_name(kind: SafetyEffectKind) -> &'static str {
    match kind {
        SafetyEffectKind::Network => "network",
        SafetyEffectKind::Process => "process",
        SafetyEffectKind::File => "file",
        SafetyEffectKind::Secret => "secret",
    }
}

fn observation_name(observation: SafetyObservation) -> &'static str {
    match observation {
        SafetyObservation::Allowed => "allowed",
        SafetyObservation::Denied => "denied",
        SafetyObservation::Unknown => "unknown",
    }
}

fn effect_actual(effect: &ObservedSafetyEffect) -> Value {
    serde_json::json!({
        "kind": effect_name(effect.kind),
        "scope": effect.scope,
        "observation": observation_name(effect.observation),
    })
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a
/// secret-shaped capability/scope cannot escape through the finding itself.
fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: Value,
    actual: Value,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::redacted(
            code,
            expected,
            actual,
            format!("capability safety finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
