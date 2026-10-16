//! 部署 operation lease CAS 合同的 Core 只读门面，属于 `kiana-core`。
//!
//! 【系统位置与调用关系】
//! ```text
//! 上层恢复/部署适配器或诊断调用者
//!     -> validate_operation_lease_cas()
//!     -> kiana-domain::OperationLeaseCas::validate()
//!     -> 原样返回合同错误
//! ```
//!
//! 本文件只把 Domain 的确定性值校验暴露在 Core 边界，没有租约获取、心跳、到期回收、释放或 effect
//! 执行逻辑。仓库源码中能看到 `restore_activation.rs` 构造和获取 CAS lease，但该模块调用者目前只
//! 出现在测试；本 facade 自身没有发现生产调用点。因此不要把这个 facade 的存在描述成已经接入
//! DaemonHost/ControlPlane 的部署写入路径。

use kiana_domain::OperationLeaseCas;

/// 通过 Core 边界检查调用方提供的 operation lease CAS 快照。
///
/// 【输入】由调用者已取得或解码的 `OperationLeaseCas` 借用；本函数不查询数据库、文件或系统时钟。
///
/// 【输出/失败】Domain 校验成功时返回 `Ok(())`；schema、身份、活动 lease 绑定或摘要不一致时，
/// 原样返回 Domain 稳定错误字符串。错误由调用方决定如何转换为 API/恢复结果；本门面不重试、
/// 不记录日志、不追加 EventStore 事实，也不改变 lease/CAS。
///
/// 【为什么保留门面】让 Core 调用者依赖核心层导出的合同入口，同时把不含副作用的值规则留在
/// `kiana-domain`。通过检查只说明当前快照满足 Domain 已实现的结构规则，不构成跨进程 CAS 或 effect-time fence 证明。
pub fn validate_operation_lease_cas(cas: &OperationLeaseCas) -> Result<(), String> {
    cas.validate()
}
