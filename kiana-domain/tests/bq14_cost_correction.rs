//! BQ-14 append-only 成本账本与审批绑定修正的合成夹具（先拒绝、后成功）。
//!
//! 覆盖卡片拒绝面：原地 UPDATE/DELETE、模型文本改账、无 evidence/approval、target digest 漂移；
//! 以及成功列：correction 只追加且可重放，原始/修正值与 approver 可追溯。全部事实在内存中合成，
//! 不连接 EventStore、不消费审批存储、不调用 provider，也不存在第二执行循环。

use kiana_domain::{
    validate_event_payload, validate_runtime_event, ApprovalId, AttemptId, BillingUnknownReason,
    CostBreakdown, CostCorrection, CostCorrectionAppendOutcome, CostCorrectionApproval,
    CostCorrectionCommand, CostCorrectionId, CostCorrectionLedger, CostLedgerEntry,
    CostLedgerEntryKind, EventId, LedgerEntryId, Money, NormalizedUsage, ProviderReceiptRef,
    RateCard, RateCardId, RequestId, RunId, RuntimeEvent, UsageConfidence, UsageId,
    UsageObservation, UsageSource, UsageVector, COST_CORRECTION_EVENT, COST_LEDGER_ENTRY_EVENT,
    USAGE_VECTOR_SCHEMA,
};
use serde_json::json;

/// 固定重复字符的 sha256 形状 fixture，不是真实内容摘要。
fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn money(currency: &str, micros: i128) -> Money {
    Money::new(currency, micros).expect("money")
}

/// 一条已封口的消费型原始 entry：带在有效期内、版本化的 rate card 与 100 估算。
fn target(estimate_micros: i128) -> CostLedgerEntry {
    let mut entry = CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        10,
        EventId::new(),
        1,
        1,
        digest('a'),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 7)
    .expect("rate card")
    .with_estimated_cost(money("USD", estimate_micros))
    .expect("estimate");
    entry.amount = Some(money("USD", estimate_micros));
    entry.entry_digest = entry.digest();
    entry.validate().expect("sealed entry");
    entry
}

#[allow(clippy::too_many_arguments)]
fn draft_for(
    entry: &CostLedgerEntry,
    correction_id: CostCorrectionId,
    delta_estimated: Option<Money>,
    delta_measured: Option<Money>,
    reason: &str,
    evidence_refs: Vec<String>,
    requested_by: &str,
    idempotency_key: &str,
) -> Result<CostCorrectionCommand, String> {
    CostCorrectionCommand::draft(
        RequestId::new(),
        correction_id,
        entry,
        delta_estimated,
        delta_measured,
        reason,
        evidence_refs,
        requested_by,
        idempotency_key,
        20,
    )
}

fn approve(
    draft: CostCorrectionCommand,
    approver: &str,
    approved_at_unix_ms: u64,
) -> CostCorrectionCommand {
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        draft.target_entry_digest.clone(),
        approver,
        approved_at_unix_ms,
    )
    .expect("approval shape");
    draft.with_approval(approval).expect("approved command")
}

/// 已批准的单增估算差额修正，供成功路径复用。
fn approved_correction(
    entry: &CostLedgerEntry,
    delta_micros: i128,
    key: &str,
) -> CostCorrectionCommand {
    let draft = draft_for(
        entry,
        CostCorrectionId::new(),
        Some(money("USD", delta_micros)),
        None,
        "provider invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "requester",
        key,
    )
    .expect("draft");
    approve(draft, "approver", 21)
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider:test",
        "model:test",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        7,
        "bq14-fixture",
    )
    .expect("rate card");
    card.request_price = Some(5);
    card.tool_price = Some(7);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid rate card");
    card
}

fn usage(confidence: UsageConfidence) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        (confidence != UsageConfidence::Known).then_some(BillingUnknownReason::Partial),
        "provider fixture",
        digest('f'),
    )
    .expect("usage")
}

