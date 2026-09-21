use kiana_domain::{RetentionWatermark, RetentionWatermarkState};

#[test]
fn retention_watermark_commits_only_after_hold_and_tombstone_gates() {
    let plan = RetentionWatermark::planned("store-1", 2, 3, 10, 20, 100).unwrap();
    let blocked = plan.clone().commit(false, true).unwrap();
    assert_eq!(blocked.state, RetentionWatermarkState::Blocked);
    assert_eq!(
        blocked.reason.as_deref(),
        Some("retention_tombstone_or_hold_gate_missing")
    );
    let committed = plan.commit(true, true).unwrap();
    assert_eq!(committed.state, RetentionWatermarkState::Committed);
    assert!(committed.advance(15).unwrap().validate().is_ok());
}

#[test]
fn retention_watermark_cannot_regress_or_exceed_bound() {
    let committed = RetentionWatermark::planned("store-1", 2, 3, 10, 20, 100)
        .unwrap()
        .commit(true, true)
        .unwrap();
    assert_eq!(
        committed.advance(9).unwrap_err(),
        "retention_watermark_advance_invalid"
    );
    assert_eq!(
        committed.advance(21).unwrap_err(),
        "retention_watermark_advance_invalid"
    );
}
