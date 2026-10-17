//! BQ-20 EventLog billing ledger projector fixtures (deny first).
//!
//! Every fixture builds already-committed `RuntimeEvent` facts in memory and replays them through
//! `BillingLedgerProjector`. Nothing here appends to an EventLog, consumes a reservation or
//! contacts a provider: the projector is a rebuildable read model, and the denials below prove
//! that a failed page never advances the acknowledged cursor.

use kiana_domain::*;
use kiana_query::{project_billing_ledger, BillingLedgerProjectionError, BillingLedgerProjector};
use serde_json::json;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn raw_event(kind: &str) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), 1, kind, json!({})).expect("event")
}

fn usage(attempt_id: AttemptId, input_tokens: u64) -> NormalizedUsage {
    NormalizedUsage::new(
        UsageId::new(),
        attempt_id,
        RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(input_tokens),
            output_tokens: Some(2),
            ..UsageVector::zero()
        },
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        UsageConfidence::Known,
        None,
        "bq20 fixture",
        digest('u'),
    )
    .expect("usage")
}

fn card(currency: &str) -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider:test",
        "model:test",
        currency,
        Some(1),
        Some(2),
        1_000,
        Some(86_400_002),
        1,
        "bq20-fixture",
    )
    .expect("card");
    card.request_price = Some(1);
    card.rate_card_digest = card.digest();
    card
}

fn estimate(usage: &NormalizedUsage, currency: &str) -> CostBreakdown {
    CostBreakdown::from_usage(usage, Some(&card(currency)), 1, 86_400_001).expect("estimate")
}

fn unknown_event(usage: &NormalizedUsage, sequence: u64) -> RuntimeEvent {
    CostBreakdown::unknown(
        usage.run_id,
        Some(usage.attempt_id),
        usage.usage_digest.clone(),
        usage.confidence,
        BillingUnknownReason::Partial,
    )
    .expect("unknown")
    .to_runtime_event(RequestId::new(), sequence)
    .expect("event")
}

fn estimated_event(usage: &NormalizedUsage, sequence: u64) -> RuntimeEvent {
    estimate(usage, "USD")
        .to_runtime_event(RequestId::new(), sequence)
        .expect("event")
}

fn measured_event(usage: &NormalizedUsage, micros: i128, sequence: u64) -> RuntimeEvent {
    CostBreakdown::from_provider_receipt(
        usage,
        Some(&card("USD")),
        Money::new("USD", micros).expect("money"),
        ProviderReceiptRef::new("provider-receipt:bq20").expect("receipt"),
        86_400_001,
    )
    .expect("measured")
    .to_runtime_event(RequestId::new(), sequence)
    .expect("event")
}

fn ledger_entry(micros: i128) -> CostLedgerEntry {
    CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        None,
        "USD",
        86_400_001,
        EventId::new(),
        1,
        1,
        digest('l'),
    )
    .expect("entry")
    .with_rate_card(RateCardId::new(), 1)
    .expect("rate card")
    .with_estimated_cost(Money::new("USD", micros).expect("money"))
    .expect("estimate")
}

fn ledger_event(entry: &CostLedgerEntry, sequence: u64) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(entry).expect("entry json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), entry.revision)
}

fn correction_fact(target: &CostLedgerEntry, delta: i128, tag: &str) -> CostCorrection {
    let draft = CostCorrectionCommand::draft(
        RequestId::new(),
        CostCorrectionId::new(),
        target,
        Some(Money::new("USD", delta).expect("money")),
        None,
        "bq20 correction",
        vec![format!("evidence:bq20:{tag}")],
        "requester:bq20",
        format!("idempotency:bq20:{tag}"),
        86_400_003,
    )
    .expect("draft");
    let approval = CostCorrectionApproval::new(
        ApprovalId::new(),
        draft.command_id,
        draft.command_digest.clone(),
        draft.target_entry_digest.clone(),
        "approver:bq20",
        86_400_004,
    )
    .expect("approval");
    CostCorrection::from_command(&draft.with_approval(approval).expect("approved"), target)
        .expect("correction")
}

