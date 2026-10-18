//! BQ-14 append-only ledger Query projection 的合成事实 fixture（先拒绝、后成功）。
//!
//! 用例在内存中构造原始 entry、带审批字段的 correction 和事件 envelope，检查 reducer 重建 view、
//! 外层 Run 绑定、sequence/聚合绑定与重复来源拒绝；不连接 EventStore，也不提交真实审批或财务事实。

use kiana_domain::{
    ApprovalId, AttemptId, CostCorrection, CostCorrectionApproval, CostCorrectionCommand,
    CostLedgerEntry, CostLedgerEntryKind, EventId, LedgerEntryId, Money, RateCardId, RequestId,
    RunId, RuntimeEvent, COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
};
use kiana_query::{
    project_cost_ledger, CostLedgerProjection, CostLedgerProjectionError,
    COST_LEDGER_PROJECTION_SCHEMA, COST_LEDGER_PROJECTION_VERSION,
};
use serde_json::json;

/// 固定重复字符的 sha256 形状 fixture，不是真实内容摘要。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn money(micros: i128) -> Money {
    Money::new("USD", micros).expect("money")
}

/// 构造消费类型原始 ledger entry，再附上 rate card 与估算金额。
fn entry() -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        10,
        1,
        digest('a'),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 3)
    .expect("card")
    .with_estimated_cost(money(900))
    .expect("estimate")
}

/// 从临时 draft 生成合成 approval 和 correction 事实，用于测试已批准 correction 的重放。
fn correction(
    entry: &CostLedgerEntry,
    delta_estimated: i128,
    delta_measured: Option<i128>,
    idempotency_key: &str,
) -> CostCorrection {
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        kiana_domain::CostCorrectionId::new(),
        entry,
        Some(money(delta_estimated)),
        delta_measured.map(money),
        "invoice reconciliation",
        vec!["invoice:opaque".to_owned()],
        "requester",
        idempotency_key,
        11,
    )
    .expect("draft");
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "approver",
        12,
    )
    .expect("approval");
    CostCorrection::from_command(&draft.with_approval(approval).expect("approved"), entry)
        .expect("correction")
}

fn entry_event(entry: &CostLedgerEntry, sequence: u64) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(entry).expect("entry json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), entry.revision)
}

fn correction_event(
    entry: &CostLedgerEntry,
    correction: &CostCorrection,
    sequence: u64,
) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_CORRECTION_EVENT,
        serde_json::to_value(correction).expect("correction json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_ledger",
        correction.target_entry_id.to_string(),
        entry.revision + 1,
    )
}

/// 事件 sequence 按原 entry、correction 顺序递增；aggregate stream version 对应各自 revision。
fn events(entry: &CostLedgerEntry, correction: &CostCorrection) -> Vec<RuntimeEvent> {
    vec![
        entry_event(entry, 1),
        correction_event(entry, correction, 2),
    ]
}

/// Re-seal the digest chain after a semantically valid mutation of a correction fact.
///
/// A `CostCorrection` carries three digest levels: the command digest (the approval subject), the
/// approval digest (binding the command digest to the approver) and the correction digest (the
/// whole fact). Mutating a bound field such as `target_run_id` invalidates all three; recomputing
/// them keeps the fixture on the intended domain refusal instead of stopping at the generic
/// `cost_correction_command_digest_mismatch` gate.
fn reseal(correction: &mut CostCorrection) {
    // The command digest is computed over the command schema, not the correction fact schema.
    let command = CostCorrectionCommand {
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
        approval: None,
        command_digest: String::new(),
    };
    let command_digest = command.digest();
    let approval = CostCorrectionApproval::new(
        correction.approval_id,
        correction.command_id,
        command_digest.clone(),
        correction.target_entry_digest.clone(),
        correction.approver_id.clone(),
        correction.approved_at_unix_ms,
    )
    .expect("approval");
    correction.command_digest = command_digest;
    correction.approval_digest = approval.approval_digest;
    correction.correction_digest = String::new();
    correction.correction_digest = correction.digest();
}

