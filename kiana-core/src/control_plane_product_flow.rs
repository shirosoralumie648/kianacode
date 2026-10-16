//! CP-30 产品流程证据在 `kiana-core` 中的只读门面。
//!
//! # 文件职责与系统位置
//!
//! CP-30 要求能够对三类产品流程的“已提交证据”做一致性检查：低风险只读流程、审批后
//! 写入流程，以及取消/恢复流程。真正运行这些流程的主链仍然是：
//!
//! ```text
//! CLI / Workbench / Web / Desktop
//!        -> 同一个 DaemonHost
//!        -> ControlPlane 授权、关卡和审批
//!        -> KianaHarness / CapabilityBroker
//!        -> EventStore / Receipt
//!        -> 形成 Cp30ProductFlowBundle
//!        -> 【当前文件】只读校验 bundle
//! ```
//!
//! `Cp30ProductFlowBundle` 的结构、四界面一致性和三种流程不变量定义在
//! `kiana-domain/src/control_plane_product_flow.rs`。本文件只把 Domain 的校验能力暴露给
//! Core 的公共 API（`kiana-core::validate_control_plane_product_bundle`），方便入口、CI
//! fixture 或其他 Core 适配层在消费证据前执行同一份检查。
//!
//! # 重要边界
//!
//! 这里接收的是已经构造好的观察结果，不会自己创建 `DaemonHost`、启动 Runner、发送模型
//! 请求、调用 Broker effect、消费审批或写 EventLog。校验成功只说明 bundle 的 schema、摘要、
//! surface parity 和流程计数满足 Domain 契约，不等于流程已经在 live/physical 环境运行，
//! 也不等于外部副作用已经发生。CI 中的 fake model/effect fixture 仍然只能提供 source 级
//! 证据，不能伪装成 durable、live 或 physical 证明。
//!
//! 这样保持一个薄门面很重要：如果在这里新增第二个执行循环或“自动批准”分支，CLI、Web、
//! Desktop 等入口就可能拥有不同的事实来源，绕开唯一的 `ControlPlane` 授权链。

use kiana_domain::Cp30ProductFlowBundle;

/// 校验一份 CP-30 产品流程证据包。
///
/// 【调用者】
/// `kiana-core/src/lib.rs` 将本函数重新导出为 Core 公共入口；当前实际消费方主要是
/// CP-30 的 CI/source guard 和 Domain fixture。未来任何需要读取该证据包的适配层，都应先
/// 通过这里复用 Domain 校验，而不是自行复制一套审批、收据或界面一致性判断。
///
/// 【输入】
/// `bundle` 是由 `kiana_domain::Cp30ProductFlowBundle` 表示的已提交证据集合。它应包含：
/// 1. `ReadOnly`；2. `ApprovalWrite`；3. `CancelRecovery`，且每个流程都要为 CLI、Workbench、
/// Web、Desktop 提供共享 flow ID、authority digest、event cursor、终态和 receipt digest。
/// 本函数不从磁盘、网络或 EventStore 加载数据，也不替调用方补齐缺少字段。
///
/// 【输出】
/// - `Ok(())`：bundle 通过 Domain 层的 schema、canonical digest、流程种类唯一性、四界面
///   parity 以及各流程特有不变量。
/// - `Err(String)`：返回稳定的 Domain 错误码，例如 bundle 不完整、surface 不一致、只读流程
///   带 effect、审批写入缺少已消费审批/receipt，或取消恢复没有事实恢复证明。Core 不会在此
///   把错误压成通用 I/O 错误。
///
/// 【副作用与失败处理】
/// 本函数是纯只读委托：不会修改 `bundle`、写事件、重试、禁用账号或执行恢复。错误由调用
/// 方决定是拒绝当前证据、记录 guard 失败，还是把结构化错误继续返回给上层；返回 `Ok` 也不
/// 会自动生成 receipt 或提升 proof level。
///
/// 【为什么直接委托】
/// 流程状态、surface 集合和 canonical digest 属于稳定领域契约，应由 `kiana-domain` 作为
/// 单一事实源定义。Core 只提供架构位置上的访问门面，避免在 Core 复制一份容易与 Domain
/// 漂移的规则，更不能为了让 fixture 通过而放宽 Domain 校验。
pub fn validate_control_plane_product_bundle(bundle: &Cp30ProductFlowBundle) -> Result<(), String> {
    bundle.validate()
}