/// 无审批、无证据、模型文本、自批准与零额修正全部在事实成形前拒绝。
#[test]
fn unapproved_evidence_free_or_model_text_commands_are_refused() {
    let entry = target(100);

    let draft = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-approval",
    )
    .expect("draft");
    assert_eq!(
        draft.validate(),
        Err("cost_correction_approval_required".to_owned())
    );

    let no_evidence = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "invoice reconciliation",
        Vec::new(),
        "operator-a",
        "bq14-idem-evidence",
    );
    assert_eq!(
        no_evidence.unwrap_err(),
        "cost_correction_evidence_required".to_owned()
    );

    let duplicate_evidence = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "invoice reconciliation",
        vec![
            "invoice:opaque-ref".to_owned(),
            "invoice:opaque-ref".to_owned(),
        ],
        "operator-a",
        "bq14-idem-duplicate-evidence",
    );
    assert_eq!(
        duplicate_evidence.unwrap_err(),
        "cost_correction_evidence_duplicate".to_owned()
    );

    // 模型文本改账：换行/空白 reason 不是可携带的自由文本。
    let model_text = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "\nraw model transcript: raise the invoice",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-model-text",
    );
    assert_eq!(
        model_text.unwrap_err(),
        "cost_correction_reason_invalid".to_owned()
    );

    let blank_reason = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "   ",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-blank",
    );
    assert_eq!(
        blank_reason.unwrap_err(),
        "cost_correction_reason_invalid".to_owned()
    );

    let no_requester = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "",
        "bq14-idem-requester",
    );
    assert_eq!(
        no_requester.unwrap_err(),
        "cost_correction_requested_by_invalid".to_owned()
    );

    let no_idempotency_key = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -20)),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "",
    );
    assert_eq!(
        no_idempotency_key.unwrap_err(),
        "cost_idempotency_key_invalid".to_owned()
    );

    let no_delta = draft_for(
        &entry,
        CostCorrectionId::new(),
        None,
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-no-delta",
    );
    assert_eq!(
        no_delta.unwrap_err(),
        "cost_correction_delta_required".to_owned()
    );

    let zero_pair = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", 0)),
        Some(money("USD", 0)),
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-zero",
    );
    assert_eq!(
        zero_pair.unwrap_err(),
        "cost_correction_zero_delta".to_owned()
    );

    let zero_single = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", 0)),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-zero-single",
    );
    assert_eq!(
        zero_single.unwrap_err(),
        "cost_correction_zero_delta".to_owned()
    );

    let mixed_currency = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", 1)),
        Some(money("EUR", 1)),
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-mixed-currency",
    );
    assert_eq!(
        mixed_currency.unwrap_err(),
        "cost_correction_currency_mismatch".to_owned()
    );
}

/// target digest/run/attempt/cursor/revision 任一漂移都在事实成形前拒绝。
#[test]
fn target_fence_drift_is_refused_before_any_fact_forms() {
    let entry = target(100);
    let command = approved_correction(&entry, -10, "bq14-idem-fence");

    let mut digest_drift = command.clone();
    digest_drift.target_entry_digest = digest('b');
    digest_drift.command_digest = digest_drift.digest();
    assert_eq!(
        digest_drift.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );

    let mut entry_id_drift = command.clone();
    entry_id_drift.target_entry_id = LedgerEntryId::new();
    entry_id_drift.command_digest = entry_id_drift.digest();
    assert_eq!(
        entry_id_drift.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );

    let mut run_drift = command.clone();
    run_drift.target_run_id = RunId::new();
    run_drift.command_digest = run_drift.digest();
    assert_eq!(
        run_drift.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );
    // 重签后 approval 主题不再匹配，不能靠“重新盖章”绕过。
    assert_eq!(
        run_drift.validate_approved().unwrap_err(),
        "cost_correction_approval_binding_mismatch".to_owned()
    );

    let mut attempt_drift = command.clone();
    attempt_drift.target_attempt_id = AttemptId::new();
    attempt_drift.command_digest = attempt_drift.digest();
    assert_eq!(
        attempt_drift.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );

    let mut cursor_drift = command.clone();
    cursor_drift.target_source_cursor += 1;
    cursor_drift.command_digest = cursor_drift.digest();
    assert_eq!(
        cursor_drift.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );

    let mut revision_drift = command.clone();
    revision_drift.target_revision += 1;
    revision_drift.command_digest = revision_drift.digest();
    assert_eq!(
        revision_drift.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );

    // 未重签的命令摘要漂移直接拒绝，不能靠改字段让审批“看起来还成立”。
    let mut tampered = command.clone();
    tampered.reason = "quietly rewritten".to_owned();
    assert_eq!(
        tampered.validate_draft_against(&entry).unwrap_err(),
        "cost_correction_command_digest_mismatch".to_owned()
    );

    // 另一条 entry（同一形状）不能复用这条命令。
    let foreign = target(100);
    assert_eq!(
        command.validate_draft_against(&foreign).unwrap_err(),
        "cost_correction_target_binding_mismatch".to_owned()
    );
}

