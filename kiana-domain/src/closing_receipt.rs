//! CO-35 Company 诚实收尾（ClosingReceipt）合同。
//!
//! # 文件职责与数据流
//!
//! 一个模型运行结束、CLI 显示成功或聊天文本说“完成”，都不能直接代表整个 Company project
//! 已经关闭。本文件聚合并校验 packet/attempt、Run、review/acceptance、delivery、incident、
//! evidence、停止状态和残余义务，形成可查询的 typed receipt：
//!
//! ```text
//! Company/EventLog 中已提交的业务事实
//!   -> packet/attempt/run/review/acceptance/delivery/incident/evidence 引用
//!   -> CompanyClosingReceiptContract
//!   -> 【当前文件】按 Success/Failure/Cancelled/Waived 校验
//!   -> CompanyClosingReceiptLedger 幂等记录
//!   -> Company read model / closing projection
//! ```
//!
//! 这是 source projection/validation，不是 Company transition 执行器。真实状态提交仍由
//! `CompanyState`/EventLog/既有 closeout 流程负责；receipt 不能从聊天、模型自述或一次 runtime
//! result 推断项目关闭，也不能替代 Outcome、durable replay、live/physical 业务证据。
//!
//! 四种 close kind 有意使用不同条件：Success 要求接受和确认交付且无残余义务，Failure 要求
//! 诚实失败状态和全部停止，Cancelled 要求取消状态和停止，Waived 要求独立 Sponsor waiver
//! 与非空残余义务。把它们合成一个“completed”标志会丢失项目治理语义。

use crate::{json_digest, AcceptanceStatus, DeliveryStatus, ProjectStatus};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COMPANY_CLOSING_RECEIPT_SCHEMA: &str = "kiana.company-closing-receipt.v2";

/// 检查一个 receipt 文本字段非空、长度受限且不含控制字符。
///
/// 该 helper 只做形状检查；它不验证 ID 是否存在于 EventLog，也不验证引用来源或调用者
/// 权限。字段错误码由 `validate` 向 Core/Company 调用者传播。
fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

