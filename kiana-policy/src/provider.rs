//! Server-owned provider.use policy and read-only credential status projection.
//!
//! Provider policy is intentionally separate from credential material.  A configured credential
//! never grants provider use by itself, and a probe can report scope/expiry/re-auth state without
//! turning an authentication failure into a capability grant.

use kiana_domain::{json_digest, CredentialDisplayStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const PROVIDER_POLICY_SCHEMA: &str = "kiana.provider-use-policy.v1";
pub const PROVIDER_POLICY_RULE_SCHEMA: &str = "kiana.provider-use-policy-rule.v1";
pub const PROVIDER_POLICY_DECISION_SCHEMA: &str = "kiana.provider-use-policy-decision.v1";
pub const PROVIDER_USE_OPERATION: &str = "provider.use";

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains(['\0', '\r', '\n'])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPolicyEffect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPolicyRule {
    pub schema: String,
    pub rule_id: String,
    pub provider_id: String,
    pub operation: String,
    pub effect: ProviderPolicyEffect,
    pub precedence: u32,
    pub rule_digest: String,
}

impl ProviderPolicyRule {
    pub fn new(
        rule_id: impl Into<String>,
        provider_id: impl Into<String>,
        operation: impl Into<String>,
        effect: ProviderPolicyEffect,
        precedence: u32,
    ) -> Result<Self, String> {
        let mut rule = Self {
            schema: PROVIDER_POLICY_RULE_SCHEMA.to_owned(),
            rule_id: rule_id.into(),
            provider_id: provider_id.into(),
            operation: operation.into(),
            effect,
            precedence,
            rule_digest: String::new(),
        };
        rule.rule_digest = rule.digest();
        rule.validate()?;
        Ok(rule)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_POLICY_RULE_SCHEMA
            || !bounded(&self.rule_id, 256)
            || !bounded(&self.provider_id, 128)
            || (!bounded(&self.operation, 128) && self.operation != "*")
            || self.precedence > 1_000_000
            || self.rule_digest != self.digest()
        {
            return Err("provider_policy_rule_invalid".to_owned());
        }
        if self.provider_id != "*" && !bounded(&self.provider_id, 128) {
            return Err("provider_policy_provider_invalid".to_owned());
        }
        Ok(())
    }

    fn matches(&self, provider_id: &str, operation: &str) -> bool {
        (self.provider_id == "*" || self.provider_id == provider_id)
            && (self.operation == "*" || self.operation == operation)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "rule_id": self.rule_id,
            "provider_id": self.provider_id,
            "operation": self.operation,
            "effect": self.effect,
            "precedence": self.precedence,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPolicyBundle {
    pub schema: String,
    pub version: u64,
    pub authority_epoch: u64,
    pub default_effect: ProviderPolicyEffect,
    pub rules: Vec<ProviderPolicyRule>,
    pub policy_digest: String,
}

impl ProviderPolicyBundle {
    pub fn new(
        authority_epoch: u64,
        default_effect: ProviderPolicyEffect,
        rules: Vec<ProviderPolicyRule>,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            schema: PROVIDER_POLICY_SCHEMA.to_owned(),
            version: 1,
            authority_epoch,
            default_effect,
            rules,
            policy_digest: String::new(),
        };
        bundle.policy_digest = bundle.digest();
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn default_deny(authority_epoch: u64) -> Result<Self, String> {
        Self::new(authority_epoch, ProviderPolicyEffect::Deny, Vec::new())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_POLICY_SCHEMA
            || self.version == 0
            || self.authority_epoch == 0
            || self.rules.len() > 256
            || self.policy_digest != self.digest()
        {
            return Err("provider_policy_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            if !ids.insert(rule.rule_id.clone()) {
                return Err("provider_policy_rule_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// Evaluate a provider.use request.  Matching rules are considered in declaration order and
    /// the highest `(precedence, declaration_index)` wins; no plugin/default route can bypass the
    /// server-owned bundle. Credential status is checked after the policy effect so missing scope
    /// remains a distinct diagnostic rather than an invalid-credential success/deny collapse.
    pub fn evaluate(
        &self,
        provider_id: &str,
        operation: &str,
        credential_status: CredentialDisplayStatus,
    ) -> ProviderPolicyDecision {
        if self.validate().is_err() {
            return ProviderPolicyDecision {
                schema: PROVIDER_POLICY_DECISION_SCHEMA.to_owned(),
                provider_id: provider_id.to_owned(),
                operation: operation.to_owned(),
                credential_status,
                effect: ProviderPolicyEffect::Deny,
                reason: "provider_policy_invalid".to_owned(),
                matched_rule: None,
                policy_digest: self.policy_digest.clone(),
            };
        }
        if !bounded(provider_id, 128) || !bounded(operation, 128) {
            return ProviderPolicyDecision {
                schema: PROVIDER_POLICY_DECISION_SCHEMA.to_owned(),
                provider_id: provider_id.to_owned(),
                operation: operation.to_owned(),
                credential_status,
                effect: ProviderPolicyEffect::Deny,
                reason: "provider_request_invalid".to_owned(),
                matched_rule: None,
                policy_digest: self.policy_digest.clone(),
            };
        }
        let mut selected: Option<(u32, usize, &ProviderPolicyRule)> = None;
        for (index, rule) in self.rules.iter().enumerate() {
            if rule.matches(provider_id, operation)
                && selected.as_ref().is_none_or(|(precedence, previous, _)| {
                    (rule.precedence, index) > (*precedence, *previous)
                })
            {
                selected = Some((rule.precedence, index, rule));
            }
        }
        let effect = selected
            .map(|(_, _, rule)| rule.effect)
            .unwrap_or(self.default_effect);
        let (effect, reason) = if operation != PROVIDER_USE_OPERATION {
            (ProviderPolicyEffect::Deny, "provider_operation_denied")
        } else {
            match (effect, credential_status) {
                (ProviderPolicyEffect::Deny, _) => {
                    (ProviderPolicyEffect::Deny, "provider_policy_denied")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Configured) => {
                    (ProviderPolicyEffect::Allow, "provider_ready")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::ScopeInsufficient) => {
                    (ProviderPolicyEffect::Deny, "provider_scope_insufficient")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Missing) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_missing")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Expired) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_expired")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::ReauthRequired) => {
                    (ProviderPolicyEffect::Deny, "provider_reauth_required")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Revoked) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_revoked")
                }
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Unsupported) => (
                    ProviderPolicyEffect::Deny,
                    "provider_credential_backend_unsupported",
                ),
                (ProviderPolicyEffect::Allow, CredentialDisplayStatus::Unknown) => {
                    (ProviderPolicyEffect::Deny, "provider_credential_unknown")
                }
            }
        };
        ProviderPolicyDecision {
            schema: PROVIDER_POLICY_DECISION_SCHEMA.to_owned(),
            provider_id: provider_id.to_owned(),
            operation: operation.to_owned(),
            credential_status,
            effect,
            reason: reason.to_owned(),
            matched_rule: selected.map(|(_, _, rule)| rule.rule_id.clone()),
            policy_digest: self.policy_digest.clone(),
        }
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "authority_epoch": self.authority_epoch,
            "default_effect": self.default_effect,
            "rules": self.rules,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPolicyDecision {
    pub schema: String,
    pub provider_id: String,
    pub operation: String,
    pub credential_status: CredentialDisplayStatus,
    pub effect: ProviderPolicyEffect,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
    pub policy_digest: String,
}

impl ProviderPolicyDecision {
    pub fn allowed(&self) -> bool {
        self.effect == ProviderPolicyEffect::Allow
            && self.credential_status == CredentialDisplayStatus::Configured
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_POLICY_DECISION_SCHEMA
            || !bounded(&self.provider_id, 128)
            || !bounded(&self.operation, 128)
            || !bounded(&self.reason, 256)
            || !self.policy_digest.starts_with("sha256:")
            || self.policy_digest.len() != 71
        {
            return Err("provider_policy_decision_invalid".to_owned());
        }
        Ok(())
    }
}
