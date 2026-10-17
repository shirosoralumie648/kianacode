//! 本文件从已提交的成本分摊事件重建 run 及其组织/项目/workflow/cell 维度的只读金额视图。
//!
//! 在系统中的位置：成本分摊写入和 EventLog 所有权属于上游 Core/Daemon；本模块只接收事件切片，
//! 校验每条 Domain `CostAllocation` 与 Event 元数据的绑定，再生成查询投影。当前仓内直接调用只
//! 出现在 BQ-19 fixture；crate 根模块公开 re-export，但没有找到生产调用点。
//!
//! ```text
//! CostAllocation Event 切片
//!   -> kind/sequence/event ID/run/aggregate/revision 绑定校验
//!   -> 每个 allocation.usage_id 作为一个唯一叶子
//!   -> run total 与 organization/project/workflow/cell totals 分别累计同一批叶子
//!   -> CostAllocationProjection 查询视图
//! ```
//!
//! 各维度 total 是对同一批叶子的平行汇总，不能把 run、project、organization 等视图相加；只有
//! run total 表示当前投影中的 ledger 总额。该模块不追加 ledger 事实、不修改 reservation、不授权
//! 项目转账，也不把 `source` 级事件投影说成现实支出证明。

use kiana_domain::{
    json_digest, AllocationCostKind, CostAllocation, CostAllocationId, EventId, OrganizationId,
    ProjectId, RunId, RuntimeEvent, SchemaVersion, UsageId, WorkflowInstanceId,
    COST_ALLOCATION_EVENT,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const COST_ALLOCATION_PROJECTION_SCHEMA: &str = "kiana.cost-allocation-projection.v1";
pub const COST_ALLOCATION_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const COST_ALLOCATION_PORTFOLIO_SCHEMA: &str = "kiana.cost-allocation-portfolio.v1";
pub const COST_ALLOCATION_PORTFOLIO_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 从成本分摊事件重建失败时使用的 Query 层错误分类。
///
/// `Invalid(String)` 保留 Domain/投影字段级错误，外层入口可据此区分无效 run、无输入事件和
/// 事件/金额合同错误；这些错误会使整个投影失败，不返回部分合计。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostAllocationProjectionError {
    /// 调用方传入 nil RunId。
    #[error("cost_allocation_projection_run_invalid")]
    RunInvalid,
    /// 切片中没有任何符合成本分摊事件 kind 的记录。
    #[error("cost_allocation_projection_source_empty")]
    SourceEmpty,
    /// 事件解码、身份绑定、重复 usage 或金额合计等其他合同失败。
    #[error("cost_allocation_projection_invalid:{0}")]
    Invalid(String),
}

/// 某一聚合范围内的估算金额、实测金额、未知项数量和唯一叶子数量。
///
/// Estimate 与 measured 金额分开存放，避免把预测值和已测值误合并；unknown 只增加数量，不伪造金额。
/// `leaf_count` 统计唯一 usage leaf，而不是维度数或父子行数；同一个 leaf 会分别进入多个维度视图。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationTotals {
    /// 所有 Estimated allocation 金额的和；没有此类叶子时为 None。
    pub estimated: Option<kiana_domain::Money>,
    /// 所有 Measured allocation 金额的和；没有此类叶子时为 None。
    pub measured: Option<kiana_domain::Money>,
    /// 成本状态为 Unknown 的唯一叶子数量，不能当成 0 金额使用。
    pub unknown_count: u64,
    /// 此聚合范围内处理过的 allocation 叶子数量。
    pub leaf_count: u64,
}

impl AllocationTotals {
    /// 将金额累加进估算或实测槽位，并拒绝跨币种合并和整数溢出。
    ///
    /// 首笔金额直接建立槽位；后续金额必须与既有币种相同，再以 checked_add 防止溢出。错误由
    /// `add()` 和投影入口向上传播，避免输出一个看似有效但金额被截断的报表。
    fn add_money(
        slot: &mut Option<kiana_domain::Money>,
        amount: &kiana_domain::Money,
    ) -> Result<(), String> {
        if let Some(existing) = slot.as_ref() {
            if existing.currency != amount.currency {
                return Err("cost_allocation_currency_mismatch".to_owned());
            }
        }
        let next = slot
            .as_ref()
            .map(|existing| {
                existing
                    .micros
                    .checked_add(amount.micros)
                    .ok_or_else(|| "cost_allocation_total_overflow".to_owned())
                    .and_then(|micros| kiana_domain::Money::new(amount.currency.clone(), micros))
            })
            .unwrap_or_else(|| Ok(amount.clone()))?;
        *slot = Some(next);
        Ok(())
    }

