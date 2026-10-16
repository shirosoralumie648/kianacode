//! DaemonHost 的部署观测 bundle 只读校验转发层，属于 `kiana-daemon`。
//!
//! 公开宿主方法 `DaemonHost::validate_observability()` 将调用转到本文件，再由 Core facade 委托 Domain 校验。
//! 此处只把 Domain `String` 错误映射为 `PortError::Failed`；没有 bundle 构造/采集、日志或指标写入、EventStore 查询、
//! trace 导出或 secret 清理。当前生产源码没有调用该 route 的调用者。

use kiana_domain::LifecycleEvidenceBundle;
use kiana_ports::PortError;

/// 校验调用方提交的 bundle，并将 Domain 错误包装为 Daemon 端口错误。
///
/// 【输入/输出】输入是外部构造的 `LifecycleEvidenceBundle`；通过时返回 `Ok(())`，格式/绑定检查失败时返回
/// `PortError::Failed`，其中保留 Core/Domain 的稳定错误文本。
/// 【副作用与限制】只读转发，不读取引用对象、不重新生成/脱敏 bundle、不写 EventStore，也不重试或记录日志；
/// 通过并不证明 bundle 对应真实采集或已持久化的观测事实。
pub(crate) fn validate(bundle: &LifecycleEvidenceBundle) -> Result<(), PortError> {
    kiana_core::validate_deployment_observability(bundle).map_err(PortError::Failed)
}
