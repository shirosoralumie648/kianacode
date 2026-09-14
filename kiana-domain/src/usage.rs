//! Runtime usage receipts are distinct from project budgets and financial spend.
use crate::RunId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageRecord {
    pub event_id: crate::EventId,
    pub request_id: crate::RequestId,
    pub run_id: RunId,
    pub step: u32,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub elapsed_ms: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CostLedger {
    pub records: Vec<UsageRecord>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Unknown until a versioned price source and complete provider usage exist.
    pub cost_micros: Option<u64>,
}
impl CostLedger {
    pub fn from_records(records: Vec<UsageRecord>) -> Self {
        let input_tokens = if records.is_empty() {
            None
        } else {
            records
                .iter()
                .try_fold(0u64, |sum, r| sum.checked_add(r.input_tokens?))
        };
        let output_tokens = if records.is_empty() {
            None
        } else {
            records
                .iter()
                .try_fold(0u64, |sum, r| sum.checked_add(r.output_tokens?))
        };
        Self {
            records,
            input_tokens,
            output_tokens,
            cost_micros: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBudget {
    pub max_model_calls: u64,
    pub max_tokens: u64,
    pub max_wall_time_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectBudget {
    pub project_id: crate::ProjectId,
    pub max_runs: u64,
    pub max_tokens: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    pub scope: String,
    pub model_calls: u64,
    pub tokens: u64,
    pub concurrency: u32,
}

impl RuntimeBudget {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.max_model_calls == 0 || self.max_tokens == 0 || self.max_wall_time_ms == 0 {
            return Err("runtime_budget_empty");
        }
        Ok(())
    }
}
impl ProjectBudget {
    pub fn check_reservation(
        &self,
        reserved_runs: u64,
        reserved_tokens: u64,
        next_tokens: u64,
    ) -> Result<(), &'static str> {
        if reserved_runs >= self.max_runs
            || reserved_tokens
                .checked_add(next_tokens)
                .is_none_or(|n| n > self.max_tokens)
        {
            return Err("project_budget_exhausted");
        }
        Ok(())
    }
}
impl Quota {
    pub fn check_reservation(
        &self,
        model_calls: u64,
        tokens: u64,
        concurrency: u32,
    ) -> Result<(), &'static str> {
        if self.scope.trim().is_empty()
            || model_calls > self.model_calls
            || tokens > self.tokens
            || concurrency > self.concurrency
        {
            return Err("quota_exhausted");
        }
        Ok(())
    }
}