    /// 把一个叶子的成本分类累加到本维度统计。
    ///
    /// 每种状态都先增加 leaf_count；Estimated/Measured 累加到各自金额槽，Unknown 增加
    /// unknown_count。因为同一 allocation 会分别调用到 run 和各维度的 totals，此计数是每个
    /// 视图中的叶子数，不可再把多个视图的计数相加。
    fn add(&mut self, cost: &AllocationCostKind) -> Result<(), String> {
        self.leaf_count = self
            .leaf_count
            .checked_add(1)
            .ok_or_else(|| "cost_allocation_total_overflow".to_owned())?;
        match cost {
            AllocationCostKind::Estimated { amount, .. } => {
                Self::add_money(&mut self.estimated, amount)
            }
            AllocationCostKind::Measured { amount, .. } => {
                Self::add_money(&mut self.measured, amount)
            }
            AllocationCostKind::Unknown { .. } => {
                self.unknown_count = self
                    .unknown_count
                    .checked_add(1)
                    .ok_or_else(|| "cost_allocation_total_overflow".to_owned())?;
                Ok(())
            }
        }
    }

    /// 检查 unknown 数不超过叶子数，并校验存在的 Money 值。
    ///
    /// 这是单个 totals 的局部校验，不会验证该值是否真的由投影中的 allocation 重算而来。
    pub fn validate(&self) -> Result<(), String> {
        if self.unknown_count > self.leaf_count {
            return Err("cost_allocation_unknown_count_invalid".to_owned());
        }
        for amount in [self.estimated.as_ref(), self.measured.as_ref()]
            .into_iter()
            .flatten()
        {
            amount.validate()?;
        }
        Ok(())
    }
}

/// 从单一 Run 的成本分摊事件生成的多维只读查询结果。
///
/// 本结构保留 allocation 明细和事件来源，再附上五个平行汇总索引。它当前没有 Serde 自动编解码
/// derive，且不负责持久化；调用方拿到的是内存对象。字段为 public，`validate()` 适合验证合同，
/// 但不会逐项重算维度 totals 与 allocations 的一致性。
#[derive(Clone, Debug, PartialEq)]
pub struct CostAllocationProjection {
    /// 固定 Query 投影 schema。
    pub schema: &'static str,
    /// 兼容性版本。
    pub version: SchemaVersion,
    /// 此投影只允许包含一个 run 的 allocations。
    pub run_id: RunId,
    /// 所有已处理成本分摊事件的最大 sequence。
    pub source_cursor: u64,
    /// 与 `allocations` 一一对应、保持输入事件顺序的源事件 ID 列表。
    pub source_event_ids: Vec<EventId>,
    /// 通过所有事件与 Domain 合同校验的明细叶子。
    pub allocations: Vec<CostAllocation>,
    /// 唯一 ledger 范围汇总；不应与下面的维度汇总相加。
    pub run_totals: AllocationTotals,
    /// 同一批叶子按 organization ID 的平行汇总。
    pub organization_totals: BTreeMap<OrganizationId, AllocationTotals>,
    /// 同一批叶子按 project ID 的平行汇总。
    pub project_totals: BTreeMap<ProjectId, AllocationTotals>,
    /// 同一批叶子按 workflow instance ID 的平行汇总。
    pub workflow_totals: BTreeMap<WorkflowInstanceId, AllocationTotals>,
    /// 同一批叶子按 Cell ID 的平行汇总。
    pub cell_totals: BTreeMap<kiana_domain::CellId, AllocationTotals>,
}

