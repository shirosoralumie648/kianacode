//! SC-40：故障期间系统必须保持的容量与资源封套（capacity envelope）。
//!
//! BQ-26 负责注入故障并记录各个 seam 的结果，例如 reservation 是否泄漏；SC-40 继续回答
//! “故障发生时，队列、输出、延迟和预算是否仍然有界”。一次操作最终返回错误，并不能证明
//! 中间没有无限排队、输出膨胀、延迟失控、429 后继续花费预算或时钟回拨导致的假延迟。
//!
//! ```text
//! fault 发生
//!     ↓  CapacityFaultSample（故障期间以及恢复后的观察值）
//! CapacityBudget（调用方明确声明的上限）
//!     ↓  evaluate_capacity_fault
//! CapacityFaultReport —— 在封套内，或返回第一个被打破的边界
//! ```
//!
//! # 为什么上限是输入而不是隐藏常量
//!
//! 没有被写入证据的上限就无法审计。`CapacityBudget` 由调用方提供，先用 digest 封存，再把
//! 同一个 budget digest 带入 report，使报告能够回答“具体按哪一组上限判断”。零值表示没有
//! 声明有效上限，会被拒绝，而不是被解释成无限大。
//!
//! # 为什么恢复必须单独检查
//!
//! 系统可能在故障期间满足所有上限，却在故障结束后留下未释放 reservation：队列已经排空、
//! 延迟恢复正常，但 429 对应的 attempt 仍然占着预算。这种泄漏无法由故障进行中的样本发现，
//! 所以 `reservations_after_recovery` 是独立字段；只要非零，即使其他数字都在预算内也拒绝。
//!
//! 本模块只计算和绑定调用方提供的观察值；它不注入故障、不测量系统、不启动恢复动作。

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::bq26_fault_harness::{Bq26FaultCase, Bq26FaultClass};

pub const CAPACITY_FAULT_REPORT_SCHEMA: &str = "kiana.capacity-fault-report.v1";
pub const CAPACITY_FAULT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 故障期间系统声明必须保持的容量上限。
///
/// 每个数值都是上限而不是目标值。零表示没有声明有效封套，因此 fail-closed 拒绝；它不
/// 表示无限。预算先生成 `budget_digest`，报告必须绑定同一个摘要。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityBudget {
    /// 允许的最大延迟（毫秒）。
    pub max_latency_ms: u64,
    /// 允许的最大队列深度。
    pub max_queue_depth: u32,
    /// 允许的最大输出/缓冲字节数。
    pub max_bytes: u64,
    /// 是否必须观察到反压（backpressure），而不是默默吸收负载。
    pub require_backpressure: bool,
    /// 对四个预算字段重算的内容摘要。
    pub budget_digest: String,
}

impl CapacityBudget {
    /// 创建并封存一组容量上限；不读取系统，也不证明上限真的被执行。
    pub fn new(
        max_latency_ms: u64,
        max_queue_depth: u32,
        max_bytes: u64,
        require_backpressure: bool,
    ) -> Self {
        let mut value = Self {
            max_latency_ms,
            max_queue_depth,
            max_bytes,
            require_backpressure,
            budget_digest: String::new(),
        };
        value.budget_digest = value.digest();
        value
    }

    /// 计算不包含自身 digest 的预算 canonical digest。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "max_latency_ms": self.max_latency_ms,
            "max_queue_depth": self.max_queue_depth,
            "max_bytes": self.max_bytes,
            "require_backpressure": self.require_backpressure,
        }))
    }

    /// 校验预算摘要和三个数值上限都已明确声明。
    ///
    /// 预算错误会在比较 sample 前失败，因为没有可信的上限就不能解释任何观察值。
    fn validate(&self) -> Result<(), String> {
        if self.budget_digest != self.digest() {
            return Err("capacity_fault_budget_digest_mismatch".to_owned());
        }
        if self.max_latency_ms == 0 || self.max_queue_depth == 0 || self.max_bytes == 0 {
            return Err("capacity_fault_budget_unbounded".to_owned());
        }
        Ok(())
    }
}

