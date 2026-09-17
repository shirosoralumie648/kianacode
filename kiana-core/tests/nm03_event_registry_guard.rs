#[test]
fn notification_registry_is_server_owned_and_checked_at_event_boundary() {
    let domain = include_str!("../../kiana-domain/src/notification_events.rs");
    let event_contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let core = include_str!("../src/events.rs");
    for marker in [
        "NOTIFICATION_EVENT_SPECS",
        "NotificationEventClass",
        "NotificationEventSource",
        "notification_event_kind_unregistered",
        "notification_event_source_untrusted",
        "notification_event_owner_required",
        "RunTerminal",
        "RunUnknown",
        "Handoff",
        "Status",
        "Evidence",
        "Incident",
        "Reminder",
    ] {
        assert!(
            domain.contains(marker),
            "notification registry marker missing: {marker}"
        );
    }
    for marker in [
        "COMMUNICATION_LIFECYCLE_IDS",
        "communication.handoff_acknowledged",
        "communication.incident_escalated",
        "EVENT_KIND_SPECS",
    ] {
        assert!(
            event_contracts.contains(marker),
            "event registry marker missing: {marker}"
        );
    }
    for marker in [
        "data.get(\"source\").is_some()",
        "notification_event_source",
        "validate_notification_event",
        "prepare_event_payload",
    ] {
        assert!(
            core.contains(marker),
            "core boundary marker missing: {marker}"
        );
    }
    assert!(!core.contains("source == \"model\" && execute"));
}