impl CostAllocationProjection {
    /// 校验投影头部、run 归属、usage 唯一性、来源事件唯一性和各 totals 的局部金额合法性。
    ///
    /// 【调用者】`project_cost_allocations()` 生成对象后立即调用；外部调用方也可再次验证。
    /// 【拒绝条件】schema/version/run/cursor/source-event 数量不符，来源事件 ID 重复，run total
    /// 无效，allocation 不属于当前 run 或重复 usage，任一维度 totals 自身不合法，以及任一 totals
    /// 无法由 `allocations` 逐条重算时返回结构化错误。
    /// 【边界】validator 不从 EventStore 读取事实、不证明事件已持久化，也不检查事件 sequence；
    /// 它只证明内存中的明细与汇总互相自洽。
    pub fn validate(&self) -> Result<(), CostAllocationProjectionError> {
        if self.schema != COST_ALLOCATION_PROJECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_ALLOCATION_PROJECTION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() != self.allocations.len()
        {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_header_invalid".to_owned(),
            ));
        }
        self.run_totals
            .validate()
            .map_err(CostAllocationProjectionError::Invalid)?;
        let mut event_ids = BTreeSet::new();
        let mut usage_ids = BTreeSet::new();
        for (event_id, allocation) in self.source_event_ids.iter().zip(&self.allocations) {
            if !event_ids.insert(*event_id) {
                return Err(CostAllocationProjectionError::Invalid(
                    "cost_allocation_projection_source_event_duplicate".to_owned(),
                ));
            }
            allocation
                .validate()
                .map_err(CostAllocationProjectionError::Invalid)?;
            if allocation.scope.run_id != self.run_id || !usage_ids.insert(allocation.usage_id) {
                return Err(CostAllocationProjectionError::Invalid(
                    "cost_allocation_leaf_duplicate_or_run_mismatch".to_owned(),
                ));
            }
        }
        for totals in self
            .organization_totals
            .values()
            .chain(self.project_totals.values())
            .chain(self.workflow_totals.values())
            .chain(self.cell_totals.values())
        {
            totals
                .validate()
                .map_err(CostAllocationProjectionError::Invalid)?;
        }
        let recomputed = Self::fold_dimensions(&self.allocations)
            .map_err(CostAllocationProjectionError::Invalid)?;
        if recomputed.0 != self.run_totals
            || recomputed.1 != self.organization_totals
            || recomputed.2 != self.project_totals
            || recomputed.3 != self.workflow_totals
            || recomputed.4 != self.cell_totals
        {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_totals_recompute_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// 由明细叶子重新折叠 run 与四个平行维度汇总。
    ///
    /// 每个叶子在 run 和每个适用维度各计一次；返回的五个汇总必须与投影中保存的值逐项相等，
    /// 否则说明有人手工改写过 totals。该函数只读，不修改投影。
    fn fold_dimensions(
        allocations: &[CostAllocation],
    ) -> Result<
        (
            AllocationTotals,
            BTreeMap<OrganizationId, AllocationTotals>,
            BTreeMap<ProjectId, AllocationTotals>,
            BTreeMap<WorkflowInstanceId, AllocationTotals>,
            BTreeMap<kiana_domain::CellId, AllocationTotals>,
        ),
        String,
    > {
        let mut run_totals = AllocationTotals::default();
        let mut organization_totals: BTreeMap<OrganizationId, AllocationTotals> = BTreeMap::new();
        let mut project_totals: BTreeMap<ProjectId, AllocationTotals> = BTreeMap::new();
        let mut workflow_totals: BTreeMap<WorkflowInstanceId, AllocationTotals> = BTreeMap::new();
        let mut cell_totals: BTreeMap<kiana_domain::CellId, AllocationTotals> = BTreeMap::new();
        for allocation in allocations {
            run_totals.add(&allocation.cost)?;
            organization_totals
                .entry(allocation.scope.organization_id)
                .or_default()
                .add(&allocation.cost)?;
            project_totals
                .entry(allocation.scope.project_id)
                .or_default()
                .add(&allocation.cost)?;
            workflow_totals
                .entry(allocation.scope.workflow_id)
                .or_default()
                .add(&allocation.cost)?;
            cell_totals
                .entry(allocation.scope.cell_id)
                .or_default()
                .add(&allocation.cost)?;
        }
        Ok((
            run_totals,
            organization_totals,
            project_totals,
            workflow_totals,
            cell_totals,
        ))
    }

    /// 获取指定项目的平行金额视图；无该项目叶子时返回 None。
    pub fn project_total(&self, project_id: ProjectId) -> Option<&AllocationTotals> {
        self.project_totals.get(&project_id)
    }

    /// 获取指定组织的平行金额视图；无该组织叶子时返回 None。
    pub fn organization_total(&self, organization_id: OrganizationId) -> Option<&AllocationTotals> {
        self.organization_totals.get(&organization_id)
    }

    /// 获取指定 workflow instance 的平行金额视图；无该 workflow 叶子时返回 None。
    pub fn workflow_total(&self, workflow_id: WorkflowInstanceId) -> Option<&AllocationTotals> {
        self.workflow_totals.get(&workflow_id)
    }

    /// 获取指定 Cell 的平行金额视图；无该 Cell 叶子时返回 None。
    pub fn cell_total(&self, cell_id: kiana_domain::CellId) -> Option<&AllocationTotals> {
        self.cell_totals.get(&cell_id)
    }

    /// 返回代表当前分摊 ledger 金额的 run 总计。
    ///
    /// project/organization/workflow/cell totals 是相同 allocation 叶子的筛选视图；将它们与
    /// run total 相加会重复统计同一笔分摊。
    pub fn ledger_total(&self) -> &AllocationTotals {
        &self.run_totals
    }
}