/// approval 必须绑定同一 command 主题、同一 target digest，并由独立 approver 签发。
#[test]
fn approval_binding_requires_an_independent_approver_and_the_exact_subject() {
    let entry = target(100);
    let draft = draft_for(
        &entry,
        CostCorrectionId::new(),
        Some(money("USD", -10)),
        None,
        "invoice reconciliation",
        vec!["invoice:opaque-ref".to_owned()],
        "operator-a",
        "bq14-idem-approval-binding",
    )
    .expect("draft");

    let wrong_command_id = CostCorrectionApproval::new(
        ApprovalId::new(),
        RequestId::new(),
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "operator-b",
        21,
    )
    .expect("approval shape");
    assert_eq!(
        draft.clone().with_approval(wrong_command_id).unwrap_err(),
        "cost_correction_approval_binding_mismatch".to_owned()
    );

    let wrong_command_digest = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        digest('c'),
        entry.entry_digest.clone(),
        "operator-b",
        21,
    )
    .expect("approval shape");
    assert_eq!(
        draft
            .clone()
            .with_approval(wrong_command_digest)
            .unwrap_err(),
        "cost_correction_approval_binding_mismatch".to_owned()
    );

    let wrong_target_digest = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        digest('d'),
        "operator-b",
        21,
    )
    .expect("approval shape");
    assert_eq!(
        draft
            .clone()
            .with_approval(wrong_target_digest)
            .unwrap_err(),
        "cost_correction_approval_binding_mismatch".to_owned()
    );

    let self_approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "operator-a",
        21,
    )
    .expect("approval shape");
    assert_eq!(
        draft.clone().with_approval(self_approval).unwrap_err(),
        "cost_correction_conflicted_approver".to_owned()
    );

    let mut tampered = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "operator-b",
        21,
    )
    .expect("approval shape");
    tampered.approved_at_unix_ms += 1;
    assert_eq!(
        draft.clone().with_approval(tampered).unwrap_err(),
        "cost_correction_approval_digest_mismatch".to_owned()
    );

    let zero_time = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "operator-b",
        0,
    );
    assert_eq!(
        zero_time.unwrap_err(),
        "cost_correction_approval_header_invalid".to_owned()
    );

    let bad_digest = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        "sha256:not-hex",
        entry.entry_digest.clone(),
        "operator-b",
        21,
    );
    assert_eq!(
        bad_digest.unwrap_err(),
        "cost_correction_approval_command_digest_invalid".to_owned()
    );

    let empty_approver = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        entry.entry_digest.clone(),
        "",
        21,
    );
    assert_eq!(
        empty_approver.unwrap_err(),
        "cost_correction_approval_approver_invalid".to_owned()
    );
}