fn correction_event(
    target: &CostLedgerEntry,
    correction: &CostCorrection,
    stream_version: u64,
    sequence: u64,
) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_CORRECTION_EVENT,
        serde_json::to_value(correction).expect("correction json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", target.entry_id.to_string(), stream_version)
}

fn allocation_event(usage_id: UsageId, run_id: RunId, micros: i128, sequence: u64) -> RuntimeEvent {
    let scope = AllocationScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        WorkflowInstanceId::new(),
        CellId::new(),
        run_id,
    )
    .expect("scope");
    let allocation = CostAllocation::new(
        CostAllocationId::new(),
        usage_id,
        scope.project_id,
        scope,
        None,
        AllocationCostKind::Estimated {
            amount: Money::new("USD", micros).expect("money"),
            rate_card_id: RateCardId::new(),
            rate_card_version: 1,
        },
        EventId::new(),
        1,
        1,
        digest('a'),
    )
    .expect("allocation");
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        COST_ALLOCATION_EVENT,
        serde_json::to_value(&allocation).expect("allocation json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_allocation",
        allocation.allocation_id.to_string(),
        allocation.revision,
    )
}

fn page(events: Vec<RuntimeEvent>, cursor: EventCursor) -> JournalPage {
    JournalPage {
        events,
        cursor,
        has_more: false,
    }
}

#[test]
fn snapshot_rebuild_failure_does_not_advance_cursor() {
    let usd = estimate(&usage(AttemptId::new(), 4), "USD")
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let eur = estimate(&usage(AttemptId::new(), 4), "EUR")
        .to_runtime_event(RequestId::new(), 1)
        .expect("event");
    let mixed = page(vec![usd, eur], 2);

    let mut projector = BillingLedgerProjector::new(4).expect("projector");
    assert_eq!(
        projector.apply_page(&mixed, 4).unwrap_err(),
        BillingLedgerProjectionError::Invalid("money_currency_mismatch".to_owned())
    );
    // 候选状态从未发布：cursor、隔离和摘要都保持失败前状态。
    assert_eq!(projector.source_cursor(), 0);
    assert!(projector.quarantine().is_empty());
    assert_eq!(
        projector.apply_page(&mixed, 4).unwrap_err().to_string(),
        "billing_projection_invalid:money_currency_mismatch"
    );
    assert_eq!(projector.source_cursor(), 0);
}

#[test]
fn cursor_gap_and_empty_page_regression_are_refused() {
    let mut projector = BillingLedgerProjector::new(4).expect("projector");
    let usage = usage(AttemptId::new(), 4);

    // 空页只能停在当前 cursor，不能推进。
    assert_eq!(
        projector.apply_page(&page(Vec::new(), 1), 4).unwrap_err(),
        BillingLedgerProjectionError::SourceCursorInvalid
    );
    // 一页 1 个事件却声称读到 cursor 2：缺口，整页拒绝。
    assert_eq!(
        projector
            .apply_page(&page(vec![estimated_event(&usage, 1)], 2), 4)
            .unwrap_err(),
        BillingLedgerProjectionError::SourceCursorGap
    );
    assert_eq!(
        projector
            .apply_page(&page(vec![estimated_event(&usage, 1)], 1), 5)
            .unwrap_err(),
        BillingLedgerProjectionError::SourceEpochInvalid
    );
    assert_eq!(
        BillingLedgerProjector::new(0).unwrap_err(),
        BillingLedgerProjectionError::SourceEpochInvalid
    );
    assert_eq!(projector.source_cursor(), 0);

    // 正常页发布后：空页同 cursor 是 no-op，回退 cursor 是 replay。
    projector
        .apply_page(&page(vec![estimated_event(&usage, 1)], 1), 4)
        .expect("page");
    projector
        .apply_page(&page(Vec::new(), 1), 4)
        .expect("no-op empty page");
    assert_eq!(projector.source_cursor(), 1);
    assert_eq!(
        projector.apply_page(&page(Vec::new(), 0), 4).unwrap_err(),
        BillingLedgerProjectionError::SourceReplay
    );
}

