//! CO-30 原子基线发布（baseline publication）的 Core 适配器。
//!
//! ChangeRequest 的真实批准、CAS 和 Company/EventLog 事务仍由既有控制面负责。本文件只把
//! Domain 已验证的 `ChangeImpact` 与 `BaselinePublication` 交给同一份进程内 ledger：
//!
//! ```text
//! ChangeRequest / 旧 baseline 事实
//!        -> ChangeImpact（受影响 milestone/packet/run/review/acceptance/delivery）
//!        -> BaselinePublication（完整新 charter/plan/packet/criteria/budget/schedule）
//!        -> 【当前文件】Core adapter
//!        -> Domain ChangePublicationLedger 校验并幂等记录
//!        -> 上层再经既有 CAS/EventLog 提交
//! ```
//!
//! 这里不能把“记录到 ledger”描述成已经改变项目权威：它不执行 Company transition、不写
//! EventLog、不停止运行中的 Run、不自动使旧审批失效，也不替代跨进程 durable CAS。
use kiana_domain::{BaselinePublication, ChangeImpact, ChangePublicationLedger};

/// 把一份已经准备好的变更影响和完整基线发布交给 Domain ledger。
///
/// 【输入】
/// `impact` 必须列出受影响的 graph/approval/delivery 范围及旧/新 baseline 版本；`publication`
/// 必须携带与 impact 完全绑定的新 charter/plan/packet/criteria/budget/schedule 和显式失效
/// 引用。两者不是两个独立请求，Domain 会比较 change/project/version/packet 引用。
///
/// 【输出与失败】
/// 成功表示 ledger 接受该 source contract；相同 change ID 和 digest 的重复提交是幂等成功，
/// 旧版本冲突、部分发布、重复 drift 或摘要不匹配返回稳定错误。失败不会自动重试或回滚
/// Company 状态，调用方仍需由正常 CAS/EventLog 路径提交最终事实。
///
/// 【为什么只做适配】
/// 影响集合、版本推进和幂等冲突规则属于 Domain 单一事实源；Core 不复制规则，也不能把
/// 修改 Project.scope_baseline 字符串当成完整的 graph publication。
pub(crate) fn publish_change(
    // ledger 由调用方持有，避免本模块创建第二个全局变更事实源。
    ledger: &mut ChangePublicationLedger,
    impact: ChangeImpact,
    publication: BaselinePublication,
) -> Result<(), &'static str> {
    ledger.publish(impact, publication)
}
