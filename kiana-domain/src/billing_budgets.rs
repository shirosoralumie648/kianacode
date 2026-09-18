//! Pure intersection of runtime, project, quota, lease and provider budget limits.
//!
//! The intersection is a conservative admission snapshot. It cannot grant a capability, charge a
//! bill or widen a child lease; FinancialBudget is intentionally not an input to execution limits.

use crate::{json_digest, BudgetLease, ProjectBudget, Quota, RuntimeBudget, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const PROVIDER_BUDGET_SCHEMA: &str = "kiana.provider-budget.v1";
pub const EFFECTIVE_BUDGET_SCHEMA: &str = "kiana.effective-budget.v1";
pub const BUDGET_INTERSECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderBudget {
    pub schema: String,
    pub version: SchemaVersion,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_selector: Option<String>,
    pub max_requests: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_micros: Option<u128>,
    pub max_concurrency: u32,
    pub queue_limit: u32,
    pub budget_digest: String,
}

impl ProviderBudget {
    pub fn new(provider_id: impl Into<String>, max_requests: u64, max_concurrency: u32) -> Self {
        let mut budget = Self {
            schema: PROVIDER_BUDGET_SCHEMA.to_owned(),
            version: BUDGET_INTERSECTION_VERSION,
            provider_id: provider_id.into(),
            model_selector: None,
            max_requests,
            max_input_tokens: None,
            max_output_tokens: None,
            max_cost_micros: None,
            max_concurrency,
            queue_limit: 0,
            budget_digest: String::new(),
        };
        budget.budget_digest = budget.digest();
        budget
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_BUDGET_SCHEMA
            || !self
                .version
                .is_compatible_with(&BUDGET_INTERSECTION_VERSION)
            || self.provider_id.trim().is_empty()
            || self.provider_id.len() > 256
            || self.model_selector.as_deref().is_some_and(|model| {
                model.trim().is_empty() || model.len() > 256 || model.contains('\0')
            })
            || self.max_requests == 0
            || self.max_concurrency == 0
            || self.budget_digest != self.digest()
            || !valid_digest(&self.budget_digest)
        {
            return Err("provider_budget_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "provider_id": self.provider_id,
            "model_selector": self.model_selector,
            "max_requests": self.max_requests,
            "max_input_tokens": self.max_input_tokens,
            "max_output_tokens": self.max_output_tokens,
            "max_cost_micros": self.max_cost_micros,
            "max_concurrency": self.max_concurrency,
            "queue_limit": self.queue_limit,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveBudget {
    pub schema: String,
    pub version: SchemaVersion,
    pub max_model_calls: u64,
    pub max_tool_calls: u64,
    pub max_tokens: u64,
    pub max_wall_time_ms: u64,
    pub max_effects: u32,
    pub max_concurrency: u32,
    pub max_project_runs: u64,
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_max_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_max_cost_micros: Option<u128>,
    pub intersection_digest: String,
}

impl EffectiveBudget {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EFFECTIVE_BUDGET_SCHEMA
            || !self
                .version
                .is_compatible_with(&BUDGET_INTERSECTION_VERSION)
            || self.max_model_calls == 0
            || self.max_tool_calls == 0
            || self.max_tokens == 0
            || self.max_wall_time_ms == 0
            || self.max_effects == 0
            || self.max_concurrency == 0
            || self.max_project_runs == 0
            || self.provider_id.trim().is_empty()
            || !valid_digest(&self.intersection_digest)
            || self.intersection_digest != self.digest()
        {
            return Err("effective_budget_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "max_model_calls": self.max_model_calls,
            "max_tool_calls": self.max_tool_calls,
            "max_tokens": self.max_tokens,
            "max_wall_time_ms": self.max_wall_time_ms,
            "max_effects": self.max_effects,
            "max_concurrency": self.max_concurrency,
            "max_project_runs": self.max_project_runs,
            "provider_id": self.provider_id,
            "model_selector": self.model_selector,
            "provider_max_input_tokens": self.provider_max_input_tokens,
            "provider_max_output_tokens": self.provider_max_output_tokens,
            "provider_max_cost_micros": self.provider_max_cost_micros,
        }))
    }
}

pub fn intersect_budgets(
    runtime: &RuntimeBudget,
    lease: &BudgetLease,
    project: &ProjectBudget,
    quota: &Quota,
    provider: &ProviderBudget,
) -> Result<EffectiveBudget, String> {
    runtime.validate().map_err(str::to_owned)?;
    lease.validate().map_err(str::to_owned)?;
    if quota.scope.trim().is_empty()
        || quota.model_calls == 0
        || quota.tokens == 0
        || quota.concurrency == 0
    {
        return Err("quota_invalid".to_owned());
    }
    if project.max_runs == 0 || project.max_tokens == 0 {
        return Err("project_budget_invalid".to_owned());
    }
    provider.validate()?;
    let max_model_calls = min_nonzero(
        runtime.max_model_calls,
        lease.model_call_limit(),
        quota.model_calls,
        provider.max_requests,
    )?;
    let max_tokens = min_nonzero(
        runtime.max_tokens,
        lease.max_tokens,
        project.max_tokens,
        quota.tokens,
    )?;
    let max_wall_time_ms = runtime.max_wall_time_ms.min(lease.max_wall_clock_ms);
    let max_concurrency = lease
        .max_concurrency
        .min(quota.concurrency)
        .min(provider.max_concurrency);
    if max_wall_time_ms == 0 || max_concurrency == 0 {
        return Err("budget_intersection_empty".to_owned());
    }
    let effective = EffectiveBudget {
        schema: EFFECTIVE_BUDGET_SCHEMA.to_owned(),
        version: BUDGET_INTERSECTION_VERSION,
        max_model_calls,
        max_tool_calls: lease.max_tool_calls,
        max_tokens,
        max_wall_time_ms,
        max_effects: lease.max_effects,
        max_concurrency,
        max_project_runs: project.max_runs,
        provider_id: provider.provider_id.clone(),
        model_selector: provider.model_selector.clone(),
        provider_max_input_tokens: provider.max_input_tokens,
        provider_max_output_tokens: provider.max_output_tokens,
        provider_max_cost_micros: provider.max_cost_micros,
        intersection_digest: String::new(),
    };
    let mut effective = effective;
    effective.intersection_digest = effective.digest();
    effective.validate()?;
    Ok(effective)
}

pub fn derive_child_lease(
    parent: &BudgetLease,
    requested: &BudgetLease,
) -> Result<BudgetLease, String> {
    parent.validate().map_err(str::to_owned)?;
    requested.validate().map_err(str::to_owned)?;
    if requested.max_tool_calls > parent.max_tool_calls
        || requested.model_call_limit() > parent.model_call_limit()
        || requested.max_tokens > parent.max_tokens
        || requested.max_wall_clock_ms > parent.max_wall_clock_ms
        || requested.max_concurrency > parent.max_concurrency
        || requested.max_effects > parent.max_effects
        || requested.max_reserved_budget > parent.reservation_limit_for_intersection()
    {
        return Err("budget_child_widening_rejected".to_owned());
    }
    if requested.tool_calls_used > parent.tool_calls_used
        || requested.model_calls_used > parent.model_calls_used
        || requested.tokens_used > parent.tokens_used
        || requested.effects_used > parent.effects_used
    {
        return Err("budget_child_usage_widening_rejected".to_owned());
    }
    Ok(requested.clone())
}

fn min_nonzero(values: u64, second: u64, third: u64, fourth: u64) -> Result<u64, String> {
    let value = values.min(second).min(third).min(fourth);
    (value > 0)
        .then_some(value)
        .ok_or_else(|| "budget_intersection_empty".to_owned())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
