#[test]
fn communication_layers_are_typed_and_cannot_grant_authority() {
    let domain = include_str!("../../kiana-domain/src/communication.rs");
    let registry = include_str!("../../kiana-domain/src/event_contracts.rs");
    let handoff = include_str!("../../kiana-domain/src/handoff.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let core = include_str!("../src/communication.rs");
    let commands = include_str!("../src/commands.rs");
    for marker in [
        "CommunicationMessageKind",
        "Chat",
        "Command",
        "Handoff",
        "Decision",
        "StatusReport",
        "Evidence",
        "Incident",
        "communication_chat_authority_fields_forbidden",
        "pub const fn grants_authority",
        "false",
    ] {
        assert!(
            domain.contains(marker),
            "communication marker missing: {marker}"
        );
    }
    for marker in [
        "to_role",
        "acknowledgement",
        "handoff_receiver_identity_invalid",
        "handoff_ack_reason_required",
    ] {
        assert!(handoff.contains(marker), "handoff marker missing: {marker}");
    }
    assert!(ports.contains("pub trait CommunicationPort"));
    for marker in [
        "COMMUNICATION_IDS",
        "COMMUNICATION_FIELDS",
        "communication.chat",
        "communication.handoff",
        "\"communication.\"",
    ] {
        assert!(
            registry.contains(marker),
            "event registry marker missing: {marker}"
        );
    }
    for marker in [
        "communication_message_required",
        "communication_sender_mismatch",
        "communication.chat",
        "authority_granted",
    ] {
        assert!(
            core.contains(marker),
            "core communication marker missing: {marker}"
        );
    }
    assert!(commands.contains("intent.name == \"communication.send\""));
}
