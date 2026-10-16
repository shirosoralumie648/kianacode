//! 旧版运行用量汇总与 Company 预算/配额值对象。
//!
//! 本文件包含两类容易因名字相近而混淆的合同：`UsageRecord` / `CostLedger` 汇总运行事件里的模型 token
//! 用量；`RuntimeBudget` / `ProjectBudget` / `Quota` 则表示单次运行、项目和范围级的上限。它们不是
//! `billing_usage.rs` 中带来源、置信度和 observation sequence 的新 `NormalizedUsage`，也不是按费率计价的
//! 财务流水。
//!
//! ```text
//! RuntimeEvent(run.model_turn, attempted=true)
//!     -> Core receipts 投影出 UsageRecord[]
//!     -> CostLedger::from_records() 汇总 input/output token 总量
//!     -> cost_micros 保持未知，等待版本化价格与完整 usage 证据
//!
//! Company budget policy
//!     -> RuntimeBudget / ProjectBudget / Quota
//!     -> Core 或 Company aggregate 在申请 run/packet 前检查 reservation
//! ```
//!
//! 上游负责从事件/Company policy 提供可信的 ID 和已占用额度；本文件只做纯值汇总与上限比较，不读取事件库、
//! 不预留资源、不累计并发状态、不计算真实费用。Core/Company 调用点位于 `receipts.rs`、`company.rs`、
//! `model_budget.rs` 等 adapter/aggregate；它们当前在工作区有其他改动，本轮只读核对。

use crate::RunId;
use serde::{Deserialize, Serialize};

/// 从一条已记录的模型调用事件抽取出的旧版运行用量行。
///
/// Core receipts projector 只把 `run.model_turn` 且 `attempted=true` 的事件转换成该行；运行 ID 由投影调用者
/// 提供，request/event ID 来自事件，缺失的 provider/model/input/output 会继续以 None 表示未知。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageRecord {
    /// 来源 `RuntimeEvent` 的稳定事件 ID。
    pub event_id: crate::EventId,
    /// 发起该模型调用的请求 ID。
    pub request_id: crate::RequestId,
    /// 所属运行 ID，由 Core 投影上下文传入。
    pub run_id: RunId,
    /// 在 run.model_turn 数据中的步骤序号；投影缺失时旧路径会填 0。
    pub step: u32,
    /// 可选 Provider 标签；事件中没有该字段时保留为空。
    pub provider_id: Option<String>,
    /// 可选模型标签；未记录时保留为空。
    pub model_id: Option<String>,
    /// 本次调用报告的输入 token；None 表示事件没有可用数值。
    pub input_tokens: Option<u64>,
    /// 本次调用报告的输出 token；None 表示事件没有可用数值。
    pub output_tokens: Option<u64>,
    /// 事件记录的运行耗时，单位毫秒；旧投影缺字段时会写 0。
    pub elapsed_ms: u64,
}

/// 多条旧版 UsageRecord 的只读汇总视图。
///
/// 该类型的 `CostLedger` 名称不表示它是逐笔财务账本：这里保留原始 records 并折叠 token 总数，真实金额
/// 直到有完整用量和版本化价格源才可计算。新 billing 路径使用不同的 CostLedgerEntry 合同。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CostLedger {
    /// 参与本次汇总的每一条调用用量记录。
    pub records: Vec<UsageRecord>,
    /// 所有记录的输入 token 之和；任一值缺失、累计溢出或记录集为空时为 None。
    pub input_tokens: Option<u64>,
    /// 所有记录的输出 token 之和；任一值缺失、累计溢出或记录集为空时为 None。
    pub output_tokens: Option<u64>,
    /// 当前保持未知；缺费率/完整 usage 不能当作免费或明确为 0。
    pub cost_micros: Option<u64>,
}
impl CostLedger {
    /// 保留明细并分别折叠 input/output token 总数。
    ///
    /// 【调用者】Core `cost_ledger_from_events()` 在 run receipt 投影阶段。
    /// 【输入/输出】接收已筛选的 UsageRecord 列表；成功总是返回 ledger 值，无法完整求和的某一维独立为 None。
    /// 【计算规则】空列表结果为 None；对非空列表用 checked_add 累计，任一条记录的该维度为 None 或发生
    /// 溢出时，try_fold 整体变成 None，而不是把缺失项按 0 跳过。
    /// 【设计原因】跳过未知记录会产出比真实值更小、却看起来完整的总数；金额也不能在缺少版本费率时伪造。
    /// 【副作用】只创建内存投影视图，不写账本事实。
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

/// 单次运行的资源上限，通常由 Company policy 或 run 入口组合得到。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBudget {
    /// 最多可发起的模型调用数。
    pub max_model_calls: u64,
    /// 所有模型调用合计最多消耗的 token 数。
    pub max_tokens: u64,
    /// run 可占用的最长墙钟时间，单位毫秒。
    pub max_wall_time_ms: u64,
}

/// 一个项目级的 run 数量与 token 预算。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectBudget {
    /// 预算归属项目 ID。
    pub project_id: crate::ProjectId,
    /// 该项目允许的最大 run 总数。
    pub max_runs: u64,
    /// 该项目允许的 token 总上限。
    pub max_tokens: u64,
}

/// 某个业务 scope 可消耗的模型调用/token/并发上限。
///
/// 此 DTO 只保存规则上限，不维护已使用量；调用者须先把当前保留量与新申请量合并，再传入检查函数。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    /// 配额对应的范围标签；本方法只检查非空，范围与 project/identity 的绑定由上层做。
    pub scope: String,
    /// 同一计费/授权范围下允许的模型调用总数。
    pub model_calls: u64,
    /// 同一范围下允许消耗的 token 总数。
    pub tokens: u64,
    /// 同一时刻允许处于活动状态的最大并发数。
    pub concurrency: u32,
}

impl RuntimeBudget {
    /// 检查三个单 run 上限都大于零。
    ///
    /// 【调用者】Core 将 Company 配置转换为本次 run budget 后；公司创建路径也会在保存前调用。
    /// 【失败】任一值为 0 时返回 `runtime_budget_empty`，调用方将其映射为预算配置冲突/拒绝。
    /// 【边界】只检查非零，不比较三者的合理比例，也不在此预留预算。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.max_model_calls == 0 || self.max_tokens == 0 || self.max_wall_time_ms == 0 {
            return Err("runtime_budget_empty");
        }
        Ok(())
    }
}
impl ProjectBudget {
    /// 检查再开启一个 run、并把其预计 token 数加入现有保留额后是否越过项目限额。
    ///
    /// 【输入】当前已保留 run 数、当前已保留 token 数、下一 run 需要的 token 预算。
    /// 【成功】当前 run 数严格小于 max_runs，且 token 加法不溢出、不超过 max_tokens。
    /// 【失败】run 名额耗尽、加法溢出或 token 超限均返回 `project_budget_exhausted`。
    /// 【容易误改】这里检查的是“再加一个 run”的预留前置条件，不修改任何累计值；预留写入/并发竞争必须由
    /// 上层原子状态迁移负责。达到 token 上限本身允许（比较是 `>`），超过才拒绝。
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
    /// 将调用方已聚合的 reservation 用量与一个 quota 上限比较。
    ///
    /// 【输入】本次合并后所需的模型调用、token 和并发总量。
    /// 【失败】scope 为空或任何维度高于上限时返回 `quota_exhausted`。
    /// 【边界】不查 scope 所属人/项目，不累加当前用量，也不原子预留名额；Company/调度调用方必须先确定
    /// 输入的 scope 与身份匹配，并负责并发安全的占额流程。
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
