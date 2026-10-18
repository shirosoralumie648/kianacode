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
//! 把输入 journal 当作 expected persisted snapshot：先校验它，再用独立 transition iterator
//! 重建投影，并要求重建结果与快照逐字段一致。

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

/// 从给定 journal 的身份字段出发，按 transition iterator 重建投影并校验其与快照一致。
///
/// 【调用者】公开导出给上层恢复/查询代码；当前未发现仓库内生产调用点。Domain `replay()`
/// 从 Created 开始并逐条调用 `append()`，因此会校验 transition 自身、operation/revision 身份、
/// 序号/cursor 连续性和 lifecycle 状态迁移。
///
/// 【输入/输出】`journal` 参数是 expected persisted snapshot；第二个参数是独立的 transition
/// 序列。成功返回重建的 journal，且它必须与 `journal` 逐字段相等（state/phase/preflight、
/// sequence、source cursor、reason/evidence、active deadline、完整 transition 列表与
/// `journal_digest`），否则返回 `operation_journal_replay_mismatch`。传入快照自身不合法时先
/// 返回 `journal.validate()` 的稳定错误。本函数不访问存储、不写回结果、不自动重试。
///
/// 【失败与边界】截断、重复、乱序、不同 revision/cursor 或不同 terminal 的 transitions 都不能
/// 静默产生一个更早的投影；失败不会返回部分成功 journal。无密钥摘要只能证明重建载荷与快照
/// 自洽，不能证明快照曾由 EventStore 提交或来自可信 signer。
pub fn replay_operation_journal(
    journal: &OperationJournal,
    transitions: impl IntoIterator<Item = OperationTransition>,
) -> Result<OperationJournal, String> {
    // 先验证 expected persisted snapshot；坏快照不能被当作重放基准。
    journal.validate()?;
    let mut replayed = OperationJournal::new(
        journal.operation_id,
        journal.revision_id.clone(),
        journal.revision_digest.clone(),
    )?;
    for transition in transitions {
        replayed.append(transition)?;
    }
    // 重建结果必须与快照逐字段一致；截断或替换的序列不能返回更早的“成功”投影。
    if replayed != *journal {
        return Err("operation_journal_replay_mismatch".to_owned());
    }
    Ok(replayed)
}
