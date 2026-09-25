use kiana_domain::{
    ActionRef, MessageId, Notification, NotificationActionCommand, NotificationActionKind,
    NotificationChannel,
};
use serde_json::json;
use std::fs;
use std::path::Path;

fn command(action: NotificationActionKind) -> NotificationActionCommand {
    let notification = Notification::new(
        MessageId::new(),
        "actor-10",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        10_000,
        3,
        None,
    )
    .unwrap();
    let action_ref = ActionRef::new(
        action.command(),
        3,
        vec![format!("notification:{}", notification.notification_id)],
        100,
        200,
    )
    .unwrap();
    NotificationActionCommand::new(
        action_ref,
        action,
        notification.notification_id,
        "actor-10",
        3,
        notification.notification_digest,
        7,
        11,
        json!({"reason":"review"}),
        "actor-10",
        "notification-action-10",
    )
    .unwrap()
}

#[test]
fn fixture_declares_action_command_deny_first_contract() {
    let value: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/nm10-notification-actions.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.notification-action-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("stale")));
}

#[test]
fn action_command_binds_ref_target_epoch_cursor_and_redacted_payload() {
    let command = command(NotificationActionKind::Review);
    command.validate().unwrap();
    assert_eq!(command.action_ref.command, "notification.review");
    assert_eq!(command.expected_source_cursor, 11);

    let mut wrong_kind = command.clone();
    wrong_kind.action = NotificationActionKind::Approve;
    assert!(wrong_kind.validate().is_err());

    let mut secret = command.clone();
    secret.payload = json!({"token":"raw-secret"});
    assert!(secret.validate().is_err());

    let mut widened = command;
    widened.action_ref.scope = vec!["project/a".to_owned()];
    assert!(widened.validate().is_err());
}
