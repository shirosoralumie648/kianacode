//! Memory-side invalidation boundary shared by memory/index/cache projections.
//!
//! This is an in-process CI adapter. It records the epoch fence and target receipts; it does not
//! claim physical JSONL deletion or cross-process recovery. Reads must check the committed epoch
//! before ranking or reinjecting a memory row.

use kiana_domain::{
    DataPropagationPlan, DataPropagationReceipt, DataPropagationState, DataPropagationTarget,
};
use kiana_ports::PortError;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ===== 内存治理状态结构 =====
// 这是 MemoryDataGovernanceStore 的内部状态容器。使用 BTreeMap 存储是为了：
// 1. 支持有序迭代（按 (project_ref, data_epoch) 排序）
// 2. 便于检测 epoch 回退（升级检查）
// 3. 在 CI 测试环境中提供确定性查询结果
//
// 注意：这是内存存储，无重启持久化。生产场景应使用数据库（如 PostgreSQL）。
#[derive(Debug, Default)]
struct MemoryGovernanceState {
    // 数据传播计划表：(项目引用, epoch) -> 计划
    // epoch 表示数据版本号，防止旧计划覆盖新计划
    plans: BTreeMap<(String, u64), DataPropagationPlan>,

    // 数据传播收据表：(项目引用, epoch, 目标) -> 收据
    // 记录每个目标的传播状态（如 "Audit"、"Index" 等）
    receipts: BTreeMap<(String, u64, String), DataPropagationReceipt>,
}

// ===== 内存数据治理存储实现 =====
// 作用：供 CI 测试和组件组装使用的非持久化存储。
//
// 核心职责：
// 1. 记录数据传播计划和目标
// 2. 追踪各传播目标的完成状态
// 3. 实施 fail-closed 语义：未通过校验的行禁止读取
//
// 与持久化存储的区别：
// - 内存存储在进程重启后丢失
// - 无 ACID 事务保证（生产场景由数据库提供）
// - 适用于单进程 CI 环保或短期功能测试
#[derive(Clone, Debug, Default)]
pub struct MemoryDataGovernanceStore {
    state: Arc<Mutex<MemoryGovernanceState>>,
}

impl MemoryDataGovernanceStore {
    pub fn new() -> Self {
        Self::default()
    }

    // ===== 应用数据传播计划 =====
    // 新建或验证数据传播计划。防止计划冲突和 epoch 回退。
    //
    // 约束条件：
    // 1. 计划必须通过自验证（validate() 检查必填字段）
    // 2. 同一项目的新 epoch 必须 > 旧 epoch（禁止回退）
    // 3. 重复提交同一计划（digest 相同）是幂等的，返回 Ok
    // 4. 计划内容冲突（相同 key，不同 digest）返回 Conflict 错误
    pub async fn apply_plan(&self, plan: DataPropagationPlan) -> Result<(), PortError> {
        // 第一步：自验证（检查计划的必填字段）
        plan.validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_plan:{error}")))?;
        let mut state = self.state.lock().await;
        let key = (plan.project_ref.clone(), plan.data_epoch);

        // 第二步：检查幂等性（相同 key 且 digest 相同 = 重复提交）
        if let Some(existing) = state.plans.get(&key) {
            if existing.plan_digest == plan.plan_digest {
                return Ok(()); // 已存在相同计划，无需重复处理
            }
            return Err(PortError::Conflict(
                "data_propagation_plan_conflict".to_owned(),
            ));
        }

        // 第三步：检查 epoch 单调递增（fail-closed 防止时间旅行）
        if state
            .plans
            .keys()
            .any(|(project, epoch)| project == &plan.project_ref && *epoch > plan.data_epoch)
        {
            return Err(PortError::Conflict(
                "data_propagation_epoch_regressed".to_owned(),
            ));
        }

        // 第四步：插入计划
        state.plans.insert(key, plan);
        Ok(())
    }

