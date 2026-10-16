//! BQ-18 fallback（故障切换）在 Core 中的重新准入边界。
//!
//! # 文件职责与系统位置
//!
//! 上游模型请求第一次尝试失败或需要换路由时，fallback 不能只是 provider 内部的“再试一次”。
//! 它必须创建一个新的 attempt，并重新核对当前 authority、数据边界、预算、凭据、allowlist
//! 路由和价格事实：
//!
//! ```text
//! 原始请求 / primary attempt
//!        -> DaemonHost / ControlPlane
//!        -> 本文件：重新调用 Domain admit_fallback_attempt
//!        -> 新 attempt + 新 reservation + 新 opaque permit 的 admission
//!        -> 既有 Broker / ProviderGateway 做最终 permit/effect-time 检查
//!        -> EventLog / receipt 由现有主链负责
//! ```
//!
//! `kiana-domain::fallback_admission` 是字段和 deny-first 规则的事实源；本文件只把它放在
//! Core 的 ControlPlane 边界上。它不调用 provider、Broker、网络、EventLog writer，也不启动
//! 隐藏重试循环。调用方必须先提交正常 permit/reservation 事实，再把这里的 admission 交给
//! 既有 dispatch 路径。
//!
//! # 不能继承 primary attempt 的原因
//!
//! primary 可能已经消耗预算、持有一次性 permit、处于 Unknown 或拥有一份不适用于新 route
//! 的 receipt。如果 fallback 复用这些对象，系统会把一次新的外部动作伪装成旧动作的重试，
//! 破坏幂等、预算结算和审计边界。因此 Domain admission 必须比较 authority/data/price/route
//! 快照，并返回新的 attempt、reservation 和 permit 摘要。

use super::ControlPlane;
use kiana_domain::{
    admit_fallback_attempt, FallbackAdmissionCandidate, FallbackAdmissionRequest,
    FallbackAttemptAdmission, FallbackRouteAllowlist,
};

/// ControlPlane fallback 重新准入的类型门面。
///
/// 该空类型本身不保存状态；它把“重新准入必须从 Core authority 边界经过”表达在公共 API
/// 上，同时让 Domain 函数可以被 deny-first fixture 独立复用。它不是 provider client、permit
/// store 或重试器，也不代表 admission 已经被 dispatch。
pub struct ControlPlaneFallbackAdmission;

impl ControlPlaneFallbackAdmission {
    /// 根据当前请求、候选 route 和 allowlist 创建一个新的 fallback admission。
    ///
    /// 【输入】
    /// - `request`：服务端拥有的原始 run/attempt、authority epoch、数据边界、预算、primary
    ///   reservation 和 permit 摘要；
    /// - `candidate`：候选 provider/model/connection、当前 authority/data/budget、凭据版本、
    ///   新 reservation、allowlist route 和 rate card；
    /// - `allowlist`：精确绑定 provider、connection、model 和 route digest 的允许列表；
    /// - `attempt_id`：为 fallback 分配的新尝试 ID。
    ///
    /// 【输出与失败】
    /// 成功返回 `FallbackAttemptAdmission`，供既有 Broker/Provider 路径继续检查。候选发生
    /// authority/data drift、能力降级、预算/凭据/价格缺失、route 不在 allowlist，或复用了
    /// primary reservation/permit 时返回 Domain 稳定错误码；此处不会自动选另一个候选或重试。
    ///
    /// 【副作用】
    /// 这是纯值校验和构造，不调用 provider、不写 EventLog、不消费审批，也不提交实际预算。
    /// 持久化 reservation/permit 事实必须由上层正常 ControlPlane 事务完成。
    pub fn re_admit(
        request: &FallbackAdmissionRequest,
        candidate: &FallbackAdmissionCandidate,
        allowlist: &FallbackRouteAllowlist,
        attempt_id: kiana_domain::AttemptId,
    ) -> Result<FallbackAttemptAdmission, String> {
        // 这里是本模块唯一的准入调用。它只生成新的 attempt admission，不触发任何 adapter。
        admit_fallback_attempt(request, candidate, allowlist, attempt_id)
    }
}

impl ControlPlane {
    /// 通过与 primary attempt 相同的 ControlPlane authority 边界重新检查一个 allowlisted fallback。
    ///
    /// 【调用者】
    /// 由已完成当前请求上下文和候选快照整理的 dispatch/adapter 调用；它应位于真正分发到
    /// ProviderGateway 之前。Core 的 `lib.rs` 会重导出 `ControlPlaneFallbackAdmission`，但
    /// 当前函数只是准备 admission，不会替调用方执行模型请求。
    ///
    /// 【输入/输出】
    /// 参数含义与 `ControlPlaneFallbackAdmission::re_admit` 相同。成功返回一个与 primary
    /// attempt 分离的新 admission；失败向上返回 Domain 的结构化字符串错误，不会静默降级。
    ///
    /// 【重要边界】
    /// 返回成功不等于 provider 已接受请求。Broker/Provider 仍必须检查 opaque permit、effect
    /// 时点的资源/凭据状态和现有 dispatch 规则；不要在这里添加 provider 调用或第二个 fallback
    /// 循环，否则会绕过统一的 EventLog/receipt 事实链。
    pub fn re_admit_model_fallback(
        &self,
        request: &FallbackAdmissionRequest,
        candidate: &FallbackAdmissionCandidate,
        allowlist: &FallbackRouteAllowlist,
        attempt_id: kiana_domain::AttemptId,
    ) -> Result<FallbackAttemptAdmission, String> {
        let _ = self;
        ControlPlaneFallbackAdmission::re_admit(request, candidate, allowlist, attempt_id)
    }
}
