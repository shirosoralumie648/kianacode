//! CI-12 产品主链 deny-first 证据的 Core 只读门面。
//!
//! `Ci12ProductGate` 是对十种安全场景的已提交观察做合同校验：缺少认证、不受信项目、
//! 跨项目范围、过期审批、秘密泄露、TOCTOU 漂移、重放、结果未知、fake provider success
//! 和 live opt-in。真实链路仍是统一的 `DaemonHost -> ControlPlane -> handler/provider`；
//! 本文件不创建第二条 provider、审批或 capability 路径。
//!
//! ```text
//! 现有入口/fixture 形成 Ci12Case 列表
//!        -> Ci12ProductGate
//!        -> 【当前文件】Core 公共门面
//!        -> kiana-domain::Ci12ProductGate::validate
//!        -> CI/source guard 消费结构化拒绝或通过结果
//! ```
//!
//! 这里的 `Ok(())` 只表示输入证据符合 deny-first 合同，不表示 CLI/Web/Workbench/Desktop、
//! 外部 provider、durable restart 或 physical effect 已实际执行。尤其是 `LiveOptIn` 即使
//! 带有 operator approval 和 provider evidence，也不应在这个 source/fixture 门面里自动把
//! proof level 提升为 live。

use kiana_domain::Ci12ProductGate;

/// 校验一份 CI-12 产品主链证据矩阵。
///
/// 【调用者】
/// `kiana-core/src/lib.rs` 将本函数重新导出；当前源码调用边界主要是 CI-12 source guard
/// 和 Domain fixture。调用方应先从同一产品主链整理事实，再复用这里的 Domain 唯一规则，
/// 不要在入口/UI 中复制十场景判断。
///
/// 【输入】
/// `gate` 包含 seed、十类场景 case、handler/provider/effect 计数、secret/spine/receipt
/// 标志、可选 approval/provider evidence、limitations 和 canonical digest。本函数不读取
/// EventLog、不运行 provider、不发请求，也不替调用方补齐缺失 case。
///
/// 【成功与失败】
/// 成功表示 deny-first 场景没有 handler/provider/effect，Unknown 有 receipt 与限制，fake
/// success 没有伪造 live evidence，live opt-in 没有越过审批/证据门槛；失败返回 Domain 稳定
/// 错误码。错误只由调用方记录/展示，不会自动重试、批准、回滚或修改运行状态。
///
/// 【为什么保持委托】
/// scenario 集合、计数不变量和摘要属于 `kiana-domain` 的稳定合同。Core 只暴露架构位置上的
/// 访问门面，避免出现第二个 authority loop 或为了让 source fixture 通过而放宽拒绝路径。
pub fn validate_ci12_product_gate(gate: &Ci12ProductGate) -> Result<(), String> {
    gate.validate()
}
