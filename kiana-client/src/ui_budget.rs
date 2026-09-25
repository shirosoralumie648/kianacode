//! Shared UI resource budgets and explicit degradation decisions.
//!
//! This is a read-only budget contract. It does not evict server facts, cancel a run, or change
//! the authoritative EventLog. Pending and Unknown items are protected from silent truncation.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const UI_RESOURCE_BUDGET_SCHEMA: &str = "kiana.ui-resource-budget.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiBudgetSurface {
    Feed,
    Snapshot,
    Artifact,
    Dom,
    TtyBuffer,
    Ipc,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiResourceBudget {
    pub schema: String,
    pub surface: UiBudgetSurface,
    pub max_bytes: u64,
    pub max_items: u64,
    pub max_sessions: u64,
    pub max_queue_depth: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiResourceUsage {
    pub schema: String,
    pub bytes: u64,
    pub items: u64,
    pub sessions: u64,
    pub queue_depth: u64,
    pub pending_items: u64,
    pub unknown_items: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiBudgetDecision {
    Accept,
    Degraded { reason: String },
    Reject { reason: String },
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum UiBudgetError {
    #[error("ui_budget_schema_invalid")]
    SchemaInvalid,
    #[error("ui_budget_limit_invalid")]
    LimitInvalid,
    #[error("ui_budget_usage_invalid")]
    UsageInvalid,
    #[error("ui_budget_protected_items_exceed_limit")]
    ProtectedItemsExceedLimit,
}

impl UiResourceBudget {
    pub fn validate(&self) -> Result<(), UiBudgetError> {
        if self.schema != UI_RESOURCE_BUDGET_SCHEMA {
            return Err(UiBudgetError::SchemaInvalid);
        }
        if self.max_bytes == 0
            || self.max_items == 0
            || self.max_sessions == 0
            || self.max_queue_depth == 0
        {
            return Err(UiBudgetError::LimitInvalid);
        }
        Ok(())
    }
}

impl UiResourceUsage {
    pub fn validate(&self) -> Result<(), UiBudgetError> {
        if self.schema != UI_RESOURCE_BUDGET_SCHEMA
            || self.pending_items.checked_add(self.unknown_items).is_none()
            || self.pending_items + self.unknown_items > self.items
        {
            return Err(UiBudgetError::UsageInvalid);
        }
        Ok(())
    }
}

pub fn evaluate_budget(
    budget: &UiResourceBudget,
    usage: &UiResourceUsage,
) -> Result<UiBudgetDecision, UiBudgetError> {
    budget.validate()?;
    usage.validate()?;
    if usage
        .pending_items
        .checked_add(usage.unknown_items)
        .is_none()
        || usage.pending_items + usage.unknown_items > budget.max_items
    {
        return Err(UiBudgetError::ProtectedItemsExceedLimit);
    }
    if usage.bytes > budget.max_bytes || usage.queue_depth > budget.max_queue_depth {
        return Ok(UiBudgetDecision::Reject {
            reason: "hard_resource_bound_exceeded".to_owned(),
        });
    }
    if usage.items > budget.max_items || usage.sessions > budget.max_sessions {
        return Ok(UiBudgetDecision::Degraded {
            reason: "bounded_view_requires_hydrate_or_pagination".to_owned(),
        });
    }
    Ok(UiBudgetDecision::Accept)
}
