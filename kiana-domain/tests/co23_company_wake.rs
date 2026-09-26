use kiana_domain::*;

fn wake(intent: &str, cursor: u64) -> CompanyWake {
    CompanyWake::new(
        intent,
        "process-1",
        CompanyWakeKind::IntentReady,
        cursor,
        format!("sha256:{}", "a".repeat(64)),
        10,
    )
    .expect("wake")
}

#[test]
fn duplicate_wakeup_and_crash_after_intent_cannot_duplicate_effects() {
    let mut ledger = CompanyWakeLedger::default();
    ledger.enqueue(wake("intent-1", 1)).expect("enqueue");
    ledger.enqueue(wake("intent-1", 2)).expect("coalesce");
    assert_eq!(ledger.wakes.len(), 1);
    assert_eq!(ledger.cursor, 2);
    let claimed = ledger
        .claim("intent-1", "daemon-1", "claim-1", 10)
        .expect("claim");
    assert_eq!(claimed.attempts, 1);
    ledger
        .mark_unknown("intent-1", "claim-1", "dispatch-1")
        .expect("unknown");
    assert_eq!(ledger.wakes["intent-1"].status, CompanyWakeStatus::Unknown);
    assert!(ledger.claim("intent-1", "daemon-2", "claim-2", 10).is_err());
}

#[test]
fn committed_human_decision_wakes_exactly_one_next_activation_after_restart() {
    let mut ledger = CompanyWakeLedger::default();
    let original = wake("intent-human", 7);
    ledger.enqueue(original.clone()).expect("enqueue");
    let reopened: CompanyWakeLedger =
        serde_json::from_value(serde_json::to_value(&ledger).expect("serialize")).expect("reopen");
    let mut ledger = reopened;
    let claimed = ledger
        .claim("intent-human", "daemon-1", "claim-human", 10)
        .expect("claim");
    let receipt = CompanyDispatchReceipt::new(
        "receipt-1",
        &claimed.intent_id,
        "dispatch-human",
        CompanyWakeStatus::Consumed,
        true,
        7,
        11,
    )
    .expect("receipt");
    ledger
        .consume("intent-human", "claim-human", receipt.clone())
        .expect("consume");
    ledger
        .consume("intent-human", "claim-human", receipt)
        .expect("idempotent consume");
    assert_eq!(
        ledger.wakes["intent-human"].status,
        CompanyWakeStatus::Consumed
    );
    assert!(ledger.pending(100).is_empty());
}

#[test]
fn unknown_receipt_cannot_be_relabelled_as_consumed() {
    let mut ledger = CompanyWakeLedger::default();
    ledger.enqueue(wake("intent-unknown", 3)).expect("enqueue");
    let claimed = ledger
        .claim("intent-unknown", "daemon", "claim", 10)
        .expect("claim");
    let receipt = CompanyDispatchReceipt::new(
        "receipt-unknown",
        &claimed.intent_id,
        "dispatch-unknown",
        CompanyWakeStatus::Unknown,
        false,
        3,
        12,
    )
    .expect("unknown receipt");
    ledger
        .consume("intent-unknown", "claim", receipt)
        .expect("unknown");
    assert_eq!(
        ledger.wakes["intent-unknown"].status,
        CompanyWakeStatus::Unknown
    );
    assert!(ledger.pending(100).is_empty());
}