/// 账本只有 append：原 entry 逐字段不变，重复来源与未知目标拒绝。
#[test]
fn ledger_is_append_only_and_refuses_duplicate_or_unknown_sources() {
    let entry = target(100);
    let mut ledger = CostCorrectionLedger::default();
    ledger.append_entry(entry.clone()).expect("append original");
    assert_eq!(
        ledger.append_entry(entry.clone()).unwrap_err(),
        "cost_ledger_entry_duplicate".to_owned()
    );

    let mut reused_source = target(50);
    reused_source.source_event_id = entry.source_event_id;
    reused_source.entry_digest = reused_source.digest();
    reused_source.validate().expect("resealed entry");
    assert_eq!(
        ledger.append_entry(reused_source).unwrap_err(),
        "cost_ledger_source_event_duplicate".to_owned()
    );

    // 未提交过的 entry 不能作为修正目标。
    let ghost = target(100);
    let ghost_command = approved_correction(&ghost, -1, "bq14-idem-ghost");
    assert_eq!(
        ledger.append_correction(ghost_command).unwrap_err(),
        "cost_correction_target_missing".to_owned()
    );

    let correction = approved_correction(&entry, -20, "bq14-idem-append");
    let committed = ledger
        .append_correction(correction.clone())
        .expect("append correction");
    let correction = match committed {
        CostCorrectionAppendOutcome::Committed(fact) => fact,
        CostCorrectionAppendOutcome::Replayed(_) => panic!("first append replayed"),
    };
    assert_eq!(correction.target_entry_digest, entry.entry_digest);
    // 原 entry 没有 update/delete 路径：折叠后仍是同一条已提交事实。
    assert_eq!(ledger.entry(entry.entry_id), Some(&entry));
    assert_eq!(ledger.entries().count(), 1);

    // 同一 correction id 换个 command id 也不能重复入账。
    let mut duplicate_id = approved_correction(&entry, -3, "bq14-idem-duplicate-id");
    duplicate_id.correction_id = correction.correction_id;
    duplicate_id.command_digest = duplicate_id.digest();
    duplicate_id.approval = Some(
        CostCorrectionApproval::new(
            ApprovalId::new(),
            duplicate_id.command_id,
            duplicate_id.command_digest.clone(),
            entry.entry_digest.clone(),
            "approver",
            22,
        )
        .expect("approval shape"),
    );
    assert_eq!(
        ledger.append_correction(duplicate_id).unwrap_err(),
        "cost_correction_id_duplicate".to_owned()
    );
    assert_eq!(ledger.entries().count(), 1);
    assert_eq!(ledger.corrections().count(), 1);
}

/// 同一命令重放只返回原 correction；同 key 不同摘要必须冲突。
#[test]
fn replayed_commands_return_the_original_fact_and_conflicting_digests_fail() {
    let entry = target(100);
    let command = approved_correction(&entry, -20, "bq14-idem-replay");
    let mut ledger = CostCorrectionLedger::default();
    ledger.append_entry(entry.clone()).expect("append original");

    let first = match ledger
        .append_correction(command.clone())
        .expect("commit correction")
    {
        CostCorrectionAppendOutcome::Committed(fact) => fact,
        CostCorrectionAppendOutcome::Replayed(_) => panic!("first append replayed"),
    };
    let replayed = match ledger
        .append_correction(command.clone())
        .expect("replay correction")
    {
        CostCorrectionAppendOutcome::Replayed(fact) => fact,
        CostCorrectionAppendOutcome::Committed(_) => panic!("replay committed a second fact"),
    };
    assert_eq!(replayed, first);
    assert_eq!(ledger.corrections().count(), 1);

    // 同一 command id 换摘要（并重新绑定审批）仍是重放冲突，不能改写原事实。
    let mut conflicting = command.clone();
    conflicting.delta_estimated = Some(money("USD", -30));
    conflicting.command_digest = conflicting.digest();
    conflicting.approval = Some(
        CostCorrectionApproval::new(
            ApprovalId::new(),
            conflicting.command_id,
            conflicting.command_digest.clone(),
            entry.entry_digest.clone(),
            "approver",
            22,
        )
        .expect("approval shape"),
    );
    assert_eq!(
        ledger.append_correction(conflicting).unwrap_err(),
        "cost_correction_command_replay_conflict".to_owned()
    );

    // 同一 idempotency key 换 command 也是冲突。
    let mut reused_key = command.clone();
    reused_key.command_id = RequestId::new();
    reused_key.correction_id = CostCorrectionId::new();
    reused_key.delta_estimated = Some(money("USD", -40));
    reused_key.command_digest = reused_key.digest();
    reused_key.approval = Some(
        CostCorrectionApproval::new(
            ApprovalId::new(),
            reused_key.command_id,
            reused_key.command_digest.clone(),
            entry.entry_digest.clone(),
            "approver",
            23,
        )
        .expect("approval shape"),
    );
    assert_eq!(
        ledger.append_correction(reused_key).unwrap_err(),
        "cost_correction_idempotency_conflict".to_owned()
    );

    assert_eq!(ledger.correction(first.correction_id), Some(&first));
    assert_eq!(ledger.corrections().count(), 1);
    assert_eq!(ledger.entry(entry.entry_id), Some(&entry));
}

