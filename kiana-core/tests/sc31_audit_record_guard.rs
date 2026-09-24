#[test]
fn sc31_audit_record_is_server_derived_and_eventlog_bound() {
    let observability = include_str!("../../kiana-domain/src/observability.rs");
    let reducer = include_str!("../../kiana-domain/src/audit.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let eventlog_boundary = include_str!("../../kiana-eventlog/src/audit_contract.rs");
    let eventlog_plan = include_str!("../../kiana-eventlog/src/event_store_core.rs");
    let journal_plan = include_str!("../../kiana-eventlog/src/journal_core.rs");

    for marker in [
        "AUDIT_RECORD_SCHEMA",
        "AUDIT_EVENT_SCHEMA",
        "AuditRecordEvent",
        "append_only",
        "payload_recoverable",
        "redaction_profile",
        "audit_event_source_binding_mismatch",
    ] {
        assert!(
            observability.contains(marker),
            "SC-31 domain marker missing: {marker}"
        );
    }
    for marker in [
        "SERVER_AUDIT_ACTOR",
        "record.reason",
        "classify_audit_event",
        "audit_event_kind_untrusted",
        "audit_record_payload_untrusted",
    ] {
        assert!(
            reducer.contains(marker),
            "SC-31 reducer marker missing: {marker}"
        );
    }
    assert!(contracts.contains("kiana.audit-event.v1"));
    assert!(protocol.contains("AuditRecordEvent"));
    for marker in [
        "AuditRecordEvent",
        "eventlog_audit_kind_untrusted",
        "eventlog_audit_payload_invalid",
        "eventlog_audit_redaction_binding_mismatch",
        "eventlog_audit_idempotency_binding_mismatch",
        "payload_recoverable",
        "artifact_refs",
    ] {
        assert!(
            eventlog_boundary.contains(marker),
            "SC-31 EventLog boundary marker missing: {marker}"
        );
    }
    assert!(eventlog_plan.contains("audit_contract::validate_runtime_event"));
    assert!(journal_plan.contains("audit_contract::validate_runtime_event"));
    assert!(!eventlog_boundary.contains("EventStorePort"));
    assert!(!eventlog_boundary.contains("std::fs"));
}
