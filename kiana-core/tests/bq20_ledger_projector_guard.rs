use kiana_core::{BillingProjectionFence, BillingProjectionFenceError};
use kiana_domain::*;
use serde_json::json;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

#[test]
fn fence_requires_candidate_projection_before_cursor_ack() {
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let source = BillingProjectionCursor::new(5, 1, BILLING_PROJECTION_NUMBER, digest('f'), vec![])
        .expect("cursor");
    let candidate = BillingProjectionSnapshot::new(
        source,
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        Default::default(),
        Default::default(),
        Default::default(),
    )
    .expect("snapshot");
    let page = JournalPage {
        events: vec![RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap()],
        cursor: 1,
        has_more: false,
    };
    fence.stage(&page, &candidate, 5).expect("stage");
    assert_eq!(fence.source_cursor(), 1);
}

#[test]
fn fence_rejects_cursor_gap_and_preserves_previous_cursor() {
    let mut fence = BillingProjectionFence::new(5).expect("fence");
    let source = BillingProjectionCursor::new(5, 2, BILLING_PROJECTION_NUMBER, digest('f'), vec![])
        .expect("cursor");
    let candidate = BillingProjectionSnapshot::new(
        source,
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        BillingRollupTotals::default(),
        Default::default(),
        Default::default(),
        Default::default(),
    )
    .expect("snapshot");
    let page = JournalPage {
        events: vec![RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap()],
        cursor: 2,
        has_more: false,
    };
    assert_eq!(
        fence.stage(&page, &candidate, 5),
        Err(BillingProjectionFenceError::CursorGap)
    );
    assert_eq!(fence.source_cursor(), 0);
}

#[test]
fn core_boundary_is_not_an_event_store_or_execution_loop() {
    let source = include_str!("../src/billing_projection.rs");
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "append(",
        "tokio::spawn",
    ] {
        assert!(
            !source.contains(forbidden),
            "core boundary widened: {forbidden}"
        );
    }
}
