//! BQ-14 ControlPlane 成本修正准入与事件构造夹具（先拒绝、后成功）。
//!
//! 准入助手只校验“服务端已提交的目标 + 已绑定审批的版本化命令”，不写账本、不派发 capability、
//! 不调用 provider。夹具全部在内存中合成：审批记录是既有 `ApprovalDecisionRecord` 投影形状，
//! 不消费真实审批存储。

use async_trait::async_trait;
use kiana_capability_broker::CapabilityBroker;
use kiana_core::{ControlPlane, CostCorrectionAdmission, CostCorrectionAdmissionError};
use kiana_domain::{
    validate_event_payload, validate_runtime_event, ApprovalChallenge, ApprovalDecision,
    ApprovalDecisionRecord, ApprovalId, ApprovalState, AttemptId, CapabilityRequest,
    CostCorrectionApproval, CostCorrectionCommand, CostLedgerEntry, CostLedgerEntryKind, EventId,
    LedgerEntryId, Money, PendingApproval, RateCardId, RequestContext, RequestId, RiskLevel, RunId,
    COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
};
use kiana_eventlog::MemoryEventLog;
use kiana_gates::DefaultGateEngine;
use kiana_policy::DefaultPolicyEngine;
use kiana_ports::{ApprovalStorePort, PortError, RunnerPort};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use std::sync::Arc;

fn digest() -> String {
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()
}

fn money(micros: i128) -> Money {
    Money::new("USD", micros).expect("money")
}

fn target() -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        4,
        2,
        digest(),
    )
    .expect("target")
    .with_rate_card(RateCardId::new(), 4)
    .expect("rate card")
    .with_estimated_cost(money(500))
    .expect("estimate")
}

/// 未批准的命令（draft 状态），用于证明准入不会把“有命令”当成“已批准”。
fn draft_command(target: &CostLedgerEntry) -> CostCorrectionCommand {
    CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        target,
        Some(money(-25)),
        None,
        "invoice receipt reconciliation",
        vec!["receipt:opaque-1".to_owned()],
        "requester",
        "bq14-core-draft",
        20,
    )
    .expect("draft")
}

fn command(target: &CostLedgerEntry) -> CostCorrectionCommand {
    let draft = draft_command(target);
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        target.entry_digest.clone(),
        "approver",
        21,
    )
    .expect("approval");
    draft.with_approval(approval).expect("command")
}

/// 既有审批决策投影的夹具形状：批准同一 command 摘要、同一 approver。
fn decision_record(command: &CostCorrectionCommand) -> ApprovalDecisionRecord {
    let approval = command.approval.as_ref().expect("approved command");
    ApprovalDecisionRecord {
        approval_id: approval.approval_id,
        state: ApprovalState::Approved,
        challenge: ApprovalChallenge {
            schema: kiana_domain::APPROVAL_CHALLENGE_SCHEMA.to_owned(),
            approval_id: approval.approval_id,
            request_id: command.command_id,
            request_hash: command.command_digest.clone(),
            risk: RiskLevel::ExternalSideEffect,
            expires_at_unix_ms: 99_999,
            reason: "bq14 cost correction".to_owned(),
            nonce: String::new(),
            policy_version: String::new(),
        },
        decision: Some(ApprovalDecision::Approve),
        decision_command_id: Some(command.command_id),
        decided_by: Some("approver".to_owned()),
        dispatch_command_id: None,
        payload_available: false,
        version: 1,
        decision_digest: None,
        consumption_digest: None,
    }
}

struct UnavailableRunner;

#[async_trait]
impl RunnerPort for UnavailableRunner {
    async fn send(&self, _command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        Err(PortError::Unavailable("runner_unavailable".to_owned()))
    }
}

struct UnavailableApprovalStore;