/// 原始值与每次修正、approver、证据都能逐条追溯；币种漂移在事实成形前拒绝。
#[test]
fn original_corrected_values_and_approver_stay_traceable() {
    let entry = target(100);
    let mut ledger = CostCorrectionLedger::default();
    ledger.append_entry(entry.clone()).expect("append original");

    let first = approved_correction(&entry, -20, "bq14-idem-trace-1");
    let second = approve(
        draft_for(
            &entry,
            CostCorrectionId::new(),
            Some(money("USD", -5)),
            Some(money("USD", 7)),
            "invoice reconciliation follow-up",
            vec!["invoice:opaque-ref-2".to_owned()],
            "requester",
            "bq14-idem-trace-2",
        )
        .expect("draft"),
        "approver-c",
        22,
    );
    ledger.append_correction(first).expect("first correction");
    ledger.append_correction(second).expect("second correction");

    let view = ledger.view(entry.entry_id).expect("view");
    assert_eq!(view.original.entry_digest, entry.entry_digest);
    assert_eq!(view.original.estimated_cost, Some(money("USD", 100)));
    assert_eq!(view.original.amount, Some(money("USD", 100)));
    assert_eq!(view.estimated_cost, Some(money("USD", 75)));
    assert_eq!(view.measured_cost, Some(money("USD", 7)));
    assert_eq!(view.corrections.len(), 2);
    for correction in &view.corrections {
        assert_eq!(correction.target_entry_digest, entry.entry_digest);
        assert_eq!(correction.target_revision, entry.revision);
        assert_eq!(correction.requested_by, "requester");
        assert!(!correction.evidence_refs.is_empty());
        assert_ne!(correction.approver_id, correction.requested_by);
    }
    let mut approvers: Vec<&str> = view
        .corrections
        .iter()
        .map(|correction| correction.approver_id.as_str())
        .collect();
    approvers.sort_unstable();
    assert_eq!(approvers, vec!["approver", "approver-c"]);
    // 第二次修正同时携带 estimated 与 measured 差额，两者在 view 中分列。
    assert!(view
        .corrections
        .iter()
        .any(|correction| correction.delta_measured == Some(money("USD", 7))));

    // 已批准但币种漂移的修正不得进入账本，也不能改变已有 view。
    let drifted = approve(
        draft_for(
            &entry,
            CostCorrectionId::new(),
            Some(money("EUR", -20)),
            None,
            "foreign currency correction",
            vec!["invoice:opaque-ref-3".to_owned()],
            "requester",
            "bq14-idem-trace-3",
        )
        .expect("draft"),
        "approver",
        23,
    );
    assert_eq!(
        CostCorrection::from_command(&drifted, &entry).unwrap_err(),
        "cost_correction_currency_mismatch".to_owned()
    );
    assert_eq!(
        ledger.append_correction(drifted).unwrap_err(),
        "cost_correction_currency_mismatch".to_owned()
    );
    let unchanged = ledger.view(entry.entry_id).expect("view");
    assert_eq!(unchanged, view);
    assert_eq!(ledger.corrections().count(), 2);
}

