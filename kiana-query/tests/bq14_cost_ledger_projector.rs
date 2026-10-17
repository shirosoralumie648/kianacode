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

    let mut run_drift = serde_json::to_value(&correction).expect("json");
    run_drift["target_run_id"] = json!(RunId::new().to_string());
    let event = RuntimeEvent::new(RequestId::new(), 2, COST_CORRECTION_EVENT, run_drift)
        .expect("event")
        .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), 2);
    assert_eq!(
        invalid_reason(project_cost_ledger(
            entry.run_id,
            &[entry_event(&entry, 1), event]
        )),
        "cost_correction_target_binding_mismatch"
    );

    let mut currency_drift = serde_json::to_value(&correction).expect("json");
    currency_drift["delta_estimated"] = json!({
        "schema": currency_drift["delta_estimated"]["schema"].clone(),
        "currency": "EUR",
        "micros": -100,
    });
    let event = RuntimeEvent::new(RequestId::new(), 2, COST_CORRECTION_EVENT, currency_drift)
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

    assert!(matches!(
        project_cost_ledger(RunId::new(), &[]),
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
