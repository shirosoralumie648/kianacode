//! AUT-19 bounded parent-child workflow fan-out/fan-in source contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const AUTOMATION_FANOUT_SCHEMA: &str = "kiana.automation-fanout.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFanoutPlan {
    pub schema: String,
    pub parent_execution_id: String,
    pub child_execution_ids: Vec<String>,
    pub max_depth: u32,
    pub depth: u32,
    pub max_concurrency: u32,
    pub requested_concurrency: u32,
    pub parent_budget_units: u64,
    pub child_budget_units: u64,
    pub ttl_ms: u64,
    pub parent_ttl_remaining_ms: u64,
    pub child_scope_is_subset: bool,
    pub parent_id_in_children: bool,
    pub fail_fast: bool,
    pub fan_in_required: bool,
    pub plan_digest: String,
}

impl AutomationFanoutPlan {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != AUTOMATION_FANOUT_SCHEMA
            || !valid_text(&self.parent_execution_id)
            || self.child_execution_ids.is_empty()
            || self.child_execution_ids.len() > 32
            || self.child_execution_ids.iter().any(|v| !valid_text(v))
            || self
                .child_execution_ids
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.child_execution_ids.len()
            || self.max_depth == 0
            || self.depth >= self.max_depth
            || self.max_concurrency == 0
            || self.requested_concurrency == 0
            || self.requested_concurrency > self.max_concurrency
            || self.requested_concurrency > self.child_execution_ids.len() as u32
            || self.child_budget_units == 0
            || self
                .child_budget_units
                .saturating_mul(self.child_execution_ids.len() as u64)
                > self.parent_budget_units
            || self.ttl_ms == 0
            || self.ttl_ms > self.parent_ttl_remaining_ms
            || !self.child_scope_is_subset
            || self.parent_id_in_children
            || !self.fan_in_required
            || !valid_digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("automation_fanout_plan_invalid");
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "parent_execution_id": self.parent_execution_id,
            "child_execution_ids": self.child_execution_ids,
            "max_depth": self.max_depth,
            "depth": self.depth,
            "max_concurrency": self.max_concurrency,
            "requested_concurrency": self.requested_concurrency,
            "parent_budget_units": self.parent_budget_units,
            "child_budget_units": self.child_budget_units,
            "ttl_ms": self.ttl_ms,
            "parent_ttl_remaining_ms": self.parent_ttl_remaining_ms,
            "child_scope_is_subset": self.child_scope_is_subset,
            "parent_id_in_children": self.parent_id_in_children,
            "fail_fast": self.fail_fast,
            "fan_in_required": self.fan_in_required,
        }))
    }
}

pub fn validate_automation_fanout(plan: &AutomationFanoutPlan) -> Result<(), &'static str> {
    plan.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.contains(['\0', '\n', '\r'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