/// entry 自身的不变量：来源完整性、measured 必须有 receipt、修正必须携带金额。
#[test]
fn ledger_entry_invariants_reject_forged_provenance() {
    let mut no_rate_card = target(100);
    no_rate_card.rate_card_id = None;
    no_rate_card.rate_card_version = None;
    no_rate_card.entry_digest = no_rate_card.digest();
    assert_eq!(
        no_rate_card.validate().unwrap_err(),
        "cost_ledger_entry_rate_card_required".to_owned()
    );

    let mut zero_version = target(100);
    zero_version.rate_card_version = Some(0);
    zero_version.entry_digest = zero_version.digest();
    assert_eq!(
        zero_version.validate().unwrap_err(),
        "cost_ledger_entry_rate_card_invalid".to_owned()
    );

    let mut measured_without_receipt = target(100);
    measured_without_receipt.amount = None;
    measured_without_receipt.estimated_cost = None;
    measured_without_receipt.measured_cost = Some(money("USD", 100));
    measured_without_receipt.entry_digest = measured_without_receipt.digest();
    assert_eq!(
        measured_without_receipt.validate().unwrap_err(),
        "cost_ledger_entry_provider_receipt_required".to_owned()
    );

    let mut receipt_without_measured = target(100);
    receipt_without_measured.amount = None;
    receipt_without_measured.estimated_cost = None;
    receipt_without_measured.provider_receipt_ref =
        Some(ProviderReceiptRef::new("provider-receipt-forged".to_owned()).expect("receipt"));
    receipt_without_measured.entry_digest = receipt_without_measured.digest();
    assert_eq!(
        receipt_without_measured.validate().unwrap_err(),
        "cost_ledger_entry_provider_receipt_without_measured".to_owned()
    );

    let mut currency_drift = target(100);
    currency_drift.amount = Some(money("EUR", 100));
    currency_drift.entry_digest = currency_drift.digest();
    assert_eq!(
        currency_drift.validate().unwrap_err(),
        "cost_ledger_entry_currency_mismatch".to_owned()
    );

    let mut empty_correction = target(100);
    empty_correction.kind = CostLedgerEntryKind::Correction;
    empty_correction.amount = None;
    empty_correction.estimated_cost = None;
    empty_correction.entry_digest = empty_correction.digest();
    assert_eq!(
        empty_correction.validate().unwrap_err(),
        "cost_ledger_entry_correction_amount_required".to_owned()
    );

    let mut no_attempt = target(100);
    no_attempt.attempt_id = None;
    no_attempt.entry_digest = no_attempt.digest();
    assert_eq!(
        no_attempt.validate().unwrap_err(),
        "cost_ledger_entry_attempt_required".to_owned()
    );

    let mut zero_cursor = target(100);
    zero_cursor.source_cursor = 0;
    zero_cursor.entry_digest = zero_cursor.digest();
    assert_eq!(
        zero_cursor.validate().unwrap_err(),
        "cost_ledger_entry_header_invalid".to_owned()
    );

    let mut bad_source_digest = target(100);
    bad_source_digest.source_digest = "sha256:not-hex".to_owned();
    bad_source_digest.entry_digest = bad_source_digest.digest();
    assert_eq!(
        bad_source_digest.validate().unwrap_err(),
        "cost_ledger_entry_source_digest_invalid".to_owned()
    );

    let mut tampered = target(100);
    tampered.created_at_unix_ms += 1;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "cost_ledger_entry_digest_mismatch".to_owned()
    );
}

/// BQ-13 三类观测进入账本后仍保留 estimate/measured/unknown 来源，不放宽为伪零额。
#[test]
fn ledger_entries_keep_breakdown_provenance_without_transcript_authority() {
    let usage = usage(UsageConfidence::Known);
    let card = card();
    let attempt_id = usage.attempt_id;
    let run_id = usage.run_id;
    let receipt = ProviderReceiptRef::new("provider-receipt-bq14".to_owned()).expect("receipt");

    let estimate = CostBreakdown::from_usage(&usage, Some(&card), 1, 2_000).expect("estimate");
    let estimate_amount = estimate.kind.amount().cloned().expect("estimate amount");
    let estimated_entry = CostLedgerEntry::from_cost_breakdown(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        &estimate,
        10,
        EventId::new(),
        1,
        1,
    )
    .expect("estimated ledger entry");
    assert_eq!(estimated_entry.run_id, run_id);
    assert_eq!(estimated_entry.attempt_id, Some(attempt_id));
    assert_eq!(
        estimated_entry.estimated_cost,
        Some(estimate_amount.clone())
    );
    assert_eq!(estimated_entry.amount, Some(estimate_amount));
    assert!(estimated_entry.measured_cost.is_none());
    assert!(estimated_entry.provider_receipt_ref.is_none());
    assert_eq!(estimated_entry.rate_card_id, Some(card.rate_card_id));
    assert_eq!(estimated_entry.rate_card_version, Some(card.card_version));
    assert_eq!(estimated_entry.source_digest, estimate.breakdown_digest);

    let measured = CostBreakdown::from_provider_receipt(
        &usage,
        Some(&card),
        money("USD", 29),
        receipt.clone(),
        2_000,
    )
    .expect("measured");
    let measured_entry = CostLedgerEntry::from_cost_breakdown(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        &measured,
        10,
        EventId::new(),
        2,
        1,
    )
    .expect("measured ledger entry");
    assert_eq!(measured_entry.measured_cost, Some(money("USD", 29)));
    assert_eq!(measured_entry.amount, Some(money("USD", 29)));
    assert!(measured_entry.estimated_cost.is_none());
    assert_eq!(measured_entry.provider_receipt_ref, Some(receipt));

    let unknown = CostBreakdown::unknown(
        run_id,
        Some(attempt_id),
        digest('e'),
        UsageConfidence::Unknown,
        BillingUnknownReason::ResultUnknown,
    )
    .expect("unknown");
    let unknown_entry = CostLedgerEntry::from_cost_breakdown(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        &unknown,
        10,
        EventId::new(),
        3,
        1,
    )
    .expect("unknown ledger entry");
    assert!(unknown_entry.amount.is_none());
    assert!(unknown_entry.estimated_cost.is_none());
    assert!(unknown_entry.measured_cost.is_none());
    assert!(unknown_entry.provider_receipt_ref.is_none());
    assert_eq!(unknown_entry.currency, "UNK");

    let encoded = serde_json::to_string(&unknown_entry).expect("encode unknown entry");
    assert!(!encoded.contains("transcript"));
    assert!(!encoded.contains("prompt"));
}