    // ===== 追加数据传播收据 =====
    // 记录特定传播目标（如 Index、Cache）的完成状态。
    //
    // 三层防御机制：
    // 1. 收据所指的目标必须在计划的目标列表中（plan.targets 包含）
    // 2. EventProjection 目标禁止作为收据目标（保留用于内部）
    // 3. 计划本身必须已存在且 epoch 相符
    //
    // 幂等性：相同的收据（key 相同）重复提交返回 Ok
    pub async fn append_propagation_receipt(
        &self,
        plan: &DataPropagationPlan,
        receipt: DataPropagationReceipt,
    ) -> Result<(), PortError> {
        // 第一步：验证计划和收据的完整性
        plan.validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_plan:{error}")))?;
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("data_propagation_receipt:{error}")))?;

        // 第二步：检查收据目标合法性
        // EventProjection 不允许作为接收端，必须在计划的目标列表中
        if receipt.target == DataPropagationTarget::EventProjection
            || !plan.targets.iter().any(|target| {
                target.target == receipt.target
                    && target.project_ref == receipt.project_ref
                    && target.data_epoch == receipt.data_epoch
            })
        {
            return Err(PortError::Conflict(
                "memory_propagation_target_not_in_plan".to_owned(),
            ));
        }

        // 第三步：原子锁定状态，检查计划存在且处理收据
        let mut state = self.state.lock().await;
        if !state
            .plans
            .contains_key(&(plan.project_ref.clone(), plan.data_epoch))
        {
            return Err(PortError::Unavailable(
                "memory_propagation_plan_missing".to_owned(),
            ));
        }

        // 第四步：插入或验证收据的幂等性
        let key = (
            receipt.project_ref.clone(),
            receipt.data_epoch,
            receipt.target.as_str().to_owned(),
        );
        if let Some(existing) = state.receipts.get(&key) {
            if existing == &receipt {
                return Ok(()); // 重复提交相同收据，幂等返回
            }
            return Err(PortError::Conflict(
                "memory_propagation_receipt_conflict".to_owned(),
            ));
        }
        state.receipts.insert(key, receipt);
        Ok(())
    }

    // ===== 查询行读取权限 =====
    // 根据当前服务器 epoch 和传播状态，判断内存/索引/缓存中的一行是否可读。
    //
    // 权限检查规则：
    // 1. 无传播计划存在 → 允许读（向后兼容）
    // 2. 行的 epoch < 计划的 epoch → 禁止读（数据过期）
    // 3. 目标状态为 "PreservedMetadata" 且目标是 Audit → 允许读（元数据已保存）
    // 4. 其他情况 → 禁止读（数据传播尚未完成）
    //
    // Fail-closed 设计：不满足任何允许条件 → 禁读，防止数据泄露。
    pub async fn read_allowed(
        &self,
        project_ref: &str,
        observed_data_epoch: u64,
        target: DataPropagationTarget,
    ) -> Result<bool, PortError> {
        let state = self.state.lock().await;

        // 第一步：查询该项目最新的计划
        let Some((_, plan)) = state
            .plans
            .iter()
            .filter(|((project, _), _)| project == project_ref)
            .max_by_key(|((_, epoch), _)| *epoch)
        // 取最大 epoch（最新计划）
        else {
            return Ok(true); // 无计划，允许读取（legacy 行为）
        };

        // 第二步：检查行 epoch 是否已追上计划 epoch
        if observed_data_epoch < plan.data_epoch {
            return Ok(false); // 行数据版本过旧，禁止读
        }

        // 第三步：查询该目标的完成收据
        let completed = state.receipts.values().find(|receipt| {
            receipt.project_ref == project_ref
                && receipt.data_epoch == plan.data_epoch
                && receipt.target == target
        });

        // 第四步：判断读权限
        // 仅当传播状态为 "PreservedMetadata" 且目标为 Audit 时，允许读
        Ok(completed.is_some_and(|receipt| {
            receipt.state == DataPropagationState::PreservedMetadata
                && target == DataPropagationTarget::Audit
        }))
    }
}
