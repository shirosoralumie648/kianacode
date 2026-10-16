//! 部署监督请求结果的 Core 校验门面，属于 `kiana-core`。
//!
//! 【系统位置与数据流】
//! ```text
//! 未来的部署协调器
//!     -> 构造 SupervisorRequest
//!     -> SupervisorPort / 平台 adapter
//!     -> 收到 SupervisorObservation
//!     -> 本文件 validate_supervisor_observation()
//!     -> Domain validate_against() 检查请求绑定和结果状态合同
//! ```
//!
//! 本文件只把 Core 层调用转交给 Domain 值对象，不读取 lease、EventStore、服务目录或时钟，也不发起操作。
//! 当前检索未发现这个 facade 的生产调用者；它是待接线的校验边界，不表示真实 supervisor 已进入部署流程。

use kiana_domain::{SupervisorObservation, SupervisorRequest};

/// 检查 adapter 返回的 observation 是否回答了原始 supervisor request。
///
/// 【作用】为未来消费平台监督结果的 Core 协调流程提供统一入口；当前函数只委托 Domain 的
/// `SupervisorObservation::validate_against()`。
///
/// 【调用者】设计上应由持有原始请求的部署协调器，在 adapter 返回结果后调用；目前没有仓库内生产调用点。
///
/// 【输入】`request` 是交给 adapter 的原请求；`observation` 是 adapter 返回的结果声明。两者都由调用方提供，
/// 本函数不会自行查找或重建它们。
///
/// 【输出与失败】合同匹配时返回 `Ok(())`；schema、自摘要、请求身份绑定、动作/结果组合或代次检查失败时，
/// 原样返回 Domain 的稳定 `String` 错误。此函数不把错误转换成成功，也不在这里决定重试、禁用 adapter 或向用户展示什么。
///
/// 【副作用与限制】只读借用输入并执行确定性校验；不读写 EventStore/lease、不调用 `SupervisorPort`、不记录日志，
/// 也不验证 observation 是否来自真实操作系统。调用方仍需将错误映射到部署 operation 的恢复/报告流程。
pub fn validate_supervisor_observation(
    request: &SupervisorRequest,
    observation: &SupervisorObservation,
) -> Result<(), String> {
    observation.validate_against(request)
}