/// CO-45 portfolio 视图中的一行：一个 `(organization, project)` 组合在同一批叶子上的折叠。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostAllocationPortfolioEntry {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub totals: AllocationTotals,
}

/// 由同一批 allocation 叶子重算的 CO-45 portfolio 只读视图。
///
/// 它不重新读取 EventLog、不保存第二份金额，只把 `CostAllocationProjection` 的叶子按
/// `(organization, project)` 再折叠一次，用于验证 portfolio 视图可以从账本叶子确定性重算。
/// 每个叶子只进入一行；行与行之间是互不重叠的 project 视图，行内 totals 与 run total 一样
/// 来自同一叶子集合，因此行总额与账本总额一致时才是合法视图。
#[derive(Clone, Debug, PartialEq)]
pub struct CostAllocationPortfolio {
    pub schema: &'static str,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub source_cursor: u64,
    pub ledger_total: AllocationTotals,
    pub projects: Vec<CostAllocationPortfolioEntry>,
    /// 由叶子集合确定性重算的摘要；相同叶子重算得到同一摘要，不依赖 event ID。
    pub recompute_digest: String,
}

impl CostAllocationPortfolio {
    /// 从已经过唯一性/身份校验的投影折叠 portfolio 视图。
    ///
    /// 【拒绝条件】投影自身无效、行折叠时币种混合或整数溢出、行叶子数之和与账本叶子数不符、
    /// 以及最终摘要无法自证时返回结构化错误。
    pub fn from_projection(
        projection: &CostAllocationProjection,
    ) -> Result<Self, CostAllocationProjectionError> {
        projection.validate()?;
        let mut by_project: BTreeMap<(OrganizationId, ProjectId), AllocationTotals> =
            BTreeMap::new();
        for allocation in &projection.allocations {
            by_project
                .entry((
                    allocation.scope.organization_id,
                    allocation.scope.project_id,
                ))
                .or_default()
                .add(&allocation.cost)
                .map_err(CostAllocationProjectionError::Invalid)?;
        }
        let projects = by_project
            .into_iter()
            .map(
                |((organization_id, project_id), totals)| CostAllocationPortfolioEntry {
                    organization_id,
                    project_id,
                    totals,
                },
            )
            .collect();
        let mut portfolio = Self {
            schema: COST_ALLOCATION_PORTFOLIO_SCHEMA,
            version: COST_ALLOCATION_PORTFOLIO_VERSION,
            run_id: projection.run_id,
            source_cursor: projection.source_cursor,
            ledger_total: projection.run_totals.clone(),
            projects,
            recompute_digest: String::new(),
        };
        portfolio.recompute_digest = portfolio.fresh_digest();
        portfolio.validate()?;
        Ok(portfolio)
    }

