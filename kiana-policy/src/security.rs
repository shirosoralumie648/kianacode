//! Versioned, deny-first policy bundles and replayable decision traces.
//!
//! `PolicyBundle` is a pure policy value. It does not execute capabilities, consume approvals or
//! trust caller-supplied identity fields. The existing `PolicyEngine` remains the compatibility
//! adapter used by ControlPlane; `BundlePolicyEngine` exposes the stricter revision-bound contract
//! without creating a second execution path.

use crate::{hard_policy_denial, PolicyEngine};
use kiana_domain::{
    json_digest, CapabilityKind, CapabilityRequest, PolicyDecision, RequestContext, RiskLevel,
    SchemaVersion, SecurityDecisionId, SecurityPolicyId, SecurityReasonCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const POLICY_BUNDLE_SCHEMA: &str = "kiana.policy-bundle.v1";
pub const POLICY_REVISION_SCHEMA: &str = "kiana.policy-revision.v1";
pub const POLICY_DECISION_TRACE_SCHEMA: &str = "kiana.policy-decision-trace.v1";
pub const POLICY_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_POLICY_RULES: usize = 512;
pub const MAX_POLICY_RULE_ID: usize = 128;
pub const MAX_POLICY_OPERATION: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEffect {
    Deny,
    Ask,
    Allow,
}

impl PolicyEffect {
    const fn rank(self) -> u8 {
        match self {
            Self::Deny => 0,
            Self::Ask => 1,
            Self::Allow => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRule {
    pub rule_id: String,
    pub priority: u16,
    pub operation: String,
    #[serde(default)]
    pub capability: Option<CapabilityKind>,
    #[serde(default)]
    pub risk: Option<RiskLevel>,
    pub effect: PolicyEffect,
    #[serde(default)]
    pub reason: Option<SecurityReasonCode>,
}

impl PolicyRule {
    pub fn new(
        rule_id: impl Into<String>,
        priority: u16,
        operation: impl Into<String>,
        capability: Option<CapabilityKind>,
        risk: Option<RiskLevel>,
        effect: PolicyEffect,
        reason: Option<SecurityReasonCode>,
    ) -> Result<Self, String> {
        let rule = Self {
            rule_id: rule_id.into(),
            priority,
            operation: operation.into(),
            capability,
            risk,
            effect,
            reason,
        };
        rule.validate()?;
        Ok(rule)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.rule_id.trim().is_empty()
            || self.rule_id.len() > MAX_POLICY_RULE_ID
            || self.rule_id.contains('\0')
            || self.operation.trim().is_empty()
            || self.operation.len() > MAX_POLICY_OPERATION
            || self.operation.contains('\0')
        {
            return Err("policy_rule_identity_invalid".to_owned());
        }
        match (self.effect, self.reason) {
            (PolicyEffect::Allow, Some(_)) => Err("policy_allow_reason_unexpected".to_owned()),
            (PolicyEffect::Deny | PolicyEffect::Ask, None) => {
                Err("policy_reason_required".to_owned())
            }
            _ => Ok(()),
        }
    }

    fn selector_key(&self) -> String {
        format!(
            "{}\u{1f}{:?}\u{1f}{:?}",
            self.operation, self.capability, self.risk
        )
    }

    fn matches(&self, request: &CapabilityRequest) -> bool {
        self.operation == request.operation
            && self
                .capability
                .as_ref()
                .is_none_or(|capability| capability == &request.capability)
            && self.risk.is_none_or(|risk| risk == request.risk)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRevision {
    pub schema: String,
    pub version: SchemaVersion,
    pub policy_id: SecurityPolicyId,
    pub revision: u64,
    pub authority_epoch: u64,
    pub policy_digest: String,
    pub revision_digest: String,
}

impl PolicyRevision {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_REVISION_SCHEMA
            || !self.version.is_compatible_with(&POLICY_SCHEMA_VERSION)
            || self.policy_id.as_uuid().is_nil()
            || self.revision == 0
            || self.authority_epoch == 0
        {
            return Err("policy_revision_header_invalid".to_owned());
        }
        validate_digest(&self.policy_digest, "policy_revision_policy_digest")?;
        validate_digest(&self.revision_digest, "policy_revision_digest")?;
        if self.revision_digest != self.digest() {
            return Err("policy_revision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let revision: Self = serde_json::from_value(value.clone())
            .map_err(|_| "policy_revision_decode_failed".to_owned())?;
        revision.validate()?;
        Ok(revision)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "policy_revision_encode_failed".to_owned())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "policy_id": self.policy_id,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "policy_digest": self.policy_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyBundle {
    pub schema: String,
    pub version: SchemaVersion,
    pub policy_id: SecurityPolicyId,
    pub revision: u64,
    pub authority_epoch: u64,
    pub default_effect: PolicyEffect,
    pub rules: Vec<PolicyRule>,
    pub policy_digest: String,
    pub bundle_digest: String,
}

impl PolicyBundle {
    pub fn new(
        policy_id: SecurityPolicyId,
        revision: u64,
        authority_epoch: u64,
        mut rules: Vec<PolicyRule>,
    ) -> Result<Self, String> {
        rules.sort_by_key(|rule| (rule.priority, rule.rule_id.clone(), rule.selector_key()));
        let mut bundle = Self {
            schema: POLICY_BUNDLE_SCHEMA.to_owned(),
            version: POLICY_SCHEMA_VERSION,
            policy_id,
            revision,
            authority_epoch,
            default_effect: PolicyEffect::Deny,
            rules,
            policy_digest: String::new(),
            bundle_digest: String::new(),
        };
        bundle.policy_digest = bundle.rules_digest();
        bundle.bundle_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn deny_all(
        policy_id: SecurityPolicyId,
        revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        Self::new(policy_id, revision, authority_epoch, Vec::new())
    }

    pub fn with_default_effect(mut self, effect: PolicyEffect) -> Result<Self, String> {
        if effect == PolicyEffect::Allow {
            return Err("policy_default_allow_forbidden".to_owned());
        }
        self.default_effect = effect;
        self.policy_digest = self.rules_digest();
        self.bundle_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn revision_snapshot(&self) -> Result<PolicyRevision, String> {
        let mut revision = PolicyRevision {
            schema: POLICY_REVISION_SCHEMA.to_owned(),
            version: POLICY_SCHEMA_VERSION,
            policy_id: self.policy_id,
            revision: self.revision,
            authority_epoch: self.authority_epoch,
            policy_digest: self.policy_digest.clone(),
            revision_digest: String::new(),
        };
        revision.revision_digest = revision.digest();
        revision.validate()?;
        Ok(revision)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let bundle: Self = serde_json::from_value(value.clone())
            .map_err(|_| "policy_bundle_decode_failed".to_owned())?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "policy_bundle_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_BUNDLE_SCHEMA
            || !self.version.is_compatible_with(&POLICY_SCHEMA_VERSION)
            || self.policy_id.as_uuid().is_nil()
            || self.revision == 0
            || self.authority_epoch == 0
            || self.rules.len() > MAX_POLICY_RULES
            || self.default_effect == PolicyEffect::Allow
        {
            return Err("policy_bundle_header_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        let mut selectors = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            if !ids.insert(rule.rule_id.as_str()) {
                return Err("policy_rule_duplicate_id".to_owned());
            }
            if !selectors.insert(rule.selector_key()) {
                return Err("policy_rule_duplicate_selector".to_owned());
            }
        }
        if self.rules.windows(2).any(|pair| {
            (
                pair[0].priority,
                pair[0].rule_id.as_str(),
                pair[0].selector_key(),
            ) > (
                pair[1].priority,
                pair[1].rule_id.as_str(),
                pair[1].selector_key(),
            )
        }) {
            return Err("policy_rules_noncanonical".to_owned());
        }
        validate_digest(&self.policy_digest, "policy_bundle_policy_digest")?;
        if self.policy_digest != self.rules_digest() {
            return Err("policy_bundle_policy_digest_mismatch".to_owned());
        }
        validate_digest(&self.bundle_digest, "policy_bundle_digest")?;
        if self.bundle_digest != self.digest() {
            return Err("policy_bundle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn evaluate(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PolicyEvaluation, String> {
        self.evaluate_with_snapshot(context, request, self.authority_epoch, &self.policy_digest)
    }

    pub fn evaluate_with_snapshot(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
        authority_epoch: u64,
        policy_digest: &str,
    ) -> Result<PolicyEvaluation, String> {
        self.validate()?;
        let input_digest = json_digest(&json!({
            "context": context_digest(context),
            "request": request,
        }));
        let context_digest = context_digest(context);
        let mut matched = Vec::new();
        let mut selected: Option<&PolicyRule> = None;
        let mut deny_reason = None;
        if authority_epoch < self.authority_epoch {
            deny_reason = Some(SecurityReasonCode::PolicyAuthorityEpochRollback);
        } else if authority_epoch != self.authority_epoch {
            deny_reason = Some(SecurityReasonCode::PolicyAuthorityEpochStale);
        } else if policy_digest != self.policy_digest {
            deny_reason = Some(SecurityReasonCode::PolicyRevisionStale);
        } else if let Some(reason) = hard_policy_denial(context, request) {
            deny_reason = Some(kiana_domain::classify_security_reason(&reason));
        } else {
            for rule in &self.rules {
                if rule.matches(request) {
                    matched.push(rule.rule_id.clone());
                    selected = match selected {
                        None => Some(rule),
                        Some(current)
                            if (rule.effect.rank(), rule.priority, rule.rule_id.as_str())
                                < (
                                    current.effect.rank(),
                                    current.priority,
                                    current.rule_id.as_str(),
                                ) =>
                        {
                            Some(rule)
                        }
                        Some(current) => Some(current),
                    };
                }
            }
        }

        let (decision, outcome, reason) = if let Some(reason) = deny_reason {
            (
                PolicyDecision::Deny {
                    reason: reason.as_str().to_owned(),
                },
                PolicyOutcome::Deny,
                Some(reason),
            )
        } else if let Some(rule) = selected {
            match rule.effect {
                PolicyEffect::Deny => {
                    let reason = rule
                        .reason
                        .unwrap_or(SecurityReasonCode::PolicyBundleInvalid);
                    (
                        PolicyDecision::Deny {
                            reason: reason.as_str().to_owned(),
                        },
                        PolicyOutcome::Deny,
                        Some(reason),
                    )
                }
                PolicyEffect::Ask => {
                    let reason = rule
                        .reason
                        .unwrap_or(SecurityReasonCode::PolicyBundleInvalid);
                    (
                        PolicyDecision::Ask {
                            reason: reason.as_str().to_owned(),
                        },
                        PolicyOutcome::Ask,
                        Some(reason),
                    )
                }
                PolicyEffect::Allow => (
                    PolicyDecision::Allow {
                        authorization_id: format!(
                            "policy:{}:{}",
                            self.revision, request.request_id
                        ),
                    },
                    PolicyOutcome::Allow,
                    None,
                ),
            }
        } else {
            let reason = match self.default_effect {
                PolicyEffect::Deny => SecurityReasonCode::PolicyOperationUnregistered,
                PolicyEffect::Ask => SecurityReasonCode::PolicyApprovalRequired,
                PolicyEffect::Allow => SecurityReasonCode::PolicyBundleInvalid,
            };
            let decision = match self.default_effect {
                PolicyEffect::Deny => PolicyDecision::Deny {
                    reason: reason.as_str().to_owned(),
                },
                PolicyEffect::Ask => PolicyDecision::Ask {
                    reason: reason.as_str().to_owned(),
                },
                PolicyEffect::Allow => PolicyDecision::Deny {
                    reason: reason.as_str().to_owned(),
                },
            };
            (
                decision,
                if self.default_effect == PolicyEffect::Ask {
                    PolicyOutcome::Ask
                } else {
                    PolicyOutcome::Deny
                },
                Some(reason),
            )
        };

        let trace = DecisionTrace::new(
            self,
            request,
            input_digest,
            context_digest,
            matched,
            outcome,
            reason,
        )?;
        Ok(PolicyEvaluation { decision, trace })
    }

    fn rules_digest(&self) -> String {
        json_digest(&json!({
            "default_effect": self.default_effect,
            "rules": self.rules,
        }))
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "policy_id": self.policy_id,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
            "default_effect": self.default_effect,
            "rules": self.rules,
            "policy_digest": self.policy_digest,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOutcome {
    Allow,
    Ask,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionTrace {
    pub schema: String,
    pub version: SchemaVersion,
    pub decision_id: SecurityDecisionId,
    pub policy_id: SecurityPolicyId,
    pub policy_revision: u64,
    pub authority_epoch: u64,
    pub request_id: kiana_domain::RequestId,
    pub input_digest: String,
    pub context_digest: String,
    pub matched_rule_ids: Vec<String>,
    pub outcome: PolicyOutcome,
    #[serde(default)]
    pub reason: Option<SecurityReasonCode>,
    pub trace_digest: String,
}

impl DecisionTrace {
    fn new(
        bundle: &PolicyBundle,
        request: &CapabilityRequest,
        input_digest: String,
        context_digest: String,
        matched_rule_ids: Vec<String>,
        outcome: PolicyOutcome,
        reason: Option<SecurityReasonCode>,
    ) -> Result<Self, String> {
        let mut trace = Self {
            schema: POLICY_DECISION_TRACE_SCHEMA.to_owned(),
            version: POLICY_SCHEMA_VERSION,
            decision_id: SecurityDecisionId::new(),
            policy_id: bundle.policy_id,
            policy_revision: bundle.revision,
            authority_epoch: bundle.authority_epoch,
            request_id: request.request_id,
            input_digest,
            context_digest,
            matched_rule_ids,
            outcome,
            reason,
            trace_digest: String::new(),
        };
        trace.trace_digest = trace.digest();
        trace.validate()?;
        Ok(trace)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let trace: Self = serde_json::from_value(value.clone())
            .map_err(|_| "policy_decision_trace_decode_failed".to_owned())?;
        trace.validate()?;
        Ok(trace)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "policy_decision_trace_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != POLICY_DECISION_TRACE_SCHEMA
            || !self.version.is_compatible_with(&POLICY_SCHEMA_VERSION)
            || self.decision_id.as_uuid().is_nil()
            || self.policy_id.as_uuid().is_nil()
            || self.policy_revision == 0
            || self.authority_epoch == 0
            || self.matched_rule_ids.iter().any(|id| id.trim().is_empty())
        {
            return Err("policy_decision_trace_header_invalid".to_owned());
        }
        validate_digest(&self.input_digest, "policy_decision_trace_input_digest")?;
        validate_digest(&self.context_digest, "policy_decision_trace_context_digest")?;
        validate_digest(&self.trace_digest, "policy_decision_trace_digest")?;
        if self.trace_digest != self.digest() {
            return Err("policy_decision_trace_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "decision_id": self.decision_id,
            "policy_id": self.policy_id,
            "policy_revision": self.policy_revision,
            "authority_epoch": self.authority_epoch,
            "request_id": self.request_id,
            "input_digest": self.input_digest,
            "context_digest": self.context_digest,
            "matched_rule_ids": self.matched_rule_ids,
            "outcome": self.outcome,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolicyEvaluation {
    pub decision: PolicyDecision,
    pub trace: DecisionTrace,
}

impl PolicyEvaluation {
    pub fn validate(&self) -> Result<(), String> {
        self.trace.validate()
    }
}

#[derive(Clone, Debug)]
pub struct BundlePolicyEngine {
    bundle: PolicyBundle,
}

impl BundlePolicyEngine {
    pub fn new(bundle: PolicyBundle) -> Result<Self, String> {
        bundle.validate()?;
        Ok(Self { bundle })
    }

    pub fn bundle(&self) -> &PolicyBundle {
        &self.bundle
    }

    pub fn evaluate_with_trace(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PolicyEvaluation, String> {
        self.bundle.evaluate(context, request)
    }

    pub fn evaluate(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> PolicyDecision {
        <Self as PolicyEngine>::evaluate(self, context, request)
    }
}

impl PolicyEngine for BundlePolicyEngine {
    fn evaluate(&self, context: &RequestContext, request: &CapabilityRequest) -> PolicyDecision {
        self.bundle
            .evaluate(context, request)
            .map(|evaluation| evaluation.decision)
            .unwrap_or_else(|_| PolicyDecision::Deny {
                reason: SecurityReasonCode::PolicyBundleInvalid.as_str().to_owned(),
            })
    }
}

fn context_digest(context: &RequestContext) -> String {
    json_digest(&json!({
        "request_id": context.request_id,
        "session_id": context.session_id,
        "actor_id": context.actor_id,
        "project_root": context.project_root,
        "project_trusted": context.project_trusted,
        "permission_profile": context.permission_profile,
        "role_id": context.role_id,
        "department_id": context.department_id,
        "work_packet_id": context.work_packet_id,
        "cell_id": context.cell_id,
        "path_allow": context.path_allow,
    }))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
