//! Bounded Harness budget configuration and scope-local accounting.
//!
//! `BudgetLedger` 是 Runner 的内存预算视图，不授予模型或工具执行权限，也不写 EventLog。
//! ControlPlane 的模型准入与 Broker 授权仍是独立边界；本地预留成功不代表已获准执行。
//!
//! ## 调用与结算
//!
//! harness 在模型调用前用 `reserve_attempt()` 同时预留 token 并增加 attempt 计数。
//! 准入前被拒绝时，`release_attempt()` 释放这份预留并减回 attempt 计数。
//! 调用结束后，`settle_attempt()` 移除整份预留，再把报告的用量计入 `charged_tokens`；
//! 没有用量报告时按整份预留计费，并增加 `unknown_attempts`，不能假定消费为零。
//! 例如预留 100K、报告 50K，成功结算后预留减少 100K、已用增加 50K。
//!
//! 工具和压缩次数分别由 `reserve_tool_call()`、`reserve_compaction()` 检查。
//! `reserve_repair()` 提供修复次数记账接口，但当前 harness 没有调用它；有此字段不等于
//! 已实现自动修复。单 turn 的模型步数限制由 harness 检查，不由本账本单独维护。
//!
//! ## Scope 与持久性
//!
//! `HashMap<String, BudgetUsage>` 按调用方传入的 scope 记账。当前
//! `KianaHarness::budget_scope()` 使用 `project:<project_root>:role:<role_id>`，
//! 无 assignment 时角色为 `unassigned`；同一 harness 内同项目、同角色的多个 run
//! 会共享账户，Continue 和 provider 重试不会自动清空这些累计计数。
//!
//! 账户不包含于 `HarnessCheckpoint`，本模块也没有从 EventLog 重放账户的实现。
//! 新建 harness 会创建新的内存账本，不能把这里的计数当作跨进程的持久预算证明。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const HARNESS_BUDGET_SCHEMA: &str = "kiana.harness-budget.v1";

// ===== 预算上限常数（可配置但有默认值）=====
//
// DEFAULT_MAX_* 是 HarnessBudgetConfig 的默认值，不代表已获 ControlPlane 授权。
// daemon 可从环境或角色生成配置；harness::effective_budget 再与 run/assignment 限额取小值。
// 以下 *_per_task 计数和 token 上限实际作用于 budget_scope 返回的共享账户。
pub const DEFAULT_MAX_MODEL_STEPS_PER_TURN: u32 = 32; // 单轮最多 32 个模型 step，每步仍可能重试
pub const DEFAULT_MAX_ATTEMPTS_PER_TASK: u32 = 1_024; // 每个 scope 最多保留 1024 次 attempt 计数
pub const DEFAULT_MAX_TOOL_CALLS_PER_TASK: u32 = 1_024; // 每个 scope 最多预留 1024 次工具调用
pub const DEFAULT_MAX_REPAIRS_PER_TASK: u32 = 64; // 修复计数上限；当前调用链尚未使用
pub const DEFAULT_MAX_COMPACTIONS_PER_TASK: u32 = 64; // 每个 scope 最多预留 64 次压缩
pub const DEFAULT_MAX_TOKENS_PER_TASK: u64 = 4_000_000; // 每个 scope 预留加已用的默认 token 上限

// ===== 内部防护栏（防 overflow）=====
// 这两个常数不导出，只用于 validate() 防止配置数字太疯狂。
const MAX_HARNESS_COUNTER: u32 = 1_000_000; // 计数器不超过 100 万
const MAX_HARNESS_TOKENS: u64 = 64 * 1024 * 1024; // 配置上限为 67,108,864 个 token，不是字节数

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessBudgetSource {
    Default,     // 使用模块内的 DEFAULT_MAX_* 常数
    Environment, // 从环境变量覆盖
    Role,        // 从 Agent 的 Role 权限定义覆盖
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

/// Harness 本地预算配置，在构造时传入，执行时按值复制并收紧。
///
/// `KianaHarness::effective_budget()` 将步数、token 和挂钟限制与 run/assignment 中
/// 对应的限额取小值；`BudgetLedger` 按所得配置检查 scope 账户。
/// 这里不持有 API key 的月度配额或持久租约，本地通过不能替代 ControlPlane 准入。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessBudgetConfig {
    pub source: HarnessBudgetSource,
    pub max_model_steps_per_turn: u32,
    pub max_attempts_per_task: u32,
    pub max_tool_calls_per_task: u32,
    pub max_repairs_per_task: u32,
    pub max_compactions_per_task: u32,
    pub max_tokens_per_task: u64,
    pub max_wall_time_per_task: Option<Duration>, // 挂钟时间上限（可选）
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
    /// 验证配置的合法性。
    ///
    /// 检查项：
    /// 1. 所有上限都 > 0（没有意义的「0 步」或「0 token」上限）
    /// 2. 所有计数器都 ≤ MAX_HARNESS_COUNTER
    /// 3. token 总数 ≤ MAX_HARNESS_TOKENS
    /// 4. 如果配置了 max_wall_time_per_task，则它不能是 Duration::ZERO
    ///
    /// 返回 Err 意味着配置根本不合法，harness 会拒绝启动。
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

/// 预算使用快照：某个 scope 的内存计数副本。
///
/// `invoke_model()` 在结算后调用 `snapshot()`，将字段放入 ModelTurn 的
/// `harness_budget` metadata。该结构不包含于 HarnessCheckpoint，也不实现预算恢复。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BudgetUsageSnapshot {
    pub model_attempts: u32,   // 已预留且未被 release_attempt 撤回的 attempt 次数
    pub tool_calls: u32,       // 已预留的工具调用次数，不等同于执行成功次数
    pub repairs: u32,          // 已预留的修复次数；当前 harness 未调用 reserve_repair
    pub compactions: u32,      // 已预留的压缩次数
    pub reserved_tokens: u64,  // 当前预留（已承诺但未最终结账）的 token 数
    pub charged_tokens: u64,   // 已结算用量；缺少报告时包含按整份预留计入的量
    pub unknown_attempts: u32, // 缺少 token 用量报告而保守结算的 attempt 次数，不是重试额度
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

/// 进程内、按 scope 共享的预算账户。
///
/// 本对象只做本地限额检查和记账，不能授予能力。模型准入与工具的 ControlPlane/Broker
/// 路径独立于它；本地计数拒绝也不会被外层许可覆盖。
///
/// 内部 Mutex 使账户操作串行，但不提供事务回滚。账本不序列化、不重放 EventLog，
/// 不能据此声称进程重启后仍保留累计预算。
#[derive(Debug, Default)]
pub(crate) struct BudgetLedger {
    // scope 由 harness 提供，当前按 project_root + role_id 共享。
    usage: Mutex<HashMap<String, BudgetUsage>>,
}

impl BudgetLedger {
    /// 校验 scope，在 Mutex 保护下获取或创建账户，再执行闭包。
    ///
    /// 账户仍留在 HashMap 中；闭包返回 Err 时，已做的写入不会自动回滚。
    /// scope 为空、超过 512 字节或含 NUL 返回 `harness_budget_scope_invalid`；
    /// 锁中毒返回 `harness_budget_lock_poisoned`，其余错误直接来自闭包。
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

    /// 检查从账户创建起累计的挂钟时间是否已达到配置上限。
    ///
    /// elapsed >= limit 时返回 `budget_exceeded:task_wall_time`。这里只在调用时检查，
    /// 不启动计时器，也不主动取消进行中的 I/O；调用方仍需自己的 deadline/取消控制。
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