    /// 取某个组织的某个项目视图；不存在时返回 None。
    pub fn project(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Option<&AllocationTotals> {
        self.projects
            .iter()
            .find(|entry| {
                entry.organization_id == organization_id && entry.project_id == project_id
            })
            .map(|entry| &entry.totals)
    }

    /// 校验 schema/版本/run、行叶子数之和与账本一致、行互不重复且摘要自洽。
    pub fn validate(&self) -> Result<(), CostAllocationProjectionError> {
        if self.schema != COST_ALLOCATION_PORTFOLIO_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_ALLOCATION_PORTFOLIO_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.projects.is_empty()
        {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_portfolio_header_invalid".to_owned(),
            ));
        }
        self.ledger_total
            .validate()
            .map_err(CostAllocationProjectionError::Invalid)?;
        let mut leaf_count = 0u64;
        let mut previous: Option<(OrganizationId, ProjectId)> = None;
        for entry in &self.projects {
            entry
                .totals
                .validate()
                .map_err(CostAllocationProjectionError::Invalid)?;
            if let Some(previous) = previous {
                if previous >= (entry.organization_id, entry.project_id) {
                    return Err(CostAllocationProjectionError::Invalid(
                        "cost_allocation_portfolio_project_duplicate".to_owned(),
                    ));
                }
            }
            previous = Some((entry.organization_id, entry.project_id));
            leaf_count = leaf_count
                .checked_add(entry.totals.leaf_count)
                .ok_or_else(|| {
                    CostAllocationProjectionError::Invalid(
                        "cost_allocation_total_overflow".to_owned(),
                    )
                })?;
        }
        if leaf_count != self.ledger_total.leaf_count {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_portfolio_total_mismatch".to_owned(),
            ));
        }
        if self.recompute_digest != self.fresh_digest() {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_portfolio_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// 只依赖叶子集合的确定性摘要；同批叶子重算必然得到同一值。
    pub fn fresh_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "source_cursor": self.source_cursor,
            "ledger_total": self.ledger_total,
            "projects": self.projects,
        }))
    }
}

