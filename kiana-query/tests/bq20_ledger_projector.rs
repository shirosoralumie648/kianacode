use kiana_domain::*;
use kiana_query::{project_billing_ledger, BillingLedgerProjectionError, BillingLedgerProjector};
use serde_json::json;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn usage_event() -> RuntimeEvent {
    let usage = NormalizedUsage::new(
        UsageId::new(),
        AttemptId::new(),
        RunId::new(),
        Some("provider:test".to_owned()),
        "model:test",
        "route:test",
        UsageVector {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(4),
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
    .expect("usage");
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider:test",
        "model:test",
        "USD",
        Some(1),
        Some(2),
        1_000,
        Some(10_000),
        1,
        "bq20-fixture",
    )
    .expect("card");
    card.request_price = Some(1);
    card.rate_card_digest = card.digest();
    let fact = CostBreakdown::from_usage(&usage, Some(&card), 1, 86_400_001).expect("cost");
    fact.to_runtime_event(RequestId::new(), 1).expect("event")
}

fn ledger_event() -> RuntimeEvent {
    let entry = CostLedgerEntry::new(
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
    .with_estimated_cost(Money::new("USD", 11).expect("money"))
    .expect("estimate");
    RuntimeEvent::new(
        RequestId::new(),
        2,
        COST_LEDGER_ENTRY_EVENT,
        serde_json::to_value(&entry).expect("entry json"),
    )
    .expect("event")
    .with_stream_metadata("cost_ledger", entry.entry_id.to_string(), entry.revision)
}

#[test]
fn rebuild_keeps_usage_and_ledger_separate_and_rolls_daily_window() {
    let usage = usage_event();
    let ledger = ledger_event();
    let snapshot = project_billing_ledger(9, 2, &[usage, ledger]).expect("projection");
    assert_eq!(snapshot.source.source_epoch, 9);
    assert_eq!(snapshot.source.source_cursor, 2);
    assert_eq!(snapshot.usage.usage_count, 1);
    assert_eq!(snapshot.ledger.ledger_entry_count, 1);
    assert_eq!(snapshot.daily_rollups.len(), 1);
    assert_eq!(snapshot.window_rollups.len(), 1);
    snapshot.validate().expect("valid snapshot");
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
fn failed_page_does_not_advance_cursor_and_replay_is_rejected() {
    let mut projector = BillingLedgerProjector::new(4).expect("projector");
    let event = usage_event();
    let page = JournalPage {
        events: vec![event.clone()],
        cursor: 1,
        has_more: false,
    };
    projector.apply_page(&page, 4).expect("page");
    let duplicate = JournalPage {
        events: vec![event],
        cursor: 2,
        has_more: false,
    };
    assert_eq!(
        projector.apply_page(&duplicate, 4),
        Err(BillingLedgerProjectionError::SourceEventDuplicate)
    );
    assert_eq!(projector.source_cursor(), 1);
}