#[test]
fn failed_page_does_not_advance_cursor_and_replay_is_rejected() {
    let mut projector = BillingLedgerProjector::new(4).expect("projector");
    let event = estimated_event(&usage(AttemptId::new(), 4), 1);
    let first = page(vec![event.clone()], 1);
    projector.apply_page(&first, 4).expect("page");
    let digest_after_first = projector.snapshot().projection_digest.clone();

    // 同一事件 ID 在新 cursor 上再次出现：整页拒绝且不能发布。
    let duplicate = page(vec![event], 2);
    assert_eq!(
        projector.apply_page(&duplicate, 4).unwrap_err(),
        BillingLedgerProjectionError::SourceEventDuplicate
    );
    assert_eq!(projector.source_cursor(), 1);
    assert_eq!(projector.snapshot().projection_digest, digest_after_first);
    assert!(projector.quarantine().is_empty());
}

#[test]
fn quarantine_keeps_bad_facts_queryable_by_cursor_event_and_payload_digest() {
    let malformed = RuntimeEvent::new(
        RequestId::new(),
        1,
        COST_EVENT_ESTIMATED,
        json!({"run_id": RunId::new()}),
    )
    .expect("event");
    let usage = usage(AttemptId::new(), 4);
    let stream_drift = estimated_event(&usage, 1).with_stream_metadata(
        "billing_attempt",
        AttemptId::new().to_string(),
        1,
    );
    let measured = measured_event(&usage, 7, 1);
    let kind_drift = RuntimeEvent::new(
        RequestId::new(),
        3,
        COST_EVENT_ESTIMATED,
        measured.data.clone(),
    )
    .expect("event")
    .with_stream_metadata(
        "billing_attempt",
        usage.attempt_id.to_string(),
        usage.sequence.unwrap_or(1),
    );
    let reservation = RuntimeEvent::new(
        RequestId::new(),
        4,
        "usage.reserved",
        json!({"run_id": RunId::new()}),
    )
    .expect("event");

    let mut projector = BillingLedgerProjector::new(4).expect("projector");
    projector
        .apply_page(
            &page(
                vec![
                    malformed.clone(),
                    stream_drift.clone(),
                    kind_drift.clone(),
                    reservation,
                ],
                4,
            ),
            4,
        )
        .expect("page");

    // 坏事实不阻断来源位置，但也不能被静默跳过：cursor 前进、quarantine 可查询。
    assert_eq!(projector.source_cursor(), 4);
    assert_eq!(projector.quarantine().len(), 3);
    let snapshot = projector.snapshot();
    assert_eq!(
        snapshot
            .quarantine_for_cursor(1)
            .first()
            .map(|record| record.reason.as_str()),
        Some("usage_cost_decode_failed")
    );
    assert_eq!(
        snapshot
            .quarantine_for_event(stream_drift.event_id)
            .map(|record| record.reason.as_str()),
        Some("usage_cost_stream_mismatch")
    );
    assert_eq!(
        snapshot
            .quarantine_for_event(kind_drift.event_id)
            .map(|record| record.reason.as_str()),
        Some("usage_cost_kind_mismatch")
    );
    assert!(snapshot.quarantine_for_event(EventId::new()).is_none());

    let payload_digest = snapshot
        .quarantine_for_event(malformed.event_id)
        .expect("malformed quarantine")
        .payload_digest
        .clone();
    assert_eq!(
        snapshot.quarantine_by_payload_digest(&payload_digest).len(),
        1
    );
    assert!(snapshot
        .quarantine_by_payload_digest(&digest('z'))
        .is_empty());

    // 读模型只保留身份/原因/载荷摘要，不复制原始载荷；坏事实也不产生任何金额。
    let json = serde_json::to_value(snapshot).expect("snapshot json");
    let quarantined = &json["quarantine"][0];
    assert!(quarantined.get("payload_digest").is_some());
    assert!(quarantined.get("data").is_none());
    assert!(quarantined.get("raw_payload").is_none());
    assert_eq!(snapshot.usage.usage_count, 0);
    assert_eq!(snapshot.ledger.ledger_entry_count, 0);
}

