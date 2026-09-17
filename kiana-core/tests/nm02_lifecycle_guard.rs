#[test]
fn communication_lifecycle_uses_eventlog_and_never_dispatches_from_message_text() {
    let domain = include_str!("../../kiana-domain/src/communication.rs");
    let core = include_str!("../src/communication.rs");
    let commands = include_str!("../src/commands.rs");
    let events = include_str!("../src/events.rs");
    for marker in [
        "CommunicationLifecycleStatus",
        "CommunicationLifecycleEvent",
        "communication_handoff_ack_invalid",
        "communication_lifecycle_transition_invalid",
        "authority_granted",
        "false",
    ] {
        assert!(
            domain.contains(marker),
            "domain lifecycle marker missing: {marker}"
        );
    }
    for marker in [
        "communication.ack",
        "communication.reject",
        "communication.escalate",
        "load_communication",
        "communication.handoff_acknowledged",
        "communication.handoff_rejected",
        "communication.incident_escalated",
        "communication_sender_mismatch",
        "communication_sender_role_invalid",
        "CommunicationLifecycleEvent::new",
        "record_event",
    ] {
        assert!(
            core.contains(marker),
            "core lifecycle marker missing: {marker}"
        );
    }
    assert!(commands.contains("communication.ack"));
    assert!(commands.contains("communication.reject"));
    assert!(commands.contains("communication.escalate"));
    assert!(events.contains("\"communication\".to_owned()"));
    assert!(!core.contains("handle_company_command"));
    assert!(!core.contains("authorize_and_execute"));
}
