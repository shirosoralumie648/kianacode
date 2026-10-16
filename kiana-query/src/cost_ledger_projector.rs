//! 本文件从已提交的账本 entry 与 correction 事件重建 BQ-14 成本账本查询视图。
//!
//! 在系统中的位置：上游 Core/Daemon 负责授权、追加和持久化不可变账本事件；Query 收到事件切片后，
//! 按顺序重建 Domain `CostCorrectionLedger`，最后为每条原始 entry 派生 correction view。仓内当前
//! 直接调用只见 BQ-14 fixture；crate 根模块公开导出，但没有找到生产路由消费点。
//!
//! ```text
//! committed cost.ledger_entry / cost.corrected events
//!   -> 事件 kind、sequence、aggregate、run、revision 校验
//!   -> 原始 CostLedgerEntry 追加到内存 reducer
//!   -> committed CostCorrection 还原成含原审批证据的 reducer command
//!   -> Domain append_correction 校验目标/审批/幂等合同
//!   -> 每个原 entry 的 CostLedgerView
//! ```
//!
//! correction 是追加事实，不会覆盖原 entry。这里重建 command 只是适配 Domain reducer 的输入类型，
//! 使用 correction 事件已经携带的审批字段，不推断新的审批。Query 不写 EventLog、不修改原条目，
//! 也不接受模型 transcript 作为改账依据。

use kiana_domain::{
    CostCorrection, CostCorrectionLedger, CostLedgerEntry, CostLedgerView, EventId, RunId,
    RuntimeEvent, SchemaVersion, COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
};
use std::collections::BTreeSet;

pub const COST_LEDGER_PROJECTION_SCHEMA: &str = "kiana.cost-ledger-projection.v1";
pub const COST_LEDGER_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// 成本账本投影拒绝原因的 Query 层分类。
///
/// `Invalid(String)` 承接事件解码、run/aggregate/revision 绑定及 Domain ledger 错误；任一事实无效时
/// 整份 projection 失败，不返回部分账本。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CostLedgerProjectionError {
    /// 目标 RunId 是 nil，不能确定查询范围。
    #[error("cost_ledger_projection_run_invalid")]
    RunInvalid,
    /// 没有原始 entry 或 correction 事件。
    #[error("cost_ledger_projection_source_empty")]
    SourceEmpty,
    /// source 事件或内存 reducer 处理失败。
    #[error("cost_ledger_projection_invalid:{0}")]
    Invalid(String),
}

/// 一次 Run 的账本重建结果：保留输入来源、Domain reducer 状态和逐 entry 派生 view。
///
/// 这是 Query 内存 projection，不提供 Serde 自动编解码，也不负责持久化。`ledger` 保留不可变原始
/// entry/correction 事实；`views` 是查询时由这些事实派生的结果，不是第二份可写账本。
#[derive(Clone, Debug, PartialEq)]
pub struct CostLedgerProjection {
    /// 固定 Query projection schema。
    pub schema: &'static str,
    /// 当前投影合同版本。
    pub version: SchemaVersion,
    /// 所有 entry 和 correction 必须属于的 Run。
    pub run_id: RunId,
    /// 被消费的账本事件中最大的全局 sequence。
    pub source_cursor: u64,
    /// 实际纳入本投影的账本事件 ID，按输入顺序排列。
    pub source_event_ids: Vec<EventId>,
    /// Domain 的内存 append-only reducer，保存原 entry 与已接受 correction。
    pub ledger: CostCorrectionLedger,
    /// 每个原 entry 对应的原值与 correction 组合视图。
    pub views: Vec<CostLedgerView>,
}

impl CostLedgerProjection {
    /// 校验投影 schema/version、Run、来源游标/ID 和各 view 的 Run 归属。
    ///
    /// 【调用者】`project_cost_ledger()` 在构造完 projection 后调用；下游也可再次验证。
    /// 【失败】头字段无效、来源为空/包含 nil ID，或任一 view 跨 Run 时返回 `Invalid`。
    /// 【边界】该类型字段可公开构造，但 validator 不重放 ledger、不逐项比对 `source_event_ids` 与
    /// reducer 内的事实，也不核对 views 是否完整覆盖 ledger；生产 builder 路径负责这些转换。
    pub fn validate(&self) -> Result<(), CostLedgerProjectionError> {
        if self.schema != COST_LEDGER_PROJECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&COST_LEDGER_PROJECTION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
        {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_header_invalid".to_owned(),
            ));
        }
        if self
            .views
            .iter()
            .any(|view| view.original.run_id != self.run_id)
        {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_run_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// 按原始 ledger entry ID 查找派生 view；没有该 entry 时返回 None。
    pub fn view_for(&self, entry_id: kiana_domain::LedgerEntryId) -> Option<&CostLedgerView> {
        self.views
            .iter()
            .find(|view| view.original.entry_id == entry_id)
    }
}

