//! BQ-20 committed source-page fixtures (deny first).
//!
//! The adapter reads complete committed pages through `EventStorePort`; it never appends a fact,
//! never creates a second ledger and never rewrites the page it just read. All fixtures use the
//! in-memory store or in-memory pages; no filesystem, provider or network is involved.

use kiana_domain::{EventId, JournalPage, RequestId, RuntimeEvent};
use kiana_eventlog::{
    read_billing_page, BillingSourcePage, MemoryEventLog, BILLING_SOURCE_IS_READ_ONLY,
};
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

fn event(kind: &str) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), 1, kind, json!({})).expect("event")
}

#[tokio::test]
async fn source_page_uses_committed_logical_cursor() {
    let store = MemoryEventLog::new();
    store.append(event("run.started")).await.expect("append");
    store.append(event("run.completed")).await.expect("append");
    let page = read_billing_page(&store, 3, 0, 16).await.expect("page");
    assert_eq!(page.first_cursor, Some(1));
    assert_eq!(page.cursor(), 2);
    assert_eq!(page.events().len(), 2);
}

#[test]
fn source_page_rejects_non_contiguous_cursor() {
    let page = JournalPage {
        events: vec![event("run.started")],
        cursor: 2,
        has_more: false,
    };
    assert_eq!(
        BillingSourcePage::new(3, 0, page).unwrap_err(),
        PortError::Conflict("billing_source_cursor_not_contiguous".to_owned())
    );
}

#[test]
fn source_page_rejects_zero_epoch_and_cursor_regression() {
    let empty = JournalPage {
        events: vec![],
        cursor: 0,
        has_more: false,
    };
    assert_eq!(
        BillingSourcePage::new(0, 0, empty).unwrap_err(),
        PortError::Failed("billing_source_epoch_invalid".to_owned())
    );
    let regressed = JournalPage {
        events: vec![],
        cursor: 3,
        has_more: false,
    };
    assert_eq!(
        BillingSourcePage::new(2, 5, regressed).unwrap_err(),
        PortError::Conflict("billing_source_cursor_regressed".to_owned())
    );
}

#[test]
fn source_page_rejects_duplicate_or_nil_event_ids() {
    let first = event("run.started");
    let duplicate = first.clone();
    let page = JournalPage {
        events: vec![first, duplicate],
        cursor: 2,
        has_more: false,
    };
    assert_eq!(
        BillingSourcePage::new(3, 0, page).unwrap_err(),
        PortError::Conflict("billing_source_event_duplicate".to_owned())
    );

    let mut nil_event = event("run.started");
    nil_event.event_id =
        EventId::parse_str("00000000-0000-0000-0000-000000000000").expect("nil id");
    let page = JournalPage {
        events: vec![nil_event],
        cursor: 1,
        has_more: false,
    };
    assert_eq!(
        BillingSourcePage::new(3, 0, page).unwrap_err(),
        PortError::Failed("billing_source_event_id_invalid".to_owned())
    );
}

#[test]
fn source_page_empty_page_is_a_no_progress_page() {
    let page = JournalPage {
        events: vec![],
        cursor: 4,
        has_more: false,
    };
    let wrapped = BillingSourcePage::new(3, 4, page).expect("empty page");
    assert_eq!(wrapped.cursor(), 4);
    assert_eq!(wrapped.first_cursor, None);
    assert!(wrapped.events().is_empty());

    // an empty page whose cursor claims progress is a gap, not a silent no-op
    let gap = JournalPage {
        events: vec![],
        cursor: 5,
        has_more: false,
    };
    assert_eq!(
        BillingSourcePage::new(3, 4, gap).unwrap_err(),
        PortError::Conflict("billing_source_cursor_not_contiguous".to_owned())
    );
}

#[tokio::test]
async fn source_page_snapshot_does_not_append_to_the_log() {
    let store = MemoryEventLog::new();
    store.append(event("run.started")).await.expect("append");
    let before = store.read_from(0, 16).await.expect("before");
    let page = read_billing_page(&store, 3, 0, 16).await.expect("page");
    let after = store.read_from(0, 16).await.expect("after");
    assert_eq!(before.cursor, after.cursor);
    assert_eq!(before.events.len(), after.events.len());
    assert_eq!(page.cursor(), after.cursor);
    assert!(BILLING_SOURCE_IS_READ_ONLY);
}

#[test]
fn source_adapter_is_read_only() {
    let source = include_str!("../src/ledger_source.rs");
    for marker in [
        "BillingSourcePage",
        "read_billing_page",
        "read_ledger_page",
        "BILLING_SOURCE_IS_READ_ONLY",
        "billing_source_cursor_regressed",
        "billing_source_cursor_not_contiguous",
        "billing_source_event_id_invalid",
        "billing_source_event_duplicate",
    ] {
        assert!(
            source.contains(marker),
            "BQ-20 source marker missing: {marker}"
        );
    }
    for forbidden in [
        ".append(",
        "commit_transition",
        "CapabilityBroker",
        "release_reservation",
        "ApprovalStore",
        "tokio::spawn",
        "std::fs",
        "#[ignore",
    ] {
        assert!(
            !source.contains(forbidden),
            "source boundary widened: {forbidden}"
        );
    }
}
