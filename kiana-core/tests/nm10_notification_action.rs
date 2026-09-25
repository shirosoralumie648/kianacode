use kiana_core::{NotificationActionGate, NotificationActionGateError};
use kiana_domain::{
    ActionRef, MessageId, Notification, NotificationActionCommand, NotificationActionKind,
    NotificationChannel,
};
use serde_json::json;

fn fixture() -> (NotificationActionCommand, ActionRef) {
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
        NotificationActionKind::Review.command(),
        3,
        vec![format!("notification:{}", notification.notification_id)],
        100,
        200,
    )
    .unwrap();
    let command = NotificationActionCommand::new(
        action_ref.clone(),
        NotificationActionKind::Review,
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
    .unwrap();
    (command, action_ref)
}

#[test]
fn gate_accepts_only_authoritative_fresh_target_and_returns_no_effect() {
    let (command, action_ref) = fixture();
    let admission = NotificationActionGate::admit(
        &command,
        &action_ref,
        "actor-10",
        3,
        &command.expected_target_digest,
        7,
        11,
        150,
    )
    .unwrap();
    assert!(admission.control_plane_required);
    assert!(!admission.direct_effect);

    let mut foreign = command.clone();
    foreign.submitted_by = "foreign".to_owned();
    foreign.command_digest = foreign.digest();
    assert_eq!(
        NotificationActionGate::admit(
            &foreign,
            &action_ref,
            "actor-10",
            3,
            &command.expected_target_digest,
            7,
            11,
            150
        ),
        Err(NotificationActionGateError::RecipientMismatch)
    );
    assert_eq!(
        NotificationActionGate::admit(
            &command,
            &action_ref,
            "actor-10",
            4,
            &command.expected_target_digest,
            7,
            11,
            150
        ),
        Err(NotificationActionGateError::RevisionConflict)
    );
    assert_eq!(
        NotificationActionGate::admit(
            &command,
            &action_ref,
            "actor-10",
            3,
            &command.expected_target_digest,
            8,
            11,
            150
        ),
        Err(NotificationActionGateError::AuthorityConflict)
    );
    assert_eq!(
        NotificationActionGate::admit(
            &command,
            &action_ref,
            "actor-10",
            3,
            &command.expected_target_digest,
            7,
            11,
            201
        ),
        Err(NotificationActionGateError::Expired)
    );
}