#[test]
fn quarantine_limit_fails_the_page_without_publishing() {
    let events: Vec<RuntimeEvent> = (0..=MAX_BILLING_QUARANTINE)
        .map(|index| {
            RuntimeEvent::new(
                RequestId::new(),
                (index as u64) + 1,
                COST_EVENT_ESTIMATED,
                json!({}),
            )
            .expect("event")
        })
        .collect();
    let overflow = page(events, (MAX_BILLING_QUARANTINE as u64) + 1);
    let mut projector = BillingLedgerProjector::new(4).expect("projector");
    assert_eq!(
        projector.apply_page(&overflow, 4).unwrap_err(),
        BillingLedgerProjectionError::Invalid("billing_quarantine_limit".to_owned())
    );
    assert_eq!(projector.source_cursor(), 0);
    assert!(projector.quarantine().is_empty());
}

#[test]
fn projector_rejects_nil_and_empty_sources() {
    assert_eq!(
        project_billing_ledger(0, 0, &[]).unwrap_err(),
        BillingLedgerProjectionError::SourceEpochInvalid
    );
    assert_eq!(
        project_billing_ledger(3, 2, &[]).unwrap_err(),
        BillingLedgerProjectionError::SourceCursorInvalid
    );
    project_billing_ledger(3, 0, &[]).expect("empty rebuild");

    let mut nil_event = raw_event("run.started");
    nil_event.event_id =
        EventId::parse_str("00000000-0000-0000-0000-000000000000").expect("nil id");
    assert_eq!(
        BillingSourceEvent::new(0, nil_event.clone()).unwrap_err(),
        "billing_source_event_invalid"
    );
    assert_eq!(
        BillingSourceEvent::new(1, nil_event).unwrap_err(),
        "billing_source_event_invalid"
    );
    BillingSourceEvent::new(1, raw_event("run.started")).expect("valid source event");

    // 拒绝码与快照重建要求必须保持稳定字符串。
    for (error, code) in [
        (
            BillingLedgerProjectionError::SourceEpochInvalid,
            "billing_projection_source_epoch_invalid",
        ),
        (
            BillingLedgerProjectionError::SourceCursorInvalid,
            "billing_projection_source_cursor_invalid",
        ),
        (
            BillingLedgerProjectionError::SourceCursorGap,
            "billing_projection_source_cursor_gap",
        ),
        (
            BillingLedgerProjectionError::SourceReplay,
            "billing_projection_source_replay",
        ),
        (
            BillingLedgerProjectionError::SourceEventDuplicate,
            "billing_projection_source_event_duplicate",
        ),
    ] {
        assert_eq!(error.to_string(), code);
    }
    let snapshot = project_billing_ledger(3, 0, &[]).expect("snapshot");
    assert_eq!(
        BillingLedgerProjector::from_snapshot(snapshot).unwrap_err(),
        BillingLedgerProjectionError::Invalid(
            "billing_projection_snapshot_requires_rebuild".to_owned()
        )
    );
}

