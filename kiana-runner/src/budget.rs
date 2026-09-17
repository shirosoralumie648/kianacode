//! Bounded Harness budget configuration and task-chain accounting.
//!
//! The runner owns only the local per-task view.  ControlPlane/CP-11 remains the authority for
//! the durable lease and model admission.  This ledger prevents Continue and provider retries
//! from resetting the local counters before either path reaches the provider or Broker.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const HARNESS_BUDGET_SCHEMA: &str = "kiana.harness-budget.v1";
pub const DEFAULT_MAX_MODEL_STEPS_PER_TURN: u32 = 32;
pub const DEFAULT_MAX_ATTEMPTS_PER_TASK: u32 = 1_024;
pub const DEFAULT_MAX_TOOL_CALLS_PER_TASK: u32 = 1_024;
pub const DEFAULT_MAX_REPAIRS_PER_TASK: u32 = 64;
pub const DEFAULT_MAX_COMPACTIONS_PER_TASK: u32 = 64;
pub const DEFAULT_MAX_TOKENS_PER_TASK: u64 = 4_000_000;
const MAX_HARNESS_COUNTER: u32 = 1_000_000;
const MAX_HARNESS_TOKENS: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessBudgetSource {
    Default,
    Environment,
    Role,
}

impl HarnessBudgetSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Environment => "environment",
            Self::Role => "role",
        }
    }
}

/// Deployment-level limits.  Authority/role/packet limits are intersected by the existing
/// ControlPlane and ModelBudgetPort before a provider request is admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessBudgetConfig {
    pub source: HarnessBudgetSource,
    pub max_model_steps_per_turn: u32,
    pub max_attempts_per_task: u32,
    pub max_tool_calls_per_task: u32,
    pub max_repairs_per_task: u32,
    pub max_compactions_per_task: u32,
    pub max_tokens_per_task: u64,
    pub max_wall_time_per_task: Option<Duration>,
}

impl Default for HarnessBudgetConfig {
    fn default() -> Self {
        Self {
            source: HarnessBudgetSource::Default,
            max_model_steps_per_turn: DEFAULT_MAX_MODEL_STEPS_PER_TURN,
            max_attempts_per_task: DEFAULT_MAX_ATTEMPTS_PER_TASK,
            max_tool_calls_per_task: DEFAULT_MAX_TOOL_CALLS_PER_TASK,
            max_repairs_per_task: DEFAULT_MAX_REPAIRS_PER_TASK,
            max_compactions_per_task: DEFAULT_MAX_COMPACTIONS_PER_TASK,
            max_tokens_per_task: DEFAULT_MAX_TOKENS_PER_TASK,
            max_wall_time_per_task: None,
        }
    }
}

impl HarnessBudgetConfig {
    pub fn validate(self) -> Result<(), String> {
        if self.max_model_steps_per_turn == 0
            || self.max_attempts_per_task == 0
            || self.max_tool_calls_per_task == 0
            || self.max_repairs_per_task == 0
            || self.max_compactions_per_task == 0
            || self.max_tokens_per_task == 0
            || self
                .max_wall_time_per_task
                .is_some_and(|duration| duration.is_zero())
        {
            return Err("harness_budget_empty".to_owned());
        }
        if self.max_model_steps_per_turn > MAX_HARNESS_COUNTER
            || self.max_attempts_per_task > MAX_HARNESS_COUNTER
            || self.max_tool_calls_per_task > MAX_HARNESS_COUNTER
            || self.max_repairs_per_task > MAX_HARNESS_COUNTER
            || self.max_compactions_per_task > MAX_HARNESS_COUNTER
            || self.max_tokens_per_task > MAX_HARNESS_TOKENS
        {
            return Err("harness_budget_overflow".to_owned());
        }
        Ok(())
    }