/// 只接受 `Invalid` 分支并返回稳定原因串，其他结局视为夹具写错。
fn invalid_reason(result: Result<CostLedgerProjection, CostLedgerProjectionError>) -> String {
    match result {
        Err(CostLedgerProjectionError::Invalid(reason)) => reason,
        other => panic!("expected Invalid projection error, got {other:?}"),
    }
}

/// 重建后原 entry digest 保持不变，-100 delta 只体现在 view 的 800 估算结果中。
#[test]
fn projector_rebuilds_original_and_append_only_correction() {
    let entry = entry();
    let correction = correction(&entry, -100, None, "bq14-query-command");
    let projection =
        project_cost_ledger(entry.run_id, &events(&entry, &correction)).expect("projection");
    let view = projection.view_for(entry.entry_id).expect("view");
    assert_eq!(view.original.entry_digest, entry.entry_digest);
    assert_eq!(view.estimated_cost.as_ref().expect("estimate").micros, 800);
    assert_eq!(view.corrections.len(), 1);
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.source_event_ids.len(), 2);
}

/// 外层 Run 不匹配及重复 EventId 都应拒绝整次重建。
#[test]
fn projector_rejects_foreign_run_and_duplicate_source_event() {
    let entry = entry();
    let correction = correction(&entry, -100, None, "bq14-query-command-2");
    let source = events(&entry, &correction);
    let foreign = RunId::new();
    assert!(matches!(
        project_cost_ledger(foreign, &source),
        Err(CostLedgerProjectionError::Invalid(_))
    ));
    let mut duplicated = source;
    duplicated.push(duplicated[0].clone());
    assert!(matches!(
        project_cost_ledger(entry.run_id, &duplicated),
        Err(CostLedgerProjectionError::Invalid(_))
    ));
}

/// correction 先于目标 entry、或 sequence 非递增时，整份投影失败而不是部分发布。
#[test]
fn projector_rejects_correction_before_its_target_and_sequence_regression() {
    let entry = entry();
    let correction = correction(&entry, -100, None, "bq14-query-order");

    let before_target = vec![
        correction_event(&entry, &correction, 1),
        entry_event(&entry, 2),
    ];
    assert_eq!(
        invalid_reason(project_cost_ledger(entry.run_id, &before_target)),
        "cost_correction_target_missing"
    );

    let regressed = vec![
        entry_event(&entry, 2),
        correction_event(&entry, &correction, 1),
    ];
    assert_eq!(
        invalid_reason(project_cost_ledger(entry.run_id, &regressed)),
        "cost_ledger_projection_sequence_regression"
    );
}