#[test]
fn correction_stream_versions_must_follow_the_ledger_stream() {
    let entry = ledger_entry(11);
    let ledger = ledger_event(&entry, 1);

    // 正式账本 entry 自身的聚合/版本绑定也必须精确：错 stream version 与重复 entry 都拒绝。
    let stream_mismatch = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(&entry).expect("entry json"),
    )
    .expect("event")
    .with_stream_metadata(
        "cost_ledger",
        entry.entry_id.to_string(),
        entry.revision + 1,
    );
    let mismatched =
        project_billing_ledger(7, 2, &[ledger.clone(), stream_mismatch]).expect("projection");
    assert_eq!(
        mismatched.quarantine[0].reason,
        "ledger_entry_stream_mismatch"
    );
    let duplicate_entry = RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(&entry).expect("entry json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), entry.revision);
    let duplicated_entry =
        project_billing_ledger(7, 2, &[ledger.clone(), duplicate_entry]).expect("projection");
    assert_eq!(
        duplicated_entry.quarantine[0].reason,
        "ledger_entry_duplicate"
    );
    assert_eq!(duplicated_entry.ledger.ledger_entry_count, 1);

    // 合法链：entry revision 1 → correction stream 2。
    let first = correction_fact(&entry, -5, "first");
    let valid = project_billing_ledger(
        7,
        2,
        &[ledger.clone(), correction_event(&entry, &first, 2, 2)],
    )
    .expect("projection");
    assert_eq!(valid.ledger.estimated.expect("estimated").micros, 11);
    assert_eq!(
        valid
            .ledger
            .correction_estimated
            .expect("correction")
            .micros,
        -5
    );
    assert!(valid.quarantine.is_empty());

    // stream 0 与错误聚合：stream mismatch。
    let zero = project_billing_ledger(
        7,
        2,
        &[ledger.clone(), correction_event(&entry, &first, 0, 2)],
    )
    .expect("projection");
    assert_eq!(zero.quarantine[0].reason, "correction_stream_mismatch");

    // stream 1 不大于 entry revision 1：回退。
    let stale = correction_fact(&entry, -5, "stale");
    let regression = project_billing_ledger(
        7,
        2,
        &[ledger.clone(), correction_event(&entry, &stale, 1, 2)],
    )
    .expect("projection");
    assert_eq!(
        regression.quarantine[0].reason,
        "correction_stream_regression"
    );
    assert!(regression.ledger.correction_estimated.is_none());

    // stream 4 跳过 2/3：缺口，不允许静默并入。
    let gapped = correction_fact(&entry, -5, "gapped");
    let gap = project_billing_ledger(
        7,
        2,
        &[ledger.clone(), correction_event(&entry, &gapped, 4, 2)],
    )
    .expect("projection");
    assert_eq!(gap.quarantine[0].reason, "correction_stream_gap");

    // 同一修正 ID 再来一次：重复。
    let duplicate = RuntimeEvent::new(
        RequestId::new(),
        3,
        COST_CORRECTION_EVENT,
        serde_json::to_value(&first).expect("json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), 2);
    let duplicated = project_billing_ledger(
        7,
        3,
        &[
            ledger.clone(),
            correction_event(&entry, &first, 2, 2),
            duplicate,
        ],
    )
    .expect("projection");
    assert_eq!(duplicated.quarantine[0].reason, "correction_duplicate");

    // 目标绑定漂移（重签摘要后仍与目标不符）。
    let mut drifted = first.clone();
    drifted.target_source_cursor += 1;
    drifted.correction_digest = drifted.digest();
    let binding = project_billing_ledger(
        7,
        2,
        &[ledger.clone(), correction_event(&entry, &drifted, 2, 2)],
    )
    .expect("projection");
    assert_eq!(binding.quarantine[0].reason, "correction_target_mismatch");

    // 修正先于目标出现：目标缺失。
    let before_target = project_billing_ledger(
        7,
        2,
        &[correction_event(&entry, &first, 2, 1), ledger.clone()],
    )
    .expect("projection");
    assert_eq!(
        before_target.quarantine[0].reason,
        "correction_target_missing"
    );

    // 两条合法修正按 2/3 递增时才都进入修正栏。
    let second = correction_fact(&entry, -2, "second");
    let chained = project_billing_ledger(
        7,
        3,
        &[
            ledger,
            correction_event(&entry, &first, 2, 2),
            correction_event(&entry, &second, 3, 3),
        ],
    )
    .expect("projection");
    assert_eq!(
        chained
            .ledger
            .correction_estimated
            .expect("correction")
            .micros,
        -7
    );
    assert!(chained.quarantine.is_empty());
}

#[test]
fn usage_revisions_fold_to_the_latest_and_timestamp_less_facts_stay_unbucketed() {
    let shared = usage(AttemptId::new(), 4);
    let events = vec![
        unknown_event(&shared, 1),
        estimated_event(&shared, 2),
        measured_event(&shared, 7, 3),
        estimated_event(&usage(AttemptId::new(), 8), 1),
    ];
    let snapshot = project_billing_ledger(7, 4, &events).expect("projection");
    // 同一次 attempt 的三个版本只保留最新 measured 版本；另一 attempt 是估算。
    assert_eq!(snapshot.usage.usage_count, 2);
    assert_eq!(snapshot.usage.measured.expect("measured").micros, 7);
    assert_eq!(snapshot.usage.estimated.expect("estimated").micros, 13);
    assert_eq!(snapshot.usage.unknown_count, 0);
    // usage 事实不带可信时间戳：不得伪造日/窗口桶。
    assert!(snapshot.daily_rollups.is_empty());
    assert!(snapshot.window_rollups.is_empty());

    // revision 缺口与非法状态迁移都进入 quarantine。
    let gap = project_billing_ledger(
        7,
        2,
        &[unknown_event(&shared, 1), measured_event(&shared, 7, 3)],
    )
    .expect("projection");
    assert_eq!(gap.quarantine[0].reason, "usage_cost_revision_gap");
    assert_eq!(gap.usage.usage_count, 1);
    let transition = project_billing_ledger(
        7,
        2,
        &[measured_event(&shared, 7, 1), estimated_event(&shared, 2)],
    )
    .expect("projection");
    assert_eq!(
        transition.quarantine[0].reason,
        "usage_cost_transition_invalid"
    );
    assert_eq!(transition.usage.measured.expect("measured").micros, 7);

    // 缺少 attempt 的成本事实：独立于摘要一致性被拒绝。
    let missing_attempt = CostBreakdown::unknown(
        shared.run_id,
        None,
        shared.usage_digest.clone(),
        UsageConfidence::Known,
        BillingUnknownReason::Absent,
    )
    .expect("unknown");
    let missing_event = RuntimeEvent::new(
        RequestId::new(),
        1,
        COST_EVENT_UNKNOWN,
        serde_json::to_value(&missing_attempt).expect("json"),
    )
    .expect("event");
    let missing = project_billing_ledger(7, 1, &[missing_event]).expect("projection");
    assert_eq!(missing.quarantine[0].reason, "usage_cost_attempt_missing");
}

#[test]
fn allocation_views_do_not_multiply_ledger_totals() {
    let entry = ledger_entry(11);
    let ledger = ledger_event(&entry, 1);
    let usage_id = UsageId::new();
    let snapshot = project_billing_ledger(
        7,
        2,
        &[
            ledger.clone(),
            allocation_event(usage_id, entry.run_id, 17, 2),
        ],
    )
    .expect("projection");
    // 归因维度是平行视图：既不进账本总额，也不重复进入日/窗口桶。
    assert_eq!(snapshot.ledger.ledger_entry_count, 1);
    assert_eq!(snapshot.ledger.estimated.expect("ledger").micros, 11);
    assert_eq!(snapshot.allocation.allocation_count, 1);
    assert_eq!(
        snapshot.allocation.estimated.expect("allocation").micros,
        17
    );
    assert_eq!(snapshot.daily_rollups.len(), 1);
    assert_eq!(snapshot.window_rollups.len(), 1);
    let window = snapshot.window_rollups.get(&86_400_000).expect("window");
    assert_eq!(window.estimated.as_ref().expect("estimated").micros, 11);
    assert_eq!(window.ledger_entry_count, 1);
    assert_eq!(
        snapshot
            .daily_rollups
            .get("1970-01-02")
            .expect("day")
            .estimated
            .as_ref()
            .expect("estimated")
            .micros,
        11
    );

    // 同一 usage leaf 的第二条分摊拒绝；聚合漂移也拒绝。
    let duplicated = project_billing_ledger(
        7,
        3,
        &[
            ledger.clone(),
            allocation_event(usage_id, entry.run_id, 17, 2),
            allocation_event(usage_id, entry.run_id, 17, 3),
        ],
    )
    .expect("projection");
    assert_eq!(
        duplicated.quarantine[0].reason,
        "allocation_usage_duplicate"
    );
    assert_eq!(duplicated.allocation.allocation_count, 1);
    let stream_drift = allocation_event(UsageId::new(), entry.run_id, 5, 2).with_stream_metadata(
        "cost_allocation",
        EventId::new().to_string(),
        1,
    );
    let drifted = project_billing_ledger(7, 2, &[ledger, stream_drift]).expect("projection");
    assert_eq!(drifted.quarantine[0].reason, "allocation_stream_mismatch");
}

#[test]
fn malformed_cost_is_quarantined_but_reservation_is_not_consumed() {
    let malformed = RuntimeEvent::new(
        RequestId::new(),
        1,
        COST_EVENT_ESTIMATED,
        json!({"run_id": RunId::new()}),
    )
    .expect("event");
    let reservation = RuntimeEvent::new(
        RequestId::new(),
        2,
        "usage.reserved",
        json!({"run_id": RunId::new()}),
    )
    .expect("event");
    let snapshot = project_billing_ledger(9, 2, &[malformed, reservation]).expect("projection");
    assert_eq!(snapshot.source.source_cursor, 2);
    assert_eq!(snapshot.quarantine.len(), 1);
    assert_eq!(snapshot.usage.usage_count, 0);
    assert_eq!(snapshot.ledger.ledger_entry_count, 0);
}

#[test]
fn rebuild_keeps_usage_and_ledger_separate_and_rolls_daily_window() {
    let usage = estimated_event(&usage(AttemptId::new(), 4), 1);
    let entry = ledger_entry(11);
    let ledger = ledger_event(&entry, 2);
    let snapshot = project_billing_ledger(9, 2, &[usage, ledger]).expect("projection");
    assert_eq!(snapshot.source.source_epoch, 9);
    assert_eq!(snapshot.source.source_cursor, 2);
    assert_eq!(snapshot.usage.usage_count, 1);
    assert_eq!(snapshot.ledger.ledger_entry_count, 1);
    assert_eq!(snapshot.daily_rollups.len(), 1);
    assert_eq!(snapshot.window_rollups.len(), 1);
    // 账本事实按 created_at 进入 1970-01-02 / 86_400_000 窗口；usage 不带时间戳。
    assert!(snapshot.daily_rollups.contains_key("1970-01-02"));
    assert!(snapshot.window_rollups.contains_key(&86_400_000));
    snapshot.validate().expect("valid snapshot");
}

#[test]
fn new_process_rebuilds_the_same_snapshot_digest() {
    let shared = usage(AttemptId::new(), 4);
    let entry = ledger_entry(11);
    let correction = correction_fact(&entry, -5, "rebuild");
    let events = vec![
        unknown_event(&shared, 1),
        estimated_event(&shared, 2),
        measured_event(&shared, 7, 3),
        ledger_event(&entry, 4),
        correction_event(&entry, &correction, 2, 5),
    ];

    // 增量两页与一次性重建必须给出同一个读模型。
    let mut projector = BillingLedgerProjector::new(7).expect("projector");
    projector
        .apply_page(&page(events[0..2].to_vec(), 2), 7)
        .expect("page one");
    projector
        .apply_page(&page(events[2..5].to_vec(), 5), 7)
        .expect("page two");
    let rebuilt = project_billing_ledger(7, 5, &events).expect("rebuild");
    assert_eq!(projector.snapshot(), &rebuilt);
    assert_eq!(
        projector.snapshot().projection_digest,
        rebuilt.projection_digest
    );

    // 另一进程按「每页一个事件」重建也必须得到同一摘要。
    let mut single_event_pages = BillingLedgerProjector::new(7).expect("projector");
    for (index, event) in events.iter().enumerate() {
        single_event_pages
            .apply_page(&page(vec![event.clone()], (index as u64) + 1), 7)
            .expect("single event page");
    }
    assert_eq!(
        single_event_pages.snapshot().projection_digest,
        rebuilt.projection_digest
    );

    assert_eq!(rebuilt.usage.measured.expect("measured").micros, 7);
    assert_eq!(rebuilt.ledger.estimated.expect("ledger").micros, 11);
    assert_eq!(
        rebuilt
            .ledger
            .correction_estimated
            .expect("correction")
            .micros,
        -5
    );
    assert_eq!(rebuilt.quarantine.len(), 0);
}
