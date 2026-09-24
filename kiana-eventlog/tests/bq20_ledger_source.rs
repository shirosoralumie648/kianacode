use kiana_domain::{RequestId, RuntimeEvent};
use kiana_eventlog::{read_billing_page, BillingSourcePage, MemoryEventLog};
use kiana_ports::EventStorePort;
use serde_json::json;

#[tokio::test]
async fn source_page_uses_committed_logical_cursor() {
    let store = MemoryEventLog::new();
    store
        .append(RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap())
        .await
        .expect("append");
    store
        .append(RuntimeEvent::new(RequestId::new(), 1, "run.completed", json!({})).unwrap())
        .await
        .expect("append");
    let page = read_billing_page(&store, 3, 0, 16).await.expect("page");
    assert_eq!(page.first_cursor, Some(1));
    assert_eq!(page.cursor(), 2);
    assert_eq!(page.events().len(), 2);
}

#[test]
fn source_page_rejects_non_contiguous_cursor() {
    let page = kiana_domain::JournalPage {
        events: vec![RuntimeEvent::new(RequestId::new(), 1, "run.started", json!({})).unwrap()],
        cursor: 2,
        has_more: false,
    };
    assert!(BillingSourcePage::new(3, 0, page).is_err());
}

#[test]
fn source_adapter_is_read_only() {
    let source = include_str!("../src/ledger_source.rs");
    for forbidden in [
        "append(",
        "commit_transition",
        "CapabilityBroker",
        "tokio::spawn",
    ] {
        assert!(
            !source.contains(forbidden),
            "source boundary widened: {forbidden}"
        );
    }
}
