//! Query 层索引 freshness/read gate 与 Domain invalidation plan 转发器。
//!
//! 【系统位置与数据流】workspace scanner 产生前后两份 `WorkspaceSnapshot` → `plan_index_invalidation()` 委托给 Domain 派生变更证据；
//! 数据治理层提供 epoch/传播计划 → `governed_index_invalidation()` 判断索引是否需作废；实际 cache 删除/索引 rebuild 仍由其他组件负责。
//!
//! 这些函数不会把 stale/revoked index 当成空搜索结果，也不会自己返回命中；`index_read_allowed()` 是读取前的拒绝 gate。
//! 当前 Query API 对外导出这些能力，但没有发现生产调用者；测试覆盖 Domain 计划和边界语义。

use kiana_domain::{
    DataPayloadState, DataPropagationPlan, DataPropagationTarget, IndexInvalidationPlan,
    WorkspaceSnapshot,
};

/// 在返回索引命中前检查数据 epoch 和 payload 可用状态。
///
/// 【调用者】查询 adapter 在使用 cached/indexed hits 前；当前仓内未发现生产调用点。
/// 【输入/输出】比较索引绑定 epoch 与当前 epoch，并检查来源是否仍 Available；全部通过返回 `Ok(())`。
/// 【失败情况】0/epoch 不相等返回 `index_data_epoch_stale`；Expired/Revoked/Unknown 等非 Available 状态返回 `index_data_revoked_or_expired`。
/// 【为什么返回 Err】调用者应显式拒绝或重建，而不能把 stale/revoked 索引伪装成“没有搜索结果”。本函数不执行 rebuild。
pub fn index_read_allowed(
    data_epoch: u64,
    current_data_epoch: u64,
    payload_state: DataPayloadState,
) -> Result<(), String> {
    if data_epoch == 0 || current_data_epoch == 0 || data_epoch != current_data_epoch {
        return Err("index_data_epoch_stale".to_owned());
    }
    if payload_state != DataPayloadState::Available {
        return Err("index_data_revoked_or_expired".to_owned());
    }
    Ok(())
}

/// 查看治理传播计划中 Index 目标是否已经标为 Invalidated 或 Unknown。
///
/// 【调用者】索引更新编排器；当前仓内未发现生产调用点。
/// 【输入/输出】接收 Domain `DataPropagationPlan` 和当前 epoch；成功返回 `bool`，true 表示 plan 声明需要处理索引目标。
/// 【失败情况】plan 无效、当前 epoch 落后于计划 epoch，或 plan 没有 Index target 时返回稳定错误。
/// 【边界】true 只是传播计划 metadata，不授权读取、不删除 cache，也不证明 rebuild 已完成；Unknown 必须保留为待处理状态。
pub fn governed_index_invalidation(
    plan: &DataPropagationPlan,
    current_data_epoch: u64,
) -> Result<bool, String> {
    plan.validate()?;
    if current_data_epoch < plan.data_epoch {
        return Err("index_governance_epoch_ahead".to_owned());
    }
    let target = plan
        .targets
        .iter()
        .find(|receipt| receipt.target == DataPropagationTarget::Index)
        .ok_or_else(|| "index_propagation_target_missing".to_owned())?;
    Ok(
        target.state == kiana_domain::DataPropagationState::Invalidated
            || target.state == kiana_domain::DataPropagationState::Unknown,
    )
}

/// Query 对外提供的快照差异入口，直接委托 Domain 生成 typed plan。
///
/// 【调用者】需要把 workspace 前后快照变化交给 Query 层的 adapter；目前调用点只在 fixture。
/// 【输入/输出】旧 snapshot 可为 None（首次构建），另给当前 snapshot 与 source/target generation，返回 `IndexInvalidationPlan`。
/// 【副作用】完全委托纯 Domain 合同，不访问工作区、不写索引或 cache。
pub fn plan_index_invalidation(
    previous: Option<&WorkspaceSnapshot>,
    current: &WorkspaceSnapshot,
    source_generation: u64,
    target_generation: u64,
) -> Result<IndexInvalidationPlan, String> {
    IndexInvalidationPlan::from_snapshots(previous, current, source_generation, target_generation)
}