    pub fn with_model_steps(mut self, steps: u32) -> Result<Self, String> {
        self.max_model_steps_per_turn = steps;
        self.validate().map(|()| self)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BudgetUsageSnapshot {
    pub model_attempts: u32,
    pub tool_calls: u32,
    pub repairs: u32,
    pub compactions: u32,
    pub reserved_tokens: u64,
    pub charged_tokens: u64,
    pub unknown_attempts: u32,
}

#[derive(Debug)]
struct BudgetUsage {
    model_attempts: u32,
    tool_calls: u32,
    repairs: u32,
    compactions: u32,
    reserved_tokens: u64,
    charged_tokens: u64,
    unknown_attempts: u32,
    started_at: Instant,
}

impl BudgetUsage {
    fn new() -> Self {
        Self {
            model_attempts: 0,
            tool_calls: 0,
            repairs: 0,
            compactions: 0,
            reserved_tokens: 0,
            charged_tokens: 0,
            unknown_attempts: 0,
            started_at: Instant::now(),
        }
    }

    fn snapshot(&self) -> BudgetUsageSnapshot {
        BudgetUsageSnapshot {
            model_attempts: self.model_attempts,
            tool_calls: self.tool_calls,
            repairs: self.repairs,
            compactions: self.compactions,
            reserved_tokens: self.reserved_tokens,
            charged_tokens: self.charged_tokens,
            unknown_attempts: self.unknown_attempts,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ModelAttemptReservation {
    scope: String,
    tokens: u64,
}

/// Process-local guard for the task-chain budget.  It intentionally has no authority to grant a
/// capability: all model requests still pass CP-11's `ModelBudgetPort`, and all tools still go to
/// the Broker.  A process restart loses this view; the EventLog facts remain the durable source.
#[derive(Debug, Default)]
pub(crate) struct BudgetLedger {
    usage: Mutex<HashMap<String, BudgetUsage>>,
}

impl BudgetLedger {
    fn with_usage<T>(
        &self,
        scope: &str,
        operation: impl FnOnce(&mut BudgetUsage) -> Result<T, String>,
    ) -> Result<T, String> {
        if scope.trim().is_empty() || scope.len() > 512 || scope.contains('\0') {
            return Err("harness_budget_scope_invalid".to_owned());
        }
        let mut usage = self
            .usage
            .lock()
            .map_err(|_| "harness_budget_lock_poisoned".to_owned())?;
        let account = usage
            .entry(scope.to_owned())
            .or_insert_with(BudgetUsage::new);
        operation(account)
    }

    fn check_account_time(
        account: &BudgetUsage,
        limits: HarnessBudgetConfig,
    ) -> Result<(), String> {
        if limits
            .max_wall_time_per_task
            .is_some_and(|limit| account.started_at.elapsed() >= limit)
        {
            return Err("budget_exceeded:task_wall_time".to_owned());
        }
        Ok(())
    }

    pub(crate) fn check_time(
        &self,
        scope: &str,
        limits: HarnessBudgetConfig,
    ) -> Result<(), String> {
        limits.validate()?;
        self.with_usage(scope, |account| Self::check_account_time(account, limits))
    }

    pub(crate) fn reserve_attempt(
        &self,
        scope: &str,
        limits: HarnessBudgetConfig,
        tokens: u64,
    ) -> Result<ModelAttemptReservation, String> {
        limits.validate()?;
        if tokens == 0 {
            return Err("budget_reservation_tokens_invalid".to_owned());
        }
        self.with_usage(scope, |account| {
            Self::check_account_time(account, limits)?;
            if account.model_attempts >= limits.max_attempts_per_task {
                return Err("budget_exceeded:model_attempts".to_owned());
            }
            let committed = account
                .charged_tokens
                .checked_add(account.reserved_tokens)
                .and_then(|value| value.checked_add(tokens))
                .ok_or_else(|| "budget_exceeded:tokens_overflow".to_owned())?;
            if committed > limits.max_tokens_per_task {
                return Err("budget_exceeded:tokens".to_owned());
            }
            account.model_attempts = account
                .model_attempts
                .checked_add(1)
                .ok_or_else(|| "budget_exceeded:model_attempts_overflow".to_owned())?;
            account.reserved_tokens = account
                .reserved_tokens
                .checked_add(tokens)
                .ok_or_else(|| "budget_exceeded:tokens_overflow".to_owned())?;
            Ok(ModelAttemptReservation {
                scope: scope.to_owned(),
                tokens,
            })
        })
    }

    pub(crate) fn release_attempt(
        &self,
        reservation: ModelAttemptReservation,
    ) -> Result<(), String> {
        self.with_usage(&reservation.scope, |account| {
            account.reserved_tokens = account
                .reserved_tokens
                .checked_sub(reservation.tokens)
                .ok_or_else(|| "budget_reservation_release_invalid".to_owned())?;
            account.model_attempts = account
                .model_attempts
                .checked_sub(1)
                .ok_or_else(|| "budget_reservation_release_invalid".to_owned())?;
            Ok(())
        })
    }

    pub(crate) fn settle_attempt(
        &self,
        reservation: ModelAttemptReservation,
        reported_tokens: Option<u64>,
    ) -> Result<(), String> {
        self.with_usage(&reservation.scope, |account| {
            account.reserved_tokens = account
                .reserved_tokens
                .checked_sub(reservation.tokens)
                .ok_or_else(|| "budget_settlement_reservation_missing".to_owned())?;
            let charged = reported_tokens.unwrap_or(reservation.tokens);
            if charged > reservation.tokens {
                return Err("budget_reported_usage_exceeds_reservation".to_owned());
            }
            account.charged_tokens = account
                .charged_tokens
                .checked_add(charged)
                .ok_or_else(|| "budget_exceeded:tokens_overflow".to_owned())?;
            if reported_tokens.is_none() {
                account.unknown_attempts = account
                    .unknown_attempts
                    .checked_add(1)
                    .ok_or_else(|| "budget_exceeded:unknown_attempts_overflow".to_owned())?;
            }
            Ok(())
        })
    }

    fn reserve_counter(
        &self,
        scope: &str,
        limits: HarnessBudgetConfig,
        field: &'static str,
        limit: u32,
        increment: impl FnOnce(&mut BudgetUsage) -> &mut u32,
    ) -> Result<(), String> {
        limits.validate()?;
        self.with_usage(scope, |account| {
            Self::check_account_time(account, limits)?;
            let counter = increment(account);
            if *counter >= limit {
                return Err(format!("budget_exceeded:{field}"));
            }
            *counter = counter
                .checked_add(1)
                .ok_or_else(|| format!("budget_exceeded:{field}_overflow"))?;
            Ok(())
        })
    }

    pub(crate) fn reserve_tool_call(
        &self,
        scope: &str,
        limits: HarnessBudgetConfig,
    ) -> Result<(), String> {
        self.reserve_counter(
            scope,
            limits,
            "tool_calls",
            limits.max_tool_calls_per_task,
            |usage| &mut usage.tool_calls,
        )
    }

    pub(crate) fn reserve_repair(
        &self,
        scope: &str,
        limits: HarnessBudgetConfig,
    ) -> Result<(), String> {
        self.reserve_counter(
            scope,
            limits,
            "repairs",
            limits.max_repairs_per_task,
            |usage| &mut usage.repairs,
        )
    }

    pub(crate) fn reserve_compaction(
        &self,
        scope: &str,
        limits: HarnessBudgetConfig,
    ) -> Result<(), String> {
        self.reserve_counter(
            scope,
            limits,
            "compactions",
            limits.max_compactions_per_task,
            |usage| &mut usage.compactions,
        )
    }

    pub(crate) fn snapshot(&self, scope: &str) -> Result<BudgetUsageSnapshot, String> {
        self.with_usage(scope, |account| Ok(account.snapshot()))
    }
}