/// 按 EventLog 顺序重建一个 Run 的原始账本和 correction view。
///
/// 【调用者】当前仓内直接调用者为 `kiana-query/tests/bq14_cost_ledger_projector.rs`；API 对外导出，
/// 但没有找到生产 route caller。读取、授权和事件快照由调用方负责。
/// 【输入】目标 RunId 与 RuntimeEvent 切片；只处理 `cost.ledger_entry` 和 `cost.corrected`，其他事件
/// 跳过，也不参与选中账本事件间的 sequence 检查。
/// 【输出】成功返回 reducer 和按原始 entry 生成的 views；source_event_ids 与 source_cursor 仅覆盖
/// 本函数识别并成功处理的两类事件。
/// 【核心流程】检查 Run → 过滤账本事件并校验唯一 ID/递增 sequence → 解码/绑定原 entry 或 correction
/// → 依事件顺序提交到内存 reducer → 逐 entry 计算 view → 校验外层 projection。
/// 【失败】nil Run、空来源、重复事件 ID、账本事件 sequence 不递增、payload 无效、跨 Run、aggregate
/// 不匹配、entry revision 错误、correction 缺少目标 entry 或 Domain 审批/幂等验证失败，都会使整次
/// 调用返回错误。错误不会更改持久 EventLog，但不会返回部分 view。
/// 【为什么依赖顺序】correction reducer 必须先看到目标原 entry；把 correction 排在 entry 前会得到
/// target missing。调用方应传入按事件 sequence 排列的历史，不能把事件集合任意重排后重放。
pub fn project_cost_ledger(
    run_id: RunId,
    events: &[RuntimeEvent],
) -> Result<CostLedgerProjection, CostLedgerProjectionError> {
    if run_id.as_uuid().is_nil() {
        return Err(CostLedgerProjectionError::RunInvalid);
    }
    let mut ledger = CostCorrectionLedger::default();
    let mut source_event_ids = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    let mut source_cursor = 0;
    let mut previous_sequence = 0;
    let run_text = run_id.to_string();
    for event in events {
        // 非账本事件不影响本投影的 previous_sequence；检查的是被纳入账本的事件之间顺序。
        if event.kind != COST_LEDGER_ENTRY_EVENT && event.kind != COST_CORRECTION_EVENT {
            continue;
        }
        if !seen_event_ids.insert(event.event_id) {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_duplicate_source_event".to_owned(),
            ));
        }
        if event.sequence == 0 || event.sequence <= previous_sequence {
            return Err(CostLedgerProjectionError::Invalid(
                "cost_ledger_projection_sequence_regression".to_owned(),
            ));
        }
        previous_sequence = event.sequence;
        source_cursor = source_cursor.max(event.sequence);
        source_event_ids.push(event.event_id);
        match event.kind.as_str() {
            COST_LEDGER_ENTRY_EVENT => {
                // 原始 entry 事件必须绑定自己的 ID、Run 和 revision，随后由 Domain ledger 检查重复与摘要。
                let entry: CostLedgerEntry =
                    serde_json::from_value(event.data.clone()).map_err(|_| {
                        CostLedgerProjectionError::Invalid("entry_decode_failed".to_owned())
                    })?;
                entry
                    .validate()
                    .map_err(CostLedgerProjectionError::Invalid)?;
                if entry.run_id != run_id
                    || event.data.get("run_id").and_then(serde_json::Value::as_str)
                        != Some(run_text.as_str())
                    || event.aggregate_type.as_deref() != Some("cost_ledger")
                    || event.aggregate_id.as_deref() != Some(entry.entry_id.to_string().as_str())
                    || event.stream_version != Some(entry.revision)
                {
                    return Err(CostLedgerProjectionError::Invalid(
                        "entry_identity_or_revision_mismatch".to_owned(),
                    ));
                }
                ledger
                    .append_entry(entry)
                    .map_err(CostLedgerProjectionError::Invalid)?;
            }
            COST_CORRECTION_EVENT => {
                // Correction 事件包含已审批事实；只检查 envelope 绑定并还原成 reducer 命令，
                // Domain reducer 会重新验证命令摘要、目标 entry、审批 proof 和幂等键。
                let correction: CostCorrection = serde_json::from_value(event.data.clone())
                    .map_err(|_| {
                        CostLedgerProjectionError::Invalid("correction_decode_failed".to_owned())
                    })?;
                if correction.run_id != run_id
                    || event.data.get("run_id").and_then(serde_json::Value::as_str)
                        != Some(run_text.as_str())
                    || event.aggregate_type.as_deref() != Some("cost_ledger")
                    || event.aggregate_id.as_deref()
                        != Some(correction.target_entry_id.to_string().as_str())
                    || event.stream_version.is_none_or(|version| version == 0)
                {
                    return Err(CostLedgerProjectionError::Invalid(
                        "correction_identity_or_revision_mismatch".to_owned(),
                    ));
                }
                let command = correction_to_command(&correction);
                ledger
                    .append_correction(command)
                    .map_err(CostLedgerProjectionError::Invalid)?;
            }
            _ => unreachable!("filtered above"),
        }
    }
    if source_event_ids.is_empty() {
        return Err(CostLedgerProjectionError::SourceEmpty);
    }
    let views = ledger
        .entries()
        .map(|entry| ledger.view(entry.entry_id))
        .collect::<Result<Vec<_>, _>>()
        .map_err(CostLedgerProjectionError::Invalid)?;
    let projection = CostLedgerProjection {
        schema: COST_LEDGER_PROJECTION_SCHEMA,
        version: COST_LEDGER_PROJECTION_VERSION,
        run_id,
        source_cursor,
        source_event_ids,
        ledger,
        views,
    };
    projection.validate()?;
    Ok(projection)
}

