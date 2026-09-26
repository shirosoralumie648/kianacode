//! Bounded runtime-admission facts for role symposiums.
//!
//! The Runner remains the only model loop. This contract only accounts for finite limits and
//! decides whether a board may be closed; it does not dispatch a model or publish a decision.
use serde::{Deserialize, Serialize};

pub const SYMPOSIUM_RUN_BUDGET_SCHEMA: &str = "kiana.symposium-run-budget.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymposiumRunBudget {
    pub schema: String,
    pub max_rounds: u32,
    pub max_messages: u32,
    pub max_tokens: u64,
    pub max_wall_time_ms: u64,
    pub max_stall_rounds: u32,
    pub rounds_used: u32,
    pub messages_used: u32,
    pub tokens_used: u64,
    pub stall_rounds: u32,
    pub cancelled: bool,
}

impl SymposiumRunBudget {
    pub fn new(
        max_rounds: u32,
        max_messages: u32,
        max_tokens: u64,
        max_wall_time_ms: u64,
        max_stall_rounds: u32,
    ) -> Result<Self, &'static str> {
        let budget = Self {
            schema: SYMPOSIUM_RUN_BUDGET_SCHEMA.to_owned(),
            max_rounds,
            max_messages,
            max_tokens,
            max_wall_time_ms,
            max_stall_rounds,
            rounds_used: 0,
            messages_used: 0,
            tokens_used: 0,
            stall_rounds: 0,
            cancelled: false,
        };
        budget.validate()?;
        Ok(budget)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SYMPOSIUM_RUN_BUDGET_SCHEMA
            || self.max_rounds == 0
            || self.max_messages == 0
            || self.max_tokens == 0
            || self.max_wall_time_ms == 0
            || self.max_stall_rounds == 0
            || self.rounds_used > self.max_rounds
            || self.messages_used > self.max_messages
            || self.tokens_used > self.max_tokens
            || self.stall_rounds > self.max_stall_rounds
        {
            return Err("symposium_run_budget_invalid");
        }
        Ok(())
    }

    pub fn record_round(&mut self, made_progress: bool) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        if self.rounds_used >= self.max_rounds {
            return Err("symposium_round_budget_exhausted");
        }
        self.rounds_used += 1;
        if made_progress {
            self.stall_rounds = 0;
        } else {
            self.stall_rounds += 1;
        }
        if self.stall_rounds > self.max_stall_rounds {
            return Err("symposium_stall_limit_exhausted");
        }
        Ok(())
    }

    pub fn record_message(&mut self, token_estimate: u64) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        if self.messages_used >= self.max_messages
            || token_estimate > self.max_tokens.saturating_sub(self.tokens_used)
        {
            return Err("symposium_message_budget_exhausted");
        }
        self.messages_used += 1;
        self.tokens_used += token_estimate;
        Ok(())
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn decision_allowed(&self) -> Result<(), &'static str> {
        self.validate()?;
        if self.cancelled {
            return Err("symposium_cancelled");
        }
        if self.rounds_used >= self.max_rounds
            || self.messages_used >= self.max_messages
            || self.tokens_used >= self.max_tokens
            || self.stall_rounds >= self.max_stall_rounds
        {
            return Err("symposium_budget_exhausted");
        }
        Ok(())
    }
}