/// 账本事实过已注册事件契约；缺失身份键或夹带模型文本字段一律拒绝。
#[test]
fn ledger_facts_are_registered_cost_ledger_events() {
    let entry = target(100);
    let correction = CostCorrection::from_command(
        &approved_correction(&entry, -20, "bq14-idem-events"),
        &entry,
    )
    .expect("correction");

    let entry_event = RuntimeEvent::new(
        RequestId::new(),
        1,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(&entry).expect("entry json"),
    )
    .expect("entry event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), entry.revision);
    validate_runtime_event(&entry_event).expect("registered ledger entry event");
    validate_event_payload(COST_LEDGER_ENTRY_EVENT, &entry_event.data).expect("ledger payload");

    let correction_event = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_CORRECTION_EVENT,
        serde_json::to_value(&correction).expect("correction json"),
    )
    .expect("correction event")
    .with_stream_metadata(
        "cost_ledger",
        entry.entry_id.to_string(),
        entry.revision + 1,
    );
    validate_runtime_event(&correction_event).expect("registered correction event");
    validate_event_payload(COST_CORRECTION_EVENT, &correction_event.data)
        .expect("correction payload");

    let mut missing_entry_id = entry_event.clone();
    missing_entry_id
        .data
        .as_object_mut()
        .expect("object")
        .remove("entry_id");
    assert_eq!(
        validate_event_payload(COST_LEDGER_ENTRY_EVENT, &missing_entry_id.data).unwrap_err(),
        "event_required_id_missing:entry_id".to_owned()
    );

    let mut extra_field = correction_event;
    extra_field.data["transcript"] = json!("model said raise the invoice");
    assert_eq!(
        validate_event_payload(COST_CORRECTION_EVENT, &extra_field.data).unwrap_err(),
        "event_payload_unknown_field".to_owned()
    );
}

/// 模型文本既不能解码进命令/账本事实，也不能通过重新序列化混入 wire 形状。
#[test]
fn model_transcript_cannot_be_decoded_into_command_or_ledger_facts() {
    let entry = target(100);
    let command = approved_correction(&entry, -20, "bq14-idem-transcript");
    let encoded = serde_json::to_value(&command).expect("command json");
    assert!(encoded.get("transcript").is_none());
    assert!(encoded.get("model_text").is_none());
    let decoded: CostCorrectionCommand = serde_json::from_value(encoded).expect("round trip");
    assert_eq!(decoded, command);

    let mut with_transcript = serde_json::to_value(&command).expect("command json");
    with_transcript["transcript"] = json!("model said raise the invoice");
    assert!(
        serde_json::from_value::<CostCorrectionCommand>(with_transcript).is_err(),
        "model transcript must not decode into a correction command"
    );

    let mut entry_json = serde_json::to_value(&entry).expect("entry json");
    entry_json["model_text"] = json!("model said raise the invoice");
    assert!(
        serde_json::from_value::<CostLedgerEntry>(entry_json).is_err(),
        "model text must not decode into a ledger entry"
    );
}
