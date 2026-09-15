use kiana_core::reduce_committed_audit_records;
use kiana_domain::{AuditDecision, RequestId, RuntimeEvent, SERVER_AUDIT_ACTOR};
use serde_json::json;

fn event(kind: &str, data: serde_json::Value) -> RuntimeEvent {
    let mut data = data;
    data["run_id"] = json!("run-core-oa04");
    data["authority_epoch"] = json!(2);
    data["data_epoch"] = json!(3);
    RuntimeEvent::new(RequestId::new(), 1, kind, data)
        .unwrap()
        .with_stream_metadata("run", "run-core-oa04", 1)
}

#[test]
fn core_facade_only_projects_committed_domain_facts() {
    let events = vec![
        event("opaque.event", json!({})),
        event("capability.decision", json!({"gate":{"decision":"denied"}})),
        event(
            "capability.result_unknown",
            json!({"capability_request_id":"cap-unknown"}),
        ),
    ];
    let records = reduce_committed_audit_records(&events, 41).expect("core audit projection");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].decision, AuditDecision::Denied);
    assert_eq!(records[1].decision, AuditDecision::Unknown);
    assert!(records
        .iter()
        .all(|record| record.actor_ref == SERVER_AUDIT_ACTOR));
    assert_eq!(records[0].source_cursor, 42);
    assert_eq!(records[1].source_cursor, 43);
}

#[test]
fn core_facade_rejects_untrusted_audit_event_and_missing_source_cursor() {
    let forged = event("audit.completed", json!({}));
    assert_eq!(
        reduce_committed_audit_records(&[forged], 1).unwrap_err(),
        "audit_event_kind_untrusted"
    );
    assert_eq!(
        reduce_committed_audit_records(&[], 0).unwrap_err(),
        "audit_source_cursor_required"
    );
}
