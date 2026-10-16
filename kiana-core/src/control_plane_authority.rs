//! CP-29 ControlPlane authority/crash 证据的 Core 只读门面。
//!
//! # 文件职责与真实调用链
//!
//! CP-29 的领域合同检查一份已经整理好的 authority scenario：其中包含 command facts、崩溃
//! 观察、在线/重放 fold digest，以及权限、预算、生命周期、幂等和资源 fencing 证据。规范上
//! 这些事实来自现有的 `ControlPlane -> TransitionBatch -> EventStore/Journal` 链路；当前文件
//! 只接收调用方组装好的场景并把它交给 `kiana-domain`：
//!
//! ```text
//! ControlPlane / TransitionBatch / JournalState 产生已提交事实
//!        -> fixture 或证据适配层组装 Cp29AuthorityScenario
//!        -> 【当前文件】validate_control_plane_authority_scenario
//!        -> kiana-domain::Cp29AuthorityScenario::validate
//!        -> Ok 或稳定错误码
//! ```
//!
//! `kiana-core/src/lib.rs` 重新导出这个函数；当前源码中的实际使用主要是 CP-29 source
//! guard/CI fixture 边界，不能把 crate 根导出推断成已经接入生产执行循环。
//!
//! # 为什么这个文件必须保持为薄门面
//!
//! 本文件不创建 `DaemonHost`/`ControlPlane`，不启动 Runner，不调用 Broker、provider 或
//! EventStore，不注入 crash/kill，也不补造 authority 事实。这样 CP-29 的状态、command fact、
//! crash point 集合和摘要规则只在 Domain 维护一份；若在 Core 复制一套规则，入口可能在
//! 两处得到不同结论，甚至通过“校验”绕过真实授权链。
//!
//! 校验成功只代表传入证据满足结构合同，不能证明真实双主机并发、子进程重启、存储延迟、
//! durable replay 或物理副作用已经发生。当前 baseline 的 proof 上限是 source/fixture。

use kiana_domain::Cp29AuthorityScenario;

/// 校验一份 CP-29 ControlPlane authority 场景证据。
///
/// 【调用者】
/// `kiana-core/src/lib.rs` 将它作为 crate 根 API 暴露；当前仓库可见的实际调用边界是
/// CP-29 source guard/fixture。未来的证据消费方应复用这里的 Domain 校验，而不是在 Core、
/// entrypoint 或 UI 中另写权限/崩溃恢复判断。
///
/// 【输入】
/// `scenario` 是借用的 `Cp29AuthorityScenario`，应已包含 schema、command facts、crash
/// observations、连续序列、在线/重放 fold digest 和 canonical digest。函数不会从 EventStore
/// 或进程读取缺失事实，也不会替调用方补齐七个 crash point。
///
/// 【输出与失败】
/// `Ok(())` 表示 Domain 层确认权限等级/预算只能收紧、生命周期转换合法、terminal 不复活、
/// 同 command 重放不增加 effect、未确认 effect 保持 `ResultUnknown + resource_fenced`、
/// crash 矩阵完整且在线/重放摘要一致。`Err(String)` 返回稳定原因码，例如场景头无效、摘要
/// 漂移或 crash matrix 不完整；Core 不把它压成普通 I/O 错误。
///
/// 【副作用】
/// 本函数是纯只读委托：不会拒绝正在运行的请求、回滚事件、禁用账号、重试、注入 fault、
/// 写 EventStore 或执行补偿。失败如何记录/展示由调用方决定；成功也不提升 proof level，
/// 更不等于 live、durable 或 physical 结果。
///
/// 【为什么委托 Domain】
/// authority 状态机、摘要输入和错误码属于稳定领域契约。Core 只负责暴露架构位置上的访问
/// 点，以避免校验逻辑分叉；不要为了让 fixture 变绿而放宽 Domain 的 fail-closed 规则。
pub fn validate_control_plane_authority_scenario(
    scenario: &Cp29AuthorityScenario,
) -> Result<(), String> {
    scenario.validate()
}