/// 检查引用列表的非空/数量/去重/空白规则。
///
/// `allow_empty` 由不同 close kind 决定：例如成功必须有 delivery/evidence，Failure/Cancelled
/// 可以没有 delivery；允许为空不等于引用可以重复或包含空字符串。
fn references(
    values: &[String],
    field: &'static str,
    allow_empty: bool,
) -> Result<(), &'static str> {
    if (!allow_empty && values.is_empty()) || values.len() > 512 {
        return Err(field);
    }
    if values.iter().any(|value| value.trim().is_empty())
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(field);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Company 项目收尾的四种业务语义。
pub enum CompanyCloseKind {
    /// 项目已正式成功交付并满足所有接受条件。
    Success,
    /// 项目以诚实失败状态收尾，不能冒充成功。
    Failure,
    /// 项目被明确取消，要求所有运行停止。
    Cancelled,
    /// Sponsor 明确豁免剩余义务，但必须保留残余义务和独立 waiver 证据。
    Waived,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一份不可变 Company ClosingReceipt 合同。
///
/// 所有引用和状态都来自调用方已提交的业务事实；本类型只负责把它们绑定起来，并按 close
/// kind 检查完整性、停止证明、角色独立和摘要一致性。`digest` 是内容指纹，不是签名。
pub struct CompanyClosingReceiptContract {
    /// 固定 receipt schema。
    pub schema: String,
    /// receipt 自身稳定 ID；ledger 以它做幂等键。
    pub receipt_id: String,
    /// 被关闭的 Company project ID。
    pub project_id: String,
    /// 生成 receipt 时使用的业务基线版本，零值无效。
    pub baseline_version: u64,
    /// 收尾类型，决定后续条件分支。
    pub close_kind: CompanyCloseKind,
    /// receipt 声称的项目状态。
    pub project_status: ProjectStatus,
    /// receipt 声称的 acceptance 状态。
    pub acceptance_status: AcceptanceStatus,
    /// 交付状态；Success 必须为 Confirmed，其他类型可按合同为空。
    pub delivery_status: Option<DeliveryStatus>,
    /// 收尾原因，必须非空。
    pub reason: String,
    /// packet ID 到 attempt ID 的绑定引用。
    pub packet_attempt_refs: BTreeMap<String, String>,
    /// 关联 Run 引用，必须非空且去重。
    pub run_refs: Vec<String>,
    /// 关联 review 引用。
    pub review_refs: Vec<String>,
    /// 关联 acceptance 引用。
    pub acceptance_refs: Vec<String>,
    /// 关联 delivery 引用；Success 必须有，其他类型可按规则为空。
    pub delivery_refs: Vec<String>,
    /// 关联 incident 引用；列表可为空，但不能重复或包含空值。
    pub incident_refs: Vec<String>,
    /// 支撑关闭结论的 evidence 引用，必须非空。
    pub evidence_refs: Vec<String>,
    /// 尚未完成但被 Waived 明确保留的残余义务。
    pub residual_obligations: Vec<String>,
    /// 是否所有关联 Run 已停止；四种类型都依赖此边界。
    pub all_runs_stopped: bool,
    /// 是否仍有未解决 incident；true 时任何 close kind 都拒绝。
    pub unresolved_incidents: bool,
    /// 产生工作的作者列表；用于角色独立检查。
    pub authors: Vec<String>,
    /// 独立 reviewer 引用列表。
    pub reviewer_refs: Vec<String>,
    /// 负责关闭的人；不能与作者/reviewer 冲突，Waived 时不能等于 waiver_by。
    pub closer_id: String,
    #[serde(default)]
    /// Sponsor waiver 的外部引用；仅 Waived 必需，其他类型禁止。
    pub waiver_ref: Option<String>,
    #[serde(default)]
    /// 授予 waiver 的主体；必须独立于 closer。
    pub waiver_by: Option<String>,
    /// 收尾时间戳（Unix 毫秒/单位由上层约定），零值无效。
    pub closed_at: u64,
    /// 对全部业务字段（不含自身 digest）重算的内容摘要。
    pub digest: String,
}

impl CompanyClosingReceiptContract {
    /// 校验 receipt 的公共字段、引用集合、角色独立和四种 close kind 专属条件。
    ///
    /// 【检查顺序】
    /// 1. schema、baseline、关闭时间和关键 ID；
    /// 2. packet/attempt、Run、review、acceptance、delivery、incident、evidence、残余义务的
    ///    非空/上限/去重；
    /// 3. 作者、reviewer、closer 不自审且没有未解决 incident；
    /// 4. 按 Success/Failure/Cancelled/Waived 检查状态、接受、交付、停止、waiver 和残余义务；
    /// 5. 重算 canonical digest，拒绝字段被替换却沿用旧摘要的 receipt。
    ///
    /// 【失败与副作用】
    /// 返回稳定静态错误码，不会提交 Company transition、写 EventLog、关闭 Run、发送通知或
    /// 自动补齐引用。成功只说明这份 source receipt 自洽，不能说明引用对应的外部事实已经
    /// durable/live/physical 验证。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_CLOSING_RECEIPT_SCHEMA
            || self.baseline_version == 0
            || self.closed_at == 0
        {
            return Err("closing_receipt_header_invalid");
        }
        for (value, field) in [
            (&self.receipt_id, "closing_receipt_id_required"),
            (&self.project_id, "closing_receipt_project_required"),
            (&self.reason, "closing_receipt_reason_required"),
            (&self.closer_id, "closing_receipt_closer_required"),
        ] {
            required(value, field)?;
        }
        if self.packet_attempt_refs.is_empty() {
            return Err("closing_receipt_packet_attempts_required");
        }
        if self.packet_attempt_refs.len() > 512 {
            return Err("closing_receipt_packet_attempts_limit");
        }
        for (packet, attempt) in &self.packet_attempt_refs {
            required(packet, "closing_receipt_packet_ref_invalid")?;
            required(attempt, "closing_receipt_attempt_ref_invalid")?;
        }
        references(&self.run_refs, "closing_receipt_runs_required", false)?;
        references(&self.review_refs, "closing_receipt_reviews_required", false)?;
        references(
            &self.acceptance_refs,
            "closing_receipt_acceptances_required",
            false,
        )?;
        references(
            &self.delivery_refs,
            "closing_receipt_deliveries_required",
            self.close_kind != CompanyCloseKind::Success,
        )?;
        references(
            &self.incident_refs,
            "closing_receipt_incidents_invalid",
            true,
        )?;
        references(
            &self.evidence_refs,
            "closing_receipt_evidence_required",
            false,
        )?;
        references(
            &self.residual_obligations,
            "closing_receipt_residual_invalid",
            true,
        )?;
        if self.authors.is_empty() || self.authors.len() > 512 {
            return Err("closing_receipt_authors_required");
        }
        for author in &self.authors {
            required(author, "closing_receipt_author_invalid")?;
        }
        references(
            &self.reviewer_refs,
            "closing_receipt_reviewers_required",
            false,
        )?;
        if self.authors.iter().any(|author| author == &self.closer_id)
            || self
                .reviewer_refs
                .iter()
                .any(|reviewer| reviewer == &self.closer_id)
            || self.authors.iter().collect::<BTreeSet<_>>().len() != self.authors.len()
        {
            return Err("closing_receipt_role_independence_invalid");
        }
        if self.unresolved_incidents {
            return Err("closing_receipt_unresolved_incident");
        }
        match self.close_kind {
            CompanyCloseKind::Success => {
                if !matches!(
                    self.project_status,
                    ProjectStatus::Accepted | ProjectStatus::Closed
                ) || self.acceptance_status != AcceptanceStatus::Accepted
                    || self.delivery_status != Some(DeliveryStatus::Confirmed)
                    || !self.all_runs_stopped
                    || !self.residual_obligations.is_empty()
                    || self.waiver_ref.is_some()
                    || self.waiver_by.is_some()
                {
                    return Err("closing_success_chain_incomplete");
                }
            }
            CompanyCloseKind::Failure => {
                if !matches!(
                    self.project_status,
                    ProjectStatus::Active
                        | ProjectStatus::AtRisk
                        | ProjectStatus::ReadyForAcceptance
                        | ProjectStatus::Failed
                ) || !self.all_runs_stopped
                {
                    return Err("closing_failure_stop_or_status_invalid");
                }
                if self.waiver_ref.is_some() || self.waiver_by.is_some() {
                    return Err("closing_failure_waiver_forbidden");
                }
            }
            CompanyCloseKind::Cancelled => {
                if self.project_status != ProjectStatus::Cancelled || !self.all_runs_stopped {
                    return Err("closing_cancel_stop_required");
                }
                if self.waiver_ref.is_some() || self.waiver_by.is_some() {
                    return Err("closing_cancel_waiver_forbidden");
                }
            }
            CompanyCloseKind::Waived => {
                if !matches!(
                    self.project_status,
                    ProjectStatus::Accepted | ProjectStatus::Closed
                ) || self.acceptance_status != AcceptanceStatus::Waived
                    || !self.all_runs_stopped
                    || self.residual_obligations.is_empty()
                    || self.waiver_ref.as_deref().is_none_or(str::is_empty)
                    || self.waiver_by.as_deref().is_none_or(str::is_empty)
                {
                    return Err("closing_waiver_requirements_missing");
                }
                if self.waiver_by.as_deref() == Some(self.closer_id.as_str()) {
                    return Err("closing_waiver_self_approval_forbidden");
                }
            }
        }
        if self.digest != self.canonical_digest() {
            return Err("closing_receipt_digest_mismatch");
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 的 ClosingReceipt canonical digest。
    ///
    /// close kind、状态、所有引用、角色、waiver、停止/incident 标记和时间都会参与摘要；
    /// 任何字段改变都必须重新计算，否则 `validate()` 拒绝。摘要是完整性指纹，不是签名。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "project_id": self.project_id,
            "baseline_version": self.baseline_version,
            "close_kind": self.close_kind,
            "project_status": self.project_status,
            "acceptance_status": self.acceptance_status,
            "delivery_status": self.delivery_status,
            "reason": self.reason,
            "packet_attempt_refs": self.packet_attempt_refs,
            "run_refs": self.run_refs,
            "review_refs": self.review_refs,
            "acceptance_refs": self.acceptance_refs,
            "delivery_refs": self.delivery_refs,
            "incident_refs": self.incident_refs,
            "evidence_refs": self.evidence_refs,
            "residual_obligations": self.residual_obligations,
            "all_runs_stopped": self.all_runs_stopped,
            "unresolved_incidents": self.unresolved_incidents,
            "authors": self.authors,
            "reviewer_refs": self.reviewer_refs,
            "closer_id": self.closer_id,
            "waiver_ref": self.waiver_ref,
            "waiver_by": self.waiver_by,
            "closed_at": self.closed_at,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
/// 进程内的 ClosingReceipt 幂等 ledger。
///
/// 它按 receipt ID 保存完整合同，并防止同一 project 被写入第二个不同 receipt。这个 ledger
/// 是内存/投影边界，不等于跨进程 durable EventLog；真正持久化仍由 Company/EventLog 路径负责。
pub struct CompanyClosingReceiptLedger {
    /// receipt ID 到完整合同的索引。
    pub receipts: BTreeMap<String, CompanyClosingReceiptContract>,
}

impl CompanyClosingReceiptLedger {
    /// 验证并幂等记录一份 ClosingReceipt。
    ///
    /// 相同 receipt ID 且 digest 相同视为安全重放并返回成功；相同 ID 但 digest 不同拒绝，
    /// 防止覆盖已看到的事实；不同 receipt ID 但 project 相同也拒绝，保证一个项目只能完成
    /// 一次收尾。函数只修改调用方提供的内存 map，不写 EventLog 或自动提交 Company 状态。
    pub fn record(&mut self, receipt: CompanyClosingReceiptContract) -> Result<(), &'static str> {
        receipt.validate()?;
        if let Some(existing) = self.receipts.get(&receipt.receipt_id) {
            if existing.digest == receipt.digest {
                return Ok(());
            }
            return Err("closing_receipt_duplicate_digest_mismatch");
        }
        if self
            .receipts
            .values()
            .any(|existing| existing.project_id == receipt.project_id)
        {
            return Err("closing_receipt_project_already_closed");
        }
        self.receipts.insert(receipt.receipt_id.clone(), receipt);
        Ok(())
    }

    /// 按 project ID 查询已记录的 ClosingReceipt。
    ///
    /// 返回借用值，不创建新 receipt、不验证外部项目状态；不存在时返回 `None`。查询到值
    /// 也只代表 ledger 中有一份通过 Domain 校验的投影，不能替代 CompanyState/EventLog。
    pub fn for_project(&self, project_id: &str) -> Option<&CompanyClosingReceiptContract> {
        self.receipts
            .values()
            .find(|receipt| receipt.project_id == project_id)
    }
}
