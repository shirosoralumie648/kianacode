//! DaemonHost 到 Core 部署准入纯计算的内部转发层，属于 `kiana-daemon`。
//!
//! 上层通过 `DaemonHost::deployment_admission()` 提交已整理好的 `DeploymentAdmissionInput`，调用链为：
//! `DaemonHost` → 本文件 `evaluate()` → Core `evaluate_admission()` → Domain `DeploymentAdmissionDecision::evaluate()`。
//! 本层负责把库级 `String` 校验错误映射到 Daemon 端口错误类型；不负责读取事实源或真正拦截新工作。
//! 当前生产源码未找到宿主方法调用者，也没有 scheduler/ControlPlane 消费 decision 的边，因此这是可调用的 additive route，
//! 不是已经启用的 admission gate。

use kiana_domain::{DeploymentAdmissionDecision, DeploymentAdmissionInput};
use kiana_ports::PortError;

/// 经 Core 计算 admission decision，并把 Domain 的字符串错误转换为 Daemon 端口错误。
///
/// 【调用者】公开的 `DaemonHost::deployment_admission()` 调用此 helper；目前没有生产代码调用该宿主方法。
/// 【输入】由上游组装并借用传入的 Domain input。此函数不补全 health、lease、epoch、operation 或时间事实。
/// 【输出】input 合法时返回 decision；即使 decision 的状态为 Blocked、Paused 或 Unknown，仍是成功返回值，
/// 调用者必须检查状态及 `allow_new_work`/`allow_existing_work`。input/摘要校验失败时，Core 的 `String` 被包装为
/// `PortError::Failed`。
/// 【副作用与错误边界】只有错误类型映射；不读写文件/EventLog、不取租约、不暂停 intake、不停止 operation、不重试、
/// 不记日志，也不自行将正常拒绝升级为 `PortError`。
pub(crate) fn evaluate(
    input: &DeploymentAdmissionInput,
) -> Result<DeploymentAdmissionDecision, PortError> {
    kiana_core::evaluate_admission(input).map_err(PortError::Failed)
}
