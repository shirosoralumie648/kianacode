//! DEP-05 部署 operation lifecycle 的 Core 只读门面。
//!
//! 【预期数据流】
//! ```text
//! 已提交 EventStore / JournalFrame facts
//!     -> 上游读取器整理 operation ID、revision 和 transitions
//!     -> 本文件的 validate_operation_journal / replay_operation_journal
//!     -> kiana-domain::OperationJournal 校验或确定性折叠
//!     -> 后续部署状态读取者
//! ```
//!
//! Core 在这里仅导出 Domain reducer，不执行 admission、EventStore append、lease/fence、
//! supervisor/process、approval 或 capability effect。当前 workspace 未发现这两个 facade 的
//! 生产调用者；DEP-05 虽有 focused fixture proof，但部署操作路径尚未因这些函数而接通。
//! `validate_operation_journal()` 检查输入 journal 的内部合同；`replay_operation_journal()`
//! 接收 journal 身份和独立 transition iterator，不过当前实现没有校验传入 journal 或比较重放
//! 结果与该 journal。该差异会影响对“重放成功”的理解，不能把它当作 persisted snapshot 校验。

use kiana_domain::{OperationJournal, OperationTransition};

/// 验证一个 operation journal 的字段、transition 链、派生投影和摘要是否内部自洽。
///
/// 【调用者】公开导出给 Core 使用方；当前未发现仓库内生产调用点，source guard/fixture 可用
/// 它确认 Domain 校验入口保持可见。
///
/// 【输入/输出】接收待检查 `OperationJournal` 的借用；完全符合 Domain 合同时返回 `Ok(())`，
/// 否则把 Domain 的稳定 `String` 错误原样返回。
///
/// 【边界】这不会读取 EventStore、证明 cursor 对应已提交事件、校验证据引用真实存在或持久化
/// 修复 journal，也不会重试、改状态或触发 operation effect。
pub fn validate_operation_journal(journal: &OperationJournal) -> Result<(), String> {
    journal.validate()
}

/// 从给定 journal 的身份字段出发，按 transition iterator 重建一个新的内存投影。
///
/// 【调用者】公开导出给上层恢复/查询代码；当前未发现仓库内生产调用点。Domain `replay()`
/// 从 Created 开始并逐条调用 `append()`，因此会校验 transition 自身、operation/revision 身份、
/// 序号/cursor 连续性和 lifecycle 状态迁移。
///
/// 【输入/输出】`journal` 参数当前只提供 operation ID、revision ID 和 revision digest；第二个
/// 参数是独立的 transition 序列。成功返回由该序列折叠出的新 journal，而不是原参数的修改版。
/// 非法身份、状态迁移或序列时返回 Domain 稳定字符串；本函数不访问存储、不写回结果、不自动重试。
///
/// ⚠ 此 facade 当前不会先调用 `journal.validate()`，也不会比较重放后的 state、sequence、cursor
/// 或 digest 与传入 journal。若把 persisted journal 当作期望快照传入，截断的 transitions 仍可
/// 返回一个更早的有效投影；因此 `Ok(replayed)` 只表示提供的前缀可折叠，不代表快照完整。
/// 该不一致已由 DEP-05 roadmap gap 记录，本注释任务不改变其行为。
pub fn replay_operation_journal(
    journal: &OperationJournal,
    transitions: impl IntoIterator<Item = OperationTransition>,
) -> Result<OperationJournal, String> {
    let mut replayed = OperationJournal::new(
        journal.operation_id,
        journal.revision_id.clone(),
        journal.revision_digest.clone(),
    )?;
    for transition in transitions {
        replayed.append(transition)?;
    }
    Ok(replayed)
}
