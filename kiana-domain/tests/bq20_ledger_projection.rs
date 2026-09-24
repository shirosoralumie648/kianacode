use kiana_domain::*;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

#[test]
fn cursor_binds_epoch_projection_version_and_replay_fence() {
    let cursor = BillingProjectionCursor::new(
        7,
        12,
        BILLING_PROJECTION_NUMBER,
        digest('f'),
        vec![EventId::new()],
    )
    .expect("cursor");
    cursor.validate().expect("valid cursor");
    let mut drift = cursor.clone();
    drift.source_epoch = 8;
    assert_eq!(
        drift.validate().unwrap_err(),
        "billing_projection_cursor_digest_mismatch"
    );
}

#[test]
fn rollup_keeps_estimated_measured_and_correction_separate() {
    let mut totals = BillingRollupTotals::default();
    totals
        .add_estimated(&Money::new("USD", 10).expect("money"))
        .expect("estimate");
    totals
        .add_measured(&Money::new("USD", 11).expect("money"))
        .expect("measured");
    totals
        .add_correction_estimated(&Money::new("USD", -2).expect("money"))
        .expect("correction");
    assert_eq!(totals.estimated.expect("estimate").micros, 10);
    assert_eq!(totals.measured.expect("measured").micros, 11);
    assert_eq!(totals.correction_estimated.expect("correction").micros, -2);
    assert!(totals.correction_measured.is_none());
}

#[test]
fn quarantine_record_is_queryable_and_secret_free() {
    let record = BillingQuarantineRecord::new(
        3,
        EventId::new(),
        COST_EVENT_ESTIMATED,
        "decode_failed",
        digest('q'),
    )
    .expect("record");
    record.validate().expect("valid record");
    let json = serde_json::to_value(&record).expect("json");
    assert!(json.get("payload_digest").is_some());
    assert!(json.get("raw_payload").is_none());
}