#[async_trait]
impl ApprovalStorePort for UnavailableApprovalStore {
    async fn stage(
        &self,
        _context: &RequestContext,
        _request: CapabilityRequest,
        _reason: &str,
    ) -> Result<ApprovalChallenge, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn activate(&self, _approval_id: ApprovalId) -> Result<(), PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }

    async fn consume(
        &self,
        _context: &RequestContext,
        _approval_id: ApprovalId,
    ) -> Result<PendingApproval, PortError> {
        Err(PortError::Unavailable("approval_unavailable".to_owned()))
    }
}

fn plane() -> ControlPlane {
    ControlPlane::new(
        Arc::new(DefaultPolicyEngine),
        Arc::new(DefaultGateEngine),
        Arc::new(MemoryEventLog::new()),
        Arc::new(CapabilityBroker::new()),
        Arc::new(UnavailableApprovalStore),
        Arc::new(UnavailableRunner),
    )
}

#[test]
fn admission_refuses_unapproved_stale_or_foreign_commands() {
    let target = target();
    let approved = command(&target);
    let admitted = CostCorrectionAdmission::authorize(&target, &approved).expect("admitted");
    assert_eq!(admitted.target_entry_digest, target.entry_digest);
    assert_eq!(admitted.target_source_cursor, target.source_cursor);
    assert_eq!(admitted.target_revision, target.revision);
    assert_eq!(admitted.approver_id, "approver");

    // 有命令但没有审批：不得用布尔值或模型文本替代。
    let draft = draft_command(&target);
    assert_eq!(
        CostCorrectionAdmission::authorize(&target, &draft).unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_required".to_owned()
        )
    );

    // 目标 revision 漂移（重签摘要）仍是目标绑定失败。
    let mut stale = approved.clone();
    stale.target_revision += 1;
    stale.command_digest = stale.digest();
    assert_eq!(
        CostCorrectionAdmission::authorize(&target, &stale).unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_target_binding_mismatch".to_owned()
        )
    );

    // 另一条 entry 不能复用针对本条 entry 的已批准命令。
    let foreign = target();
    assert_eq!(
        CostCorrectionAdmission::authorize(&foreign, &approved).unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_target_binding_mismatch".to_owned()
        )
    );

    // 目标自身不可信（摘要不自洽）时先拒绝目标，不进入命令评估。
    let mut tampered_target = target.clone();
    tampered_target.created_at_unix_ms += 1;
    assert_eq!(
        CostCorrectionAdmission::authorize(&tampered_target, &approved).unwrap_err(),
        CostCorrectionAdmissionError::TargetInvalid("cost_ledger_entry_digest_mismatch".to_owned())
    );

    // 模型文本（换行）即使在摘要重签后也不能通过准入。
    let mut model_text = approved.clone();
    model_text.reason = "\nmodel transcript: raise the invoice".to_owned();
    model_text.command_digest = model_text.digest();
    assert_eq!(
        CostCorrectionAdmission::authorize(&target, &model_text).unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid("cost_correction_reason_invalid".to_owned())
    );
}

#[test]
fn approval_record_must_bind_the_exact_command_digest_and_approver() {
    let target = target();
    let approved = command(&target);
    let record = decision_record(&approved);
    CostCorrectionAdmission::authorize_with_approval_record(&target, &approved, &record)
        .expect("matching approval record");

    let mut wrong_hash = record.clone();
    wrong_hash.challenge.request_hash = digest();
    assert_eq!(
        CostCorrectionAdmission::authorize_with_approval_record(&target, &approved, &wrong_hash)
            .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_record_mismatch".to_owned()
        )
    );

    let mut wrong_command = record.clone();
    wrong_command.decision_command_id = Some(RequestId::new());
    assert_eq!(
        CostCorrectionAdmission::authorize_with_approval_record(&target, &approved, &wrong_command)
            .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_record_mismatch".to_owned()
        )
    );

    let mut wrong_approver = record.clone();
    wrong_approver.decided_by = Some("someone-else".to_owned());
    assert_eq!(
        CostCorrectionAdmission::authorize_with_approval_record(
            &target,
            &approved,
            &wrong_approver
        )
        .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_record_mismatch".to_owned()
        )
    );

    let mut wrong_approval_id = record.clone();
    wrong_approval_id.approval_id = ApprovalId::new();
    assert_eq!(
        CostCorrectionAdmission::authorize_with_approval_record(
            &target,
            &approved,
            &wrong_approval_id
        )
        .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_record_mismatch".to_owned()
        )
    );

    let mut denied = record;
    denied.decision = Some(ApprovalDecision::Deny);
    assert_eq!(
        CostCorrectionAdmission::authorize_with_approval_record(&target, &approved, &denied)
            .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_record_mismatch".to_owned()
        )
    );

    // 命令本身没有审批材料时不能靠外部记录补一张“许可”。
    assert_eq!(
        CostCorrectionAdmission::authorize_with_approval_record(
            &target,
            &draft_command(&target),
            &decision_record(&approved)
        )
        .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_required".to_owned()
        )
    );
}

