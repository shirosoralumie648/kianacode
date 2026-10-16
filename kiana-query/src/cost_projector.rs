//! 本文件把 Domain 成本 reducer 的结果包装成 Query 层 Run 成本回执投影。
//!
//! 在系统中的位置：上游调用者负责从权威 EventLog 读取 RuntimeEvent；本模块把事件切片交给
//! `kiana_domain::project_receipt_cost_breakdown()` 处理每个 billing attempt 的成本状态迁移，
//! 然后将 breakdown 与 Query schema/version、Run 和 source 信息组合。当前仓内直接调用只在
//! `kiana-query/tests/bq13_cost_projector.rs`；crate 导出接口，但没有找到产品入口调用。
//!
//! ```text
//! 调用方提供 RunId + 已提交 RuntimeEvent 切片
//!   -> Domain reducer 校验 BQ-13 cost events 与 attempt 状态迁移
//!   -> ReceiptCostBreakdown（estimate / measured / unknown 分开）
//!   -> CostReceiptProjection 再绑定 Query schema、Run、cursor 和 event IDs
//!   -> Query 调用方展示或继续组合 read model
//! ```
//!
//! 此模块不读取费率、访问 EventStore、写入事件、释放 quota 或把 estimate 升级成财务事实。
//! 空事件投影和带 Unknown 成本事实是两个不同情况：前者返回 `SourceEmpty`，后者保留在 breakdown。

use kiana_domain::{
    project_receipt_cost_breakdown, ReceiptCostBreakdown, RunId, RuntimeEvent, SchemaVersion,
};

pub const COST_QUERY_PROJECTION_SCHEMA: &str = "kiana.cost-query-projection.v1";
pub const COST_QUERY_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 生成成本查询投影时可能遇到的错误分类。
///
/// `Invalid(String)` 承接 Domain reducer 或外层投影合同错误；Query API 可将它转成稳定响应错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostQueryProjectionError {
    /// 调用方提供 nil RunId，无法确定要查询的执行范围。
    #[error("cost_query_run_invalid")]
    RunInvalid,
    /// 输入事件中没有任何可构成成本 breakdown 的来源事实；这不等于成本金额为零。
    #[error("cost_query_source_empty")]
    SourceEmpty,
    /// Domain reducer 或 Query projection 校验失败。
    #[error("cost_query_projection_invalid:{0}")]
    Invalid(String),
}

/// Domain 收据级成本 breakdown 外层的 Query 投影信封。
///
/// 【系统角色】`breakdown` 是成本事实 reducer 的权威计算结果；外层把它绑定到 Query 投影合同，
/// 方便 API 消费者核对 schema、Run 和来源游标。它当前不是 Serde 自动编解码类型，也不负责持久化。
/// 【数据边界】estimate、measured 和 unknown 仍在 breakdown 内分别表示；Query 层不重算金额，也不
/// 仅凭 projection 成功就把估算标成已结算金额。
#[derive(Clone, Debug, PartialEq)]
pub struct CostReceiptProjection {
    /// 固定 Query 投影 schema。
    pub schema: &'static str,
    /// 当前投影合同版本。
    pub version: SchemaVersion,
    /// breakdown 所属 Run。
    pub run_id: RunId,
    /// breakdown 所依据的最大成本事件 sequence。
    pub source_cursor: u64,
    /// Domain reducer 保留的来源成本事件 ID 列表。
    pub source_event_ids: Vec<kiana_domain::EventId>,
    /// Domain 负责计算、校验成本迁移与总额的明细和合计。
    pub breakdown: ReceiptCostBreakdown,
}

impl CostReceiptProjection {
    /// 确认外层 Query envelope 与 Domain breakdown 的 Run/来源字段一致，再校验 breakdown 本身。
    ///
    /// 【调用者】`project_cost_receipt()` 返回前调用；消费者也可以在接收对象后再次调用。
    /// 【拒绝条件】schema/version、Run、非零 cursor、来源 ID 列表非空/长度/内容不一致，或
    /// breakdown 合同错误时返回 Query 层错误。错误只拒绝结果，不触发重试或写入。
    /// 【边界】这里只检查外层字段与 Domain 对象的一致性；金额和 attempt transition 由 Domain validator
    /// 负责，本函数不会访问原始事件重新计算金额。
    pub fn validate(&self) -> Result<(), CostQueryProjectionError> {
        if self.schema != COST_QUERY_PROJECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_QUERY_PROJECTION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.breakdown.run_id != self.run_id
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() != self.breakdown.source_event_ids.len()
            || self.source_event_ids != self.breakdown.source_event_ids
        {
            return Err(CostQueryProjectionError::Invalid(
                "cost_query_projection_header_invalid".to_owned(),
            ));
        }
        self.breakdown
            .validate()
            .map_err(CostQueryProjectionError::Invalid)
    }

    /// 读取 breakdown 保留的 Unknown 成本原因列表。
    ///
    /// 返回借用切片以避免复制；Unknown 是已知存在成本事实但金额不可确定的状态，不是空来源或零金额。
    pub fn unknown_reasons(&self) -> &[kiana_domain::BillingUnknownReason] {
        &self.breakdown.unknown_reasons
    }
}

/// 从调用方提供的 BQ-13 事件切片构建一个 Run 的成本回执投影。
///
/// 【调用者】当前仓内直接调用者是 BQ-13 Query fixture；Query crate 对外 re-export 此函数，但没有
/// 找到生产调用点。事件读取、流权限和 EventLog 快照边界由调用方负责。
/// 【输入】非 nil `run_id` 和任意 RuntimeEvent 切片；本模块将完整切片委托给 Domain reducer，由其挑选
/// 成本事件、验证 run/attempt/revision/迁移顺序并计算总额。
/// 【输出】存在成本事实时返回 `CostReceiptProjection`；没有任何相关成本事件时返回 `SourceEmpty`，
/// 不合成 0 金额。Unknown 事件属于真实成本来源，会留在 breakdown.unknown_reasons 中。
/// 【失败】nil RunId 返回 `RunInvalid`；Domain 错误映射到 `Invalid(String)`；外层绑定校验失败也拒绝
/// 整份结果。调用方收到错误后负责展示/处理，本函数不自动重试或修复源事实。
/// 【副作用】纯内存转换，只读切片；不读供应商价格、不写 EventLog、不修改 reservation 或授权转账。
/// 【核心流程】先检查 Run → 调 Domain reducer → 将 None 映射为 SourceEmpty → 复制 breakdown 来源信息
/// 到 envelope → 调 `validate()` 检查两层数据一致性。
pub fn project_cost_receipt(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostReceiptProjection, CostQueryProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(CostQueryProjectionError::RunInvalid);
    }
    let breakdown = project_receipt_cost_breakdown(run_id, events)
        .map_err(CostQueryProjectionError::Invalid)?
        .ok_or(CostQueryProjectionError::SourceEmpty)?;
    let result = CostReceiptProjection {
        schema: COST_QUERY_PROJECTION_SCHEMA,
        version: COST_QUERY_PROJECTION_VERSION,
        run_id,
        source_cursor: breakdown.source_cursor,
        source_event_ids: breakdown.source_event_ids.clone(),
        breakdown,
    };
    result.validate()?;
    Ok(result)
}

/// 供查询 adapter 使用的单数/read-model 名称；直接委托给 `project_cost_receipt()`。
///
/// 输入、错误、读取边界和投影语义都相同，不会额外进行一次 reducer 计算。
pub fn project_receipt_cost(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostReceiptProjection, CostQueryProjectionError> {
    project_cost_receipt(run_id, events)
}
