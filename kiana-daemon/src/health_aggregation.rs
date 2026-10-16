//! DaemonHost 到 Core 部署健康聚合器的私有路由适配层。
//!
//! 上游 `DaemonHost::{deployment_health, drain_health, maintenance_health}` 将已收集的
//! `HealthAggregationInput` 交给这里；本文件不收集证据，只把 Domain/Core 错误翻译为
//! `PortError`，让 daemon 对外接口维持统一错误类型。`evaluate_probe()` 用于强制选定
//! drain 或 maintenance 探针，会先覆盖输入中的 probe 再重算 input digest，避免修改字段
//! 后仍携带旧摘要。下游 Core/Domain 只生成报告，不写存储、不探测外部服务，也不改变
//! admission。当前未发现这些 DaemonHost 方法有仓库内 CLI、UI、HTTP 或其他生产调用者。

use kiana_domain::{HealthAggregationInput, HealthAggregationReport, HealthProbeKind};
use kiana_ports::PortError;

/// 沿 DaemonHost → Core → Domain 路由一个已构造的健康聚合输入。
///
/// `input` 必须已经包含调用方收集的 typed facts 和与它们匹配的 digest。本函数不会
/// 补采 EventStore、Provider、lease 或 operation 数据；Core/Domain 返回的字符串校验
/// 错误统一转换成 `PortError::Failed`，不重试或改变系统状态。
pub(crate) fn evaluate(
    input: &HealthAggregationInput,
) -> Result<HealthAggregationReport, PortError> {
    kiana_core::aggregate_health(input).map_err(PortError::Failed)
}

/// 以指定 probe 评估健康输入，并为变更后的输入重新生成摘要。
///
/// Drain/Maintenance 入口不信任输入对象原先携带的 probe 值：若直接覆盖字段而保留旧
/// `input_digest`，Domain 会正确地拒绝摘要不匹配。因此这里在调用通用评估前同步重算
/// digest。该操作只修改本次调用持有的输入副本，不持久化状态；最终报告的 status 仍由
/// Domain 状态判定顺序决定，且不会真实关闭 admission。
pub(crate) fn evaluate_probe(
    mut input: HealthAggregationInput,
    probe: HealthProbeKind,
) -> Result<HealthAggregationReport, PortError> {
    input.probe = probe;
    input.input_digest = input.digest();
    evaluate(&input)
}