/// 将不可变 correction 事实映射回 Domain reducer 接受的已审批 command 形状。
///
/// reducer 的 append API 接受 command，而 EventLog 中保存的是已确认的 `CostCorrection` 事实；此处
/// 从事实逐字段还原原 command 和它的 approval envelope，让 reducer 重用同一验证/幂等路径。不会
/// 创建新的审批、改变 correction，或绕过 `append_correction()` 的 Domain 校验。
fn correction_to_command(correction: &CostCorrection) -> kiana_domain::CostCorrectionCommand {
    let approval = kiana_domain::CostCorrectionApproval {
        schema: kiana_domain::COST_CORRECTION_APPROVAL_SCHEMA.to_owned(),
        version: kiana_domain::COST_LEDGER_VERSION,
        approval_id: correction.approval_id,
        command_id: correction.command_id,
        command_digest: correction.command_digest.clone(),
        target_entry_digest: correction.target_entry_digest.clone(),
        approver_id: correction.approver_id.clone(),
        approved_at_unix_ms: correction.approved_at_unix_ms,
        approval_digest: correction.approval_digest.clone(),
    };
    kiana_domain::CostCorrectionCommand {
        schema: kiana_domain::COST_CORRECTION_COMMAND_SCHEMA.to_owned(),
        version: kiana_domain::COST_LEDGER_VERSION,
        command_id: correction.command_id,
        correction_id: correction.correction_id,
        target_entry_id: correction.target_entry_id,
        target_entry_digest: correction.target_entry_digest.clone(),
        target_run_id: correction.target_run_id,
        target_attempt_id: correction.target_attempt_id,
        target_source_cursor: correction.target_source_cursor,
        target_revision: correction.target_revision,
        delta_estimated: correction.delta_estimated.clone(),
        delta_measured: correction.delta_measured.clone(),
        reason: correction.reason.clone(),
        evidence_refs: correction.evidence_refs.clone(),
        requested_by: correction.requested_by.clone(),
        idempotency_key: correction.idempotency_key.clone(),
        created_at_unix_ms: correction.created_at_unix_ms,
        approval: Some(approval),
        command_digest: correction.command_digest.clone(),
    }
}
