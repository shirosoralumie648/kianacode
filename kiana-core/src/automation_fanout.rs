//! AUT-19 FanOut/FanIn 计划的 Core 只读门面。
//!
//! 领域 DTO 与字段校验位于 `kiana-domain::automation_fanout`；本文件只把 Core 调用方收到的计划交给
//! 领域校验器，并原样返回成功或稳定错误码：
//!
//! ```text
//! Core / adapter 调用方
//!     -> validate_fanout_plan(plan)
//!     -> kiana-domain::validate_automation_fanout(plan)
//!     -> Ok(()) 或 automation_fanout_plan_invalid
//! ```
//!
//! 当前未找到生产调用点。此门面不创建 workflow instance、不做权限检查、不拆分或并发执行 child、
//! 不分配 budget/scope、不写 EventLog，也不汇总 FanIn 结果。它与 `kiana-workflow::durable` 中已有的
//! DAG FanOut/FanIn 节点不是同一条实现路径；调用方不能把这里的 `Ok(())` 当作运行时动作已获准或已完成。

use kiana_domain::{validate_automation_fanout, AutomationFanoutPlan};

/// 对一个 AUT-19 FanOut 计划执行领域结构校验，并保留领域层的错误码。
///
/// 【调用者】当前未找到生产调用者；公开给 Core 内部模块或后续 adapter 使用。
///
/// 【输入】`plan` 是调用方已经构造好的父执行、子执行和约束声明；本函数不从数据库或 EventStore
/// 加载真实父执行，也不验证这些 ID 是否存在。
///
/// 【输出】计划字段符合领域 validator 规则时返回 `Ok(())`，否则返回静态错误码。
///
/// 【副作用与失败】只读且同步，不修改计划、不分配资源、不写日志或事件；失败直接透传，当前没有
/// 在此处转换成人类可见消息、重试或禁用主体的逻辑。
///
/// 【设计原因】Core facade 固定依赖方向：Core 复用底层 `kiana-domain` 的规则，避免再复制一份
/// 可能与领域 DTO 漂移的校验。它仍然只是静态校验入口，不是 durable workflow 的执行调度器。
pub fn validate_fanout_plan(plan: &AutomationFanoutPlan) -> Result<(), &'static str> {
    validate_automation_fanout(plan)
}
