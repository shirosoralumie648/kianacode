//! DEP-14 生命周期观测 bundle 的 Core 校验门面，属于 `kiana-core`。
//!
//! 【静态调用链】外部调用者 → `DaemonHost::validate_observability()` → Daemon helper → 本文件 →
//! `LifecycleEvidenceBundle::validate()`。
//!
//! Core 仅委托 Domain DTO 校验，不读取 EventStore、metrics registry、trace store、日志或 audit event。源码没有生产 bundle
//! 生产者或校验调用者；这个 facade 是可调用的只读边界，不是观测采集器、脱敏器或导出器。

use kiana_domain::LifecycleEvidenceBundle;

/// 检查 lifecycle evidence bundle 的字段关系和摘要自洽性。
///
/// 【调用者】Daemon `deployment_observability::validate()` 转发至此；当前生产源码没有调用 DaemonHost route 的调用者。
/// 【输入/输出】借用 caller supplied bundle；符合 Domain 合同时返回 `Ok(())`，首个结构、secret marker、嵌套 metric 或 digest
/// 错误以原始稳定 `String` 返回。
/// 【副作用与限制】只调用 `bundle.validate()`；不会认证字段来源、检查 EventLog/trace/log 对象、执行脱敏或发出 telemetry。
pub fn validate_deployment_observability(bundle: &LifecycleEvidenceBundle) -> Result<(), String> {
    bundle.validate()
}