/// 已提交事实被改写（approver/目标 Run/币种/聚合绑定/stream version）一律拒绝。
#[test]
fn projector_rejects_tampered_correction_payload_bindings() {
    let entry = entry();
    let correction = correction(&entry, -100, None, "bq14-query-tamper");

    let mut approver_drift = serde_json::to_value(&correction).expect("json");
    approver_drift["approver_id"] = json!("someone-else");
    let event = RuntimeEvent::new(RequestId::new(), 2, COST_CORRECTION_EVENT, approver_drift)
        .expect("event")
        .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), 2);
    assert_eq!(
        invalid_reason(project_cost_ledger(
            entry.run_id,
            &[entry_event(&entry, 1), event]
        )),
        "cost_correction_approval_digest_mismatch"
    );

    // The correction fact is digest-bound end to end, so a target drift also has to move the
    // command/approval/correction digests; otherwise the generic command digest check fires first
    // and this fixture would never reach the target-binding refusal it is meant to pin. The drift
    // is expressed on the target entry digest: moving `target_run_id` alone would instead trip the
    // fact's own run binding or the projector's envelope check before the target comparison.
    let mut target_drift = correction.clone();
    target_drift.target_entry_digest = format!("sha256:{}", "b".repeat(64));
    reseal(&mut target_drift);
    let event = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_CORRECTION_EVENT,
        serde_json::to_value(&target_drift).expect("json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), 2);
    assert_eq!(
        invalid_reason(project_cost_ledger(
            entry.run_id,
            &[entry_event(&entry, 1), event]
        )),
        "cost_correction_target_binding_mismatch"
    );

    // A currency drift is re-sealed the same way so the fact still passes the digest gate and the
    // domain currency check (not the generic command digest check) is the refusal under test.
    let mut currency_drift = correction.clone();
    let mut delta = currency_drift
        .delta_estimated
        .clone()
        .expect("estimate delta");
    delta.currency = "EUR".to_owned();
    currency_drift.delta_estimated = Some(delta);
    reseal(&mut currency_drift);
    let event = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_CORRECTION_EVENT,
        serde_json::to_value(&currency_drift).expect("json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), 2);
    assert_eq!(
        invalid_reason(project_cost_ledger(
            entry.run_id,
            &[entry_event(&entry, 1), event]
        )),
        "cost_correction_currency_mismatch"
    );

    // 事件 envelope 指向另一条聚合 / 版本为 0 时拒绝，不进入 reducer。
    let foreign_aggregate = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_CORRECTION_EVENT,
        serde_json::to_value(&correction).expect("json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_ledger",
        LedgerEntryId::new().to_string(),
        entry.revision + 1,
    );
    assert_eq!(
        invalid_reason(project_cost_ledger(
            entry.run_id,
            &[entry_event(&entry, 1), foreign_aggregate]
        )),
        "correction_identity_or_revision_mismatch"
    );

    let zero_version = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_CORRECTION_EVENT,
        serde_json::to_value(&correction).expect("json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), 0);
    assert_eq!(
        invalid_reason(project_cost_ledger(
            entry.run_id,
            &[entry_event(&entry, 1), zero_version]
        )),
        "correction_identity_or_revision_mismatch"
    );
}

/// 非账本事件被跳过但不参与 sequence 记账；空来源与 nil Run 分别拒绝。
#[test]
fn projector_skips_unrelated_events_and_rejects_empty_or_nil_sources() {
    let entry = entry();
    let correction = correction(&entry, -100, None, "bq14-query-noise");
    let unrelated = |sequence: u64| {
        RuntimeEvent::new(RequestId::new(), sequence, "run.started", json!({})).expect("event")
    };

    // `RunId::new()` is a fresh random v4, never nil; a nil id must be built explicitly.
    let nil_run = RunId::parse_str("00000000-0000-0000-0000-000000000000").expect("nil run");
    assert!(matches!(
        project_cost_ledger(nil_run, &[]),
        Err(CostLedgerProjectionError::RunInvalid)
    ));
    assert!(matches!(
        project_cost_ledger(entry.run_id, &[]),
        Err(CostLedgerProjectionError::SourceEmpty)
    ));
    assert!(matches!(
        project_cost_ledger(entry.run_id, &[unrelated(1), unrelated(2)]),
        Err(CostLedgerProjectionError::SourceEmpty)
    ));

    let mixed = vec![
        unrelated(9),
        entry_event(&entry, 1),
        unrelated(10),
        correction_event(&entry, &correction, 2),
    ];
    let projection = project_cost_ledger(entry.run_id, &mixed).expect("projection with noise");
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.source_event_ids.len(), 2);
    assert_eq!(projection.views.len(), 1);
}

/// 公开的 validate() 单独可用：头部漂移与跨 Run view 都被拒绝。
#[test]
fn projection_validate_rejects_header_and_run_drift() {
    let entry = entry();
    let correction = correction(&entry, -100, None, "bq14-query-validate");
    let projection =
        project_cost_ledger(entry.run_id, &events(&entry, &correction)).expect("projection");
    projection.validate().expect("valid projection");

    let mut foreign_run = projection.clone();
    foreign_run.run_id = RunId::new();
    assert_eq!(
        foreign_run.validate().unwrap_err(),
        CostLedgerProjectionError::Invalid("cost_ledger_projection_run_mismatch".to_owned())
    );

    let mut zero_cursor = projection;
    zero_cursor.source_cursor = 0;
    assert_eq!(
        zero_cursor.validate().unwrap_err(),
        CostLedgerProjectionError::Invalid("cost_ledger_projection_header_invalid".to_owned())
    );
    assert_eq!(
        COST_LEDGER_PROJECTION_SCHEMA,
        "kiana.cost-ledger-projection.v1"
    );
    assert_eq!(
        COST_LEDGER_PROJECTION_VERSION,
        kiana_domain::SchemaVersion::new(1, 0)
    );
}