/// 一次故障期间的观察值，以及恢复后仍然残留的资源状态。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityFaultSample {
    /// 对应 BQ-26 的具体 fault case。
    pub case: Bq26FaultCase,
    /// 故障族（provider HTTP、disk full、clock fault 等）。
    pub family: Bq26FaultClass,
    /// 观察到的延迟（毫秒）。
    pub observed_latency_ms: u64,
    /// 观察到的最大队列深度。
    pub observed_queue_depth: u32,
    /// 观察到的输出/缓冲字节数。
    pub observed_bytes: u64,
    /// 是否观察到系统施加反压。
    pub backpressure_applied: bool,
    /// 宣布故障已处理时仍持有的 reservation 数。
    pub leaked_reservations: u32,
    /// 宣布故障已处理时仍持有的 lease 数。
    pub leaked_leases: u32,
    /// 操作员是否真正执行了恢复步骤；未执行不能声称已恢复。
    pub recovered: bool,
    /// 恢复后仍持有的 reservation 数；唯一可接受值是 0。
    pub reservations_after_recovery: u32,
    /// 故障期间时钟是否回拨；回拨会使所有延迟数字失去意义。
    pub clock_rollback_detected: bool,
}

impl CapacityFaultSample {
    /// 组装一个观察样本；构造不做预算比较或恢复动作。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case: Bq26FaultCase,
        family: Bq26FaultClass,
        observed_latency_ms: u64,
        observed_queue_depth: u32,
        observed_bytes: u64,
        backpressure_applied: bool,
        leaked_reservations: u32,
        leaked_leases: u32,
        recovered: bool,
        reservations_after_recovery: u32,
        clock_rollback_detected: bool,
    ) -> Self {
        Self {
            case,
            family,
            observed_latency_ms,
            observed_queue_depth,
            observed_bytes,
            backpressure_applied,
            leaked_reservations,
            leaked_leases,
            recovered,
            reservations_after_recovery,
            clock_rollback_detected,
        }
    }
}

/// 绑定预算和观察样本后的封套判断结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityFaultReport {
    /// 固定 report schema。
    pub schema: String,
    /// 可兼容的 report 版本。
    pub version: SchemaVersion,
    /// 对应故障 case。
    pub case: Bq26FaultCase,
    /// 对应故障族。
    pub family: Bq26FaultClass,
    /// 绑定的预算摘要，防止报告脱离原预算解释。
    pub budget_digest: String,
    /// 被接受的延迟观察。
    pub observed_latency_ms: u64,
    /// 被接受的队列深度观察。
    pub observed_queue_depth: u32,
    /// 被接受的字节数观察。
    pub observed_bytes: u64,
    /// 被接受的反压观察。
    pub backpressure_applied: bool,
    /// 被接受的恢复后 reservation 数。
    pub reservations_after_recovery: u32,
    /// 报告无法证明的内容；当前实现始终写入限制说明。
    pub limitations: Vec<String>,
    /// 对报告字段重算的 canonical digest。
    pub report_digest: String,
}

impl CapacityFaultReport {
    /// 判断报告是否没有限制说明。
    ///
    /// 当前构造函数总会写入 limitations，因此正常 report 不会返回 true；保留这个方法是
    /// 为调用方提供清晰的“封套内”展示条件，而不是把 source sample 当作实测证明。
    pub fn within_envelope(&self) -> bool {
        self.limitations.is_empty()
    }

    /// 确认报告逐字段绑定到给定 sample 和 budget，并校验 report digest。
    ///
    /// 这一步防止调用方把 A 样本、B 预算和 C 报告拼在一起。它不重新执行测量，也不验证
    /// sample 来源；来源可信度和真实 durable 状态仍由上层负责。
    pub fn validate_against(
        &self,
        sample: &CapacityFaultSample,
        budget: &CapacityBudget,
    ) -> Result<(), String> {
        budget.validate()?;
        if self.schema != CAPACITY_FAULT_REPORT_SCHEMA
            || !self.version.is_compatible_with(&CAPACITY_FAULT_VERSION)
            || self.case != sample.case
            || self.family != sample.family
            || self.budget_digest != budget.budget_digest
            || self.observed_latency_ms != sample.observed_latency_ms
            || self.observed_queue_depth != sample.observed_queue_depth
            || self.observed_bytes != sample.observed_bytes
            || self.backpressure_applied != sample.backpressure_applied
            || self.reservations_after_recovery != sample.reservations_after_recovery
        {
            return Err("capacity_fault_report_binding_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("capacity_fault_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算不包含自身 digest 的 report canonical digest。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "case": self.case,
            "family": self.family,
            "budget_digest": self.budget_digest,
            "observed_latency_ms": self.observed_latency_ms,
            "observed_queue_depth": self.observed_queue_depth,
            "observed_bytes": self.observed_bytes,
            "backpressure_applied": self.backpressure_applied,
            "reservations_after_recovery": self.reservations_after_recovery,
            "limitations": self.limitations,
        }))
    }
}

