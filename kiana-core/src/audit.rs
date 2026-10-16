//! ControlPlane 授权层的审计记录投影 Facade
//!
//! 本模块是 Core 对审计系统的统一入口。核心设计理念：
//!
//! 1. **所有权边界明确**：
//!    - 只有 ControlPlane（Core 层）有权创建审计记录
//!    - Adapter（Provider、Broker、Query）可读取已提交的 EventLog 页，
//!      向 Core 申请投影计算，但无权从 Model、UI、Plugin、Exporter、
//!      Capability Handler 这些外部组件接收审计记录输入
//!
//! 2. **事件流向严格单向**：
//!    EventLog（存储）→ Core 审计投影 → Query/Audit 查询面（只读）
//!    外部组件不直接造审计事实，防止数据篡改和权限规避
//!
//! 3. **两个计算接口**：
//!    - `reduce_committed_audit_records`：从清洁的事件页面生成审计行
//!    - `append_committed_audit_records`：追加新页面时保留既有审计事实
//!    两者都是 pure function（无状态、幂等），依赖 kiana_domain 的核心算法

use kiana_domain::{AuditRecord, EventCursor, RuntimeEvent};

/// 从已提交的 EventLog 页面推导出新的审计记录
///
/// 这是审计计算的基础操作。接收一段连续的 `RuntimeEvent` 序列和游标位置，
/// 计算出这些事件中产生的所有审计行。
///
/// # 参数
/// - `events`：EventLog 中已提交（committed）的事件数组。这些事件已永久化，
///   不会被回滚或修改
/// - `source_cursor`：事件序列在 EventLog 中的位置（Page ID、Start Index）。
///   用于在审计记录中追踪事件来源，支持事件的反向查询和恢复链路
///
/// # 返回
/// - `Ok(Vec<AuditRecord>)`：计算成功时，返回新生成的审计记录数组。
///   如果事件页面为空，返回空数组
/// - `Err(String)`：计算失败。可能原因：
///   - 事件格式不合法（字段缺失或类型不符）
///   - 审计规则应用失败
///   - 底层算法异常
///
/// # 设计说明
/// 本函数是 pure function：
/// - 无状态：不修改任何全局状态
/// - 幂等：重复调用相同的 events 和 cursor 总是返回相同结果
/// - 可测试：输入确定，输出确定
///
/// 这样的设计保证了审计数据的可验证性和可重放性。
pub fn reduce_committed_audit_records(
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    // 委托到 kiana_domain 中的核心审计算法
    // 那里包含了所有审计规则的完整实现
    kiana_domain::reduce_audit_records(events, source_cursor)
}

/// 追加新的审计记录同时保留所有既有的审计事实
///
/// 当 EventLog 新增一页事件时，这个函数负责：
/// 1. 计算新事件页中的审计记录
/// 2. 与既有的审计历史合并
/// 3. 确保审计记录的递增性和连贯性
///
/// # 参数
/// - `existing`：已有的审计记录数组。这代表以前的所有审计历史，不应被破坏
/// - `events`：最新一页的事件。需要从中计算增量审计记录
/// - `source_cursor`：新事件页在 EventLog 中的位置
///
/// # 返回
/// - `Ok(Vec<AuditRecord>)`：合并后的完整审计记录数组。
///   包含所有既有记录 + 新增记录，保持原有顺序，新记录追加在末尾
/// - `Err(String)`：如果新事件无法转换为审计记录
///
/// # 设计约束
/// 此函数保证了 **Audit Monotonicity**（审计单调性）：
/// - 既有审计记录永不改动
/// - 新审计记录只能追加，不能修改历史
/// - 审计页面大小单调递增
///
/// 这是 CompanyOS 的核心安全保证之一。任何审计数据的删除或修改都意味着
/// 系统已被破坏，应立即告警。
pub fn append_committed_audit_records(
    existing: &[AuditRecord],
    events: &[RuntimeEvent],
    source_cursor: EventCursor,
) -> Result<Vec<AuditRecord>, String> {
    // 1. 从新事件计算增量审计记录
    let new_records = kiana_domain::reduce_audit_records(events, source_cursor)?;

    // 2. 将新审计记录追加到既有历史
    // 既有记录保持不变，新记录直接追加，保证单调性
    kiana_domain::append_audit_records(existing, events, source_cursor)
}
