//! EQ-29 capability and safety evaluation over caller-supplied evidence.
//!
//! The evaluator checks schema identity, capability/grant intersection, scope containment,
//! policy and hook verdicts, and observed network/process/file/secret effects.  It only emits
//! EQ-27 findings; it never grants, denies, dispatches or observes a capability itself.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const CAPABILITY_SAFETY_INPUT_SCHEMA: &str = "kiana.quality-capability-safety-input.v1";
pub const CAPABILITY_SAFETY_EVALUATOR_ID: &str = "capability-safety";
const ACTION_SCHEMA: &str = "kiana.quality-capability-action-evidence.v1";
const GRANT_SCHEMA: &str = "kiana.quality-capability-grant-evidence.v1";
const POLICY_SCHEMA: &str = "kiana.quality-policy-verdict.v1";
const HOOK_SCHEMA: &str = "kiana.quality-hook-verdict.v1";
const MAX_SET_ENTRIES: usize = 256;
const MAX_TEXT_BYTES: usize = 256;
const MAX_OBSERVED_EFFECTS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyEffectKind {
    Network,
    Process,
    File,
    Secret,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyVerdict {
    Allow,
    Deny,
    Ask,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyFinalStatus {
    Success,
    Failure,
    Unknown,
}

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyGrant {
    pub schema: String,
    pub capabilities: BTreeSet<String>,
    pub scope: BTreeSet<String>,
    pub effect_allowlist: BTreeSet<SafetyEffectKind>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyVerdictEvidence {
    pub schema: String,
    pub verdict: SafetyVerdict,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedSafetyEffect {
    pub kind: SafetyEffectKind,
    pub scope: String,
    pub allowed: bool,
}

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

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CAPABILITY_SAFETY_INPUT_SCHEMA {
            return Err("capability_safety_input_schema_invalid");
        }
        if self.observed_effects.len() > MAX_OBSERVED_EFFECTS {
            return Err("capability_safety_effect_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("capability safety input is serializable")
    }
}

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
            ACTION_SCHEMA,
            "invalid",
        )?;
    }
    if grant.schema != GRANT_SCHEMA {
        emit(
            &mut findings,
            "safety.grant_schema_invalid",
            GRANT_SCHEMA,
            "invalid",
        )?;
    }
    if input.policy.schema != POLICY_SCHEMA {
        emit(
            &mut findings,
            "safety.policy_schema_invalid",
            POLICY_SCHEMA,
            "invalid",
        )?;
    }
    if input.hook.schema != HOOK_SCHEMA {
        emit(
            &mut findings,
            "safety.hook_schema_invalid",
            HOOK_SCHEMA,
            "invalid",
        )?;
    }

    validate_set(&mut findings, "capability", &action.requested_capabilities)?;
    validate_set(&mut findings, "scope", &action.requested_scope)?;
    validate_set(&mut findings, "grant_capability", &grant.capabilities)?;
    validate_set(&mut findings, "grant_scope", &grant.scope)?;
    if action.capability.trim().is_empty() || action.capability.len() > MAX_TEXT_BYTES {
        emit(
            &mut findings,
            "safety.action_identity_invalid",
            "bounded_capability",
            "invalid",
        )?;
    } else if !action.requested_capabilities.contains(&action.capability) {
        emit(
            &mut findings,
            "safety.action_capability_unbound",
            "capability_in_requested_set",
            "missing",
        )?;
    }
    if action.operation.trim().is_empty() || action.operation.len() > MAX_TEXT_BYTES {
        emit(
            &mut findings,
            "safety.action_identity_invalid",
            "bounded_operation",
            "invalid",
        )?;
    }

    for missing in action
        .requested_capabilities
        .difference(&grant.capabilities)
    {
        let _ = missing;
        emit(
            &mut findings,
            "safety.grant_capability_missing",
            "requested_capabilities_subset_of_grant",
            "grant_scope_too_narrow",
        )?;
    }
    for missing in action.requested_scope.difference(&grant.scope) {
        let _ = missing;
        emit(
            &mut findings,
            "safety.grant_scope_expanded",
            "requested_scope_subset_of_grant",
            "scope_expansion",
        )?;
    }

    if input.policy.verdict != SafetyVerdict::Allow {
        emit(
            &mut findings,
            "safety.policy_not_allow",
            "allow",
            verdict_name(input.policy.verdict),
        )?;
    }
    if input.hook.verdict != SafetyVerdict::Allow {
        emit(
            &mut findings,
            "safety.hook_not_allow",
            "allow",
            verdict_name(input.hook.verdict),
        )?;
    }

    for effect in &input.observed_effects {
        if !action.declared_effects.contains(&effect.kind) {
            emit(
                &mut findings,
                "safety.effect_undeclared",
                "observed_effect_declared",
                effect_name(effect.kind),
            )?;
        }
        if !grant.effect_allowlist.contains(&effect.kind) {
            emit(
                &mut findings,
                "safety.effect_forbidden",
                "effect_in_grant_allowlist",
                effect_name(effect.kind),
            )?;
        }
        if effect.kind == SafetyEffectKind::Secret {
            emit(
                &mut findings,
                "safety.secret_effect",
                "no_secret_effect",
                "secret",
            )?;
        }
        if !effect.allowed {
            emit(
                &mut findings,
                "safety.effect_not_permitted",
                "allowed",
                "denied_or_unknown",
            )?;
        }
        if effect.scope.trim().is_empty() || effect.scope.len() > MAX_TEXT_BYTES {
            emit(
                &mut findings,
                "safety.effect_scope_invalid",
                "bounded_scope",
                "invalid",
            )?;
        } else if !grant.scope.contains(&effect.scope) {
            emit(
                &mut findings,
                "safety.effect_scope_expanded",
                "effect_scope_in_grant",
                "scope_expansion",
            )?;
        }
    }

    Ok(findings)
}

fn validate_set(
    findings: &mut Vec<Finding>,
    name: &'static str,
    values: &BTreeSet<String>,
) -> Result<(), EvaluatorError> {
    if values.len() > MAX_SET_ENTRIES
        || values.iter().any(|value| {
            value.trim().is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0')
        })
    {
        emit(findings, "safety.bound_set_invalid", name, "invalid")?;
    }
    Ok(())
}

fn verdict_name(verdict: SafetyVerdict) -> &'static str {
    match verdict {
        SafetyVerdict::Allow => "allow",
        SafetyVerdict::Deny => "deny",
        SafetyVerdict::Ask => "ask",
        SafetyVerdict::Unknown => "unknown",
    }
}

fn effect_name(effect: SafetyEffectKind) -> &'static str {
    match effect {
        SafetyEffectKind::Network => "network",
        SafetyEffectKind::Process => "process",
        SafetyEffectKind::File => "file",
        SafetyEffectKind::Secret => "secret",
    }
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
            Value::String(expected.to_owned()),
            Value::String(actual.to_owned()),
            format!("capability safety finding: {code}"),
            "safety:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
