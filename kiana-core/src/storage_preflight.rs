//! 存储根与项目信任预检的 Core 只读门面（facade），属于 `kiana-core`。
//!
//! 【系统位置与调用关系】
//! ```text
//! 上游准备 StoragePreflightRequest
//!     -> evaluate_storage_preflight()
//!     -> Domain StoragePreflightReport::evaluate()
//!     -> Ready/Blocked 报告交回上游
//! 下游可把报告交给 validate_storage_preflight_report() 再次检查结构
//! ```
//!
//! 这里仅转调 Domain 数据归约器并暴露报告校验，不读取项目配置、存储目录、文件身份或实时容量，也不写入 EventStore。
//! 当前没有发现这些 Core 门面的生产调用者；Daemon 的根路径解析器（root resolver）走独立路径。因此不要把函数存在或测试夹具
//! 通过说成真实部署 admission 已执行。

use kiana_domain::{StoragePreflightReport, StoragePreflightRequest};

/// 经 Core 边界把请求值转换为存储预检报告（storage preflight report）。
///
/// 【调用者/输入】预期由已取得服务端 ProjectTrust、StorageRoot 和 adapter 观测值的上层调用；参数是借用的
/// Domain request，本函数不会帮调用者采集任何输入。
///
/// 【输出/失败】成功返回 Domain 生成的 Ready/Blocked 报告；无效 wire/header/digest/path 输入时原样返回稳定
/// `String` 错误。遇到业务拒绝条件时通常不是 `Err`，而是成功得到带 reasons 的 Blocked report。
///
/// 【副作用/设计原因】直接委托 Domain reducer，便于 Core API 统一承接领域合同；不访问文件系统、重试、记录日志、
/// 消耗 approval、取得锁、创建目录或改变部署状态。报告只反映输入声明，不是实时容量/安全证明。
pub fn evaluate_storage_preflight(
    request: &StoragePreflightRequest,
) -> Result<StoragePreflightReport, String> {
    StoragePreflightReport::evaluate(request)
}

/// 对已经生成或从 wire 解码的 report 重新执行 Domain 结构校验。
///
/// 【输入/输出】借用 report；内部字段/排序/status/digest 一致返回 `Ok(())`，否则原样传递 Domain 错误字符串。
///
/// 此检查没有原始 request，也不会重新读取根路径、ProjectTrust、能力或容量事实；通过不能替代 effect 前的
/// 路径 no-follow/身份复查。本函数无存储、日志、重试或状态变更副作用。
pub fn validate_storage_preflight_report(report: &StoragePreflightReport) -> Result<(), String> {
    report.validate()
}
