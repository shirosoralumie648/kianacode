use kiana_domain::{CommunicationMessage, CommunicationMessageKind, COMMUNICATION_MESSAGE_SCHEMA};
use serde_json::json;

#[test]
fn free_chat_never_grants_authority() {
    let chat = CommunicationMessage::chat(
        "message-1",
        "local-user",
        Some("operator".to_owned()),
        "Please review the current status.",
    )
    .unwrap();
    assert_eq!(chat.schema, COMMUNICATION_MESSAGE_SCHEMA);
    assert!(!chat.grants_authority());
    assert!(chat.action_ref.is_none());
    assert!(!chat.ack_required);
    chat.validate().unwrap();

    // A chat payload cannot be upgraded into a command/approval by adding an action reference.
    assert_eq!(
        chat.clone().with_action_ref("company.approve").unwrap_err(),
        "communication_chat_authority_fields_forbidden"
    );
    let mut encoded = serde_json::to_value(&chat).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<CommunicationMessage>(encoded).is_err());
}

#[test]
fn handoff_is_directed_and_requires_ack() {
    let handoff = CommunicationMessage::new(
        "handoff-1",
        CommunicationMessageKind::Handoff,
        "planner",
        Some("builder".to_owned()),
        "packet handoff",
        "Execute the frozen packet.",
    )
    .unwrap();
    assert!(handoff.ack_required);
    assert!(!handoff.grants_authority());
    assert!(CommunicationMessage::new(
        "handoff-2",
        CommunicationMessageKind::Handoff,
        "planner",
        None,
        "packet handoff",
        "missing recipient",
    )
    .is_err());
}