/// 按固定顺序把一个故障样本与一组容量预算比较，并生成绑定报告。
///
/// 【为什么检查顺序不能随便改】
/// 1. 先验证 budget；没有可信上限，后续数值比较没有意义；
/// 2. 先拒绝时钟回拨，因为时钟不可信时延迟数字全部失去证据价值；
/// 3. 再按队列、输出字节、延迟、反压检查容量封套；
/// 4. 再检查故障族特有约束、故障期间泄漏、provider 429 后继续消费；
/// 5. 最后检查是否真正恢复以及恢复后 reservation 是否为零。
///
/// 【输入/输出】
/// 输入是调用方提供的 sample 和 sealed budget；成功返回带 limitations 的 report，失败返
/// 回第一个稳定错误码。函数不测量、不注入、不释放 reservation、不重试 provider。
pub fn evaluate_capacity_fault(
    sample: &CapacityFaultSample,
    budget: &CapacityBudget,
) -> Result<CapacityFaultReport, String> {
    budget.validate()?;

    if sample.clock_rollback_detected {
        return Err("capacity_fault_clock_rollback".to_owned());
    }
    if sample.observed_queue_depth > budget.max_queue_depth {
        return Err("capacity_fault_queue_unbounded".to_owned());
    }
    if sample.observed_bytes > budget.max_bytes {
        return Err("capacity_fault_output_flood".to_owned());
    }
    if sample.observed_latency_ms > budget.max_latency_ms {
        return Err("capacity_fault_latency_unbounded".to_owned());
    }
    if budget.require_backpressure && !sample.backpressure_applied {
        return Err("capacity_fault_backpressure_missing".to_owned());
    }
    // 磁盘已满却没有反压，说明负载被吸收到样本看不到的地方；不能把这种情况当作恢复。
    if sample.family == Bq26FaultClass::DiskFull && !sample.backpressure_applied {
        return Err("capacity_fault_backpressure_missing".to_owned());
    }
    if sample.leaked_reservations > 0 || sample.leaked_leases > 0 {
        return Err("capacity_fault_reservation_leak".to_owned());
    }
    // provider 在 429/5xx 后仍继续消费属于“超额”场景，使用专用错误码保留根因，而不是
    // 折叠成普通 reservation leak。
    if sample.family == Bq26FaultClass::ProviderHttp && sample.leaked_reservations > 0 {
        return Err("capacity_fault_provider_quota_exceeded".to_owned());
    }
    // ClockFault 若没有实际回拨，就不能证明任何延迟异常；必须同时观察到回拨，避免把未发生
    // 的时钟故障写成证据。
    if sample.family == Bq26FaultClass::ClockFault && !sample.clock_rollback_detected {
        return Err("capacity_fault_clock_rollback_unproven".to_owned());
    }
    // 恢复有独立条件；声明 recovered 后仍有 reservation 必须拒绝。
    if sample.recovered && sample.reservations_after_recovery > 0 {
        return Err("capacity_fault_reservation_leak_after_recovery".to_owned());
    }
    if !sample.recovered {
        return Err("capacity_fault_recovery_not_established".to_owned());
    }

    let mut report = CapacityFaultReport {
        schema: CAPACITY_FAULT_REPORT_SCHEMA.to_owned(),
        version: CAPACITY_FAULT_VERSION,
        case: sample.case,
        family: sample.family,
        budget_digest: budget.budget_digest.clone(),
        observed_latency_ms: sample.observed_latency_ms,
        observed_queue_depth: sample.observed_queue_depth,
        observed_bytes: sample.observed_bytes,
        backpressure_applied: sample.backpressure_applied,
        reservations_after_recovery: sample.reservations_after_recovery,
        limitations: vec![
            "no latency, queue depth or byte count in this report was measured".to_owned(),
            "the sample and the budget were both supplied; neither was observed here".to_owned(),
        ],
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report.validate_against(sample, budget)?;
    Ok(report)
}