#[test]
fn event_builders_are_append_only_stream_bound_and_validated() {
    let target = target();
    let entry_event =
        CostCorrectionAdmission::ledger_entry_event(RequestId::new(), 1, &target).expect("entry");
    assert_eq!(entry_event.kind, COST_LEDGER_ENTRY_EVENT);
    assert_eq!(entry_event.aggregate_type.as_deref(), Some("cost_ledger"));
    assert_eq!(
        entry_event.aggregate_id.as_deref(),
        Some(target.entry_id.to_string().as_str())
    );
    assert_eq!(entry_event.stream_version, Some(target.revision));
    validate_runtime_event(&entry_event).expect("registered ledger entry event");

    let mut invalid_target = target.clone();
    invalid_target.created_at_unix_ms += 1;
    assert_eq!(
        CostCorrectionAdmission::ledger_entry_event(RequestId::new(), 1, &invalid_target)
            .unwrap_err(),
        CostCorrectionAdmissionError::TargetInvalid("cost_ledger_entry_digest_mismatch".to_owned())
    );

    let correction =
        CostCorrectionAdmission::authorize(&target, &command(&target)).expect("correction");
    assert_eq!(
        CostCorrectionAdmission::correction_event(RequestId::new(), 2, &correction, 0).unwrap_err(),
        CostCorrectionAdmissionError::EventInvalid("stream_version_invalid".to_owned())
    );

    let correction_event = CostCorrectionAdmission::correction_event(
        RequestId::new(),
        2,
        &correction,
        target.revision + 1,
    )
    .expect("correction event");
    assert_eq!(correction_event.kind, COST_CORRECTION_EVENT);
    assert_eq!(
        correction_event.aggregate_id.as_deref(),
        Some(target.entry_id.to_string().as_str())
    );
    assert_eq!(correction_event.stream_version, Some(target.revision + 1));
    validate_runtime_event(&correction_event).expect("registered correction event");
    validate_event_payload(COST_CORRECTION_EVENT, &correction_event.data).expect("payload");

    let payload = CostCorrectionAdmission::command_payload(&command(&target)).expect("payload");
    assert_eq!(payload["approver_id"], "approver");
    assert_eq!(payload["requested_by"], "requester");
    assert!(payload.get("transcript").is_none());
    assert_eq!(
        CostCorrectionAdmission::command_payload(&draft_command(&target)).unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_required".to_owned()
        )
    );
}

#[test]
fn control_plane_facade_admits_only_approved_bound_commands() {
    let plane = plane();
    let target = target();
    let approved = command(&target);
    let expected_approval_id = approved.approval.as_ref().expect("approval").approval_id;
    let admitted = plane
        .admit_cost_correction(&target, &approved)
        .expect("admitted");
    assert_eq!(admitted.target_entry_digest, target.entry_digest);
    assert_eq!(admitted.approval_id, expected_approval_id);

    assert_eq!(
        plane
            .admit_cost_correction(&target, &draft_command(&target))
            .unwrap_err(),
        CostCorrectionAdmissionError::CommandInvalid(
            "cost_correction_approval_required".to_owned()
        )
    );

    let record = decision_record(&approved);
    assert!(plane
        .admit_cost_correction_with_approval(&target, &approved, &record)
        .is_ok());
}