/// 从事件切片重建一个 Run 的成本分摊明细及所有查询维度汇总。
///
/// 【调用者】当前仓内直接调用点是 `kiana-query/tests/bq19_allocation_projector.rs`；Query crate
/// 公开 re-export 两个同义入口，但没有找到 Core/Daemon/entrypoint 的生产调用。
/// 【输入】目标 `run_id` 和 RuntimeEvent 切片。函数只选 `COST_ALLOCATION_EVENT`；其他事件跳过。
/// 对选中事件要求 sequence 严格递增、event_id 不重复、payload 是有效 `CostAllocation`，并要求
/// run、aggregate type/ID、stream version 与 allocation 内的身份/revision 一致。
/// 【输出】成功返回明细叶子、来源事件和 run/org/project/workflow/cell 汇总；source_cursor 是选中
/// allocation 事件中最大的 sequence。
/// 【核心流程】验证 run → 遍历目标事件并校验顺序/ID → 解码并校验 Domain allocation → 绑定事件元数据
/// → 拒绝重复 usage leaf → 同时累计五个视图 → 拒绝空来源 → 最后校验 projection。
/// 【失败与影响】任一 source event 畸形、跨 run、序列回退、重复 event/usage、币种不一致或金额溢出都会
/// 令整次投影返回错误；调用方不会得到部分汇总。函数不写入账本，因此错误不会回滚或修改上游事件。
/// 【设计原因】先拒绝重复 usage，再分别累加同一叶子到不同维度，使每个维度可单独查询而不会漏项；
/// 这些维度是重叠标签视图，所以账本唯一可直接读取的总额是 run total。
pub fn project_cost_allocations(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostAllocationProjection, CostAllocationProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(CostAllocationProjectionError::RunInvalid);
    }
    let mut allocations = Vec::new();
    let mut source_event_ids = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    let mut seen_usage_ids = BTreeSet::new();
    let mut source_cursor = 0;
    let mut previous_sequence = 0;
    let run_text = run_id.to_string();
    let mut run_totals = AllocationTotals::default();
    let mut organization_totals: BTreeMap<OrganizationId, AllocationTotals> = BTreeMap::new();
    let mut project_totals: BTreeMap<ProjectId, AllocationTotals> = BTreeMap::new();
    let mut workflow_totals: BTreeMap<WorkflowInstanceId, AllocationTotals> = BTreeMap::new();
    let mut cell_totals: BTreeMap<kiana_domain::CellId, AllocationTotals> = BTreeMap::new();

    for event in events {
        // 忽略其他事件时不检查它们的 sequence；本投影只要求成本分摊事件彼此严格递增。
        if event.kind != COST_ALLOCATION_EVENT {
            continue;
        }
        if !seen_event_ids.insert(event.event_id) {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_duplicate_source_event".to_owned(),
            ));
        }
        if event.sequence == 0 || event.sequence <= previous_sequence {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_projection_sequence_regression".to_owned(),
            ));
        }
        previous_sequence = event.sequence;
        // 成本分摊 payload 本身就是序列化后的 Domain struct；event envelope 元数据在解码后另行绑定。
        let allocation: CostAllocation =
            serde_json::from_value(event.data.clone()).map_err(|_| {
                CostAllocationProjectionError::Invalid("cost_allocation_decode_failed".to_owned())
            })?;
        allocation
            .validate()
            .map_err(CostAllocationProjectionError::Invalid)?;
        if allocation.scope.run_id != run_id
            || event.data.get("run_id").and_then(serde_json::Value::as_str)
                != Some(run_text.as_str())
            || event.aggregate_type.as_deref() != Some("cost_allocation")
            || event.aggregate_id.as_deref() != Some(allocation.allocation_id.to_string().as_str())
            || event.stream_version != Some(allocation.revision)
        {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_identity_or_revision_mismatch".to_owned(),
            ));
        }
        // usage_id 表示这份原始用量叶子；同一叶子若被多条 allocation 记录重复计入，必须停止而不是双计。
        if !seen_usage_ids.insert(allocation.usage_id) {
            return Err(CostAllocationProjectionError::Invalid(
                "cost_allocation_leaf_usage_repeated".to_owned(),
            ));
        }
        // 同一叶子各累加一次到每个适用维度；五份统计是平行视图，不是可互相相加的分项。
        run_totals
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        organization_totals
            .entry(allocation.scope.organization_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        project_totals
            .entry(allocation.scope.project_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        workflow_totals
            .entry(allocation.scope.workflow_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        cell_totals
            .entry(allocation.scope.cell_id)
            .or_default()
            .add(&allocation.cost)
            .map_err(CostAllocationProjectionError::Invalid)?;
        source_cursor = source_cursor.max(event.sequence);
        source_event_ids.push(event.event_id);
        allocations.push(allocation);
    }
    if source_event_ids.is_empty() {
        return Err(CostAllocationProjectionError::SourceEmpty);
    }
    let projection = CostAllocationProjection {
        schema: COST_ALLOCATION_PROJECTION_SCHEMA,
        version: COST_ALLOCATION_PROJECTION_VERSION,
        run_id,
        source_cursor,
        source_event_ids,
        allocations,
        run_totals,
        organization_totals,
        project_totals,
        workflow_totals,
        cell_totals,
    };
    projection.validate()?;
    Ok(projection)
}

/// 单数别名，供调用方按“一条 allocation stream”命名；实现完全委托给批量入口。
///
/// 校验、错误、顺序要求和数据流与 `project_cost_allocations()` 完全相同，不会额外执行处理。
pub fn project_cost_allocation(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostAllocationProjection, CostAllocationProjectionError> {
    project_cost_allocations(run_id, events)
}

/// 从事件切片重算 CO-45 portfolio 视图（按 organization/project 折叠同一批叶子）。
///
/// 【调用者】portfolio/成本视图入口；当前仓内直接调用点是 BQ-19 fixture。
/// 【输入】与 `project_cost_allocations()` 相同：目标 run 和 RuntimeEvent 切片。
/// 【核心流程】先走完整投影（身份、顺序、唯一 usage、金额校验），再把明细叶子按
/// `(organization, project)` 折叠一次，并用行叶子数之和与 run total 交叉验证。
/// 【失败与影响】任一投影错误、币种混合、金额溢出或行/账本叶子数不符都会返回错误；函数只读，
/// 不写 ledger、不消费 reservation，也不改变上游事件。
pub fn project_cost_allocation_portfolio(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostAllocationPortfolio, CostAllocationProjectionError> {
    CostAllocationPortfolio::from_projection(&project_cost_allocations(run_id, events)?)
}

/// 静态源码守卫使用的只读标记；它本身不在运行时控制 projector 的行为。
pub const ALLOCATION_PROJECTOR_IS_READ_ONLY: bool = true;

/// 保留两个 Domain ID 类型的显式类型引用；当前没有调用者，不参与投影计算。
#[allow(dead_code)]
fn _keep_ids_linked(_: CostAllocationId, _: UsageId) {}
