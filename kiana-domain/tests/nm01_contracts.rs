use kiana_domain::*;
use serde_json::json;

fn message() -> Message {
    Message::new(
        MessageKind::Chat,
        "planner",
        "builder",
        None,
        vec!["project/a".to_owned()],
        "bounded hello",
        None,
        100,
        Some(200),
    )
    .unwrap()
}

fn message_with_body_directly(body: &str) -> Message {
    let mut message = Message {
        schema: MESSAGE_SCHEMA.to_owned(),
        message_id: MessageId::new(),
        kind: MessageKind::Chat,
        sender_id: "planner".to_owned(),
        recipient_id: "builder".to_owned(),
        project_id: None,
        scope: vec!["project/a".to_owned()],
        body: body.to_owned(),
        action_ref_id: None,
        created_at_unix_ms: 100,
        expires_at_unix_ms: Some(200),
        message_digest: String::new(),
    };
    message.message_digest = message.digest();
    message
}

fn subscription() -> Subscription {
    Subscription::new(
        "builder",
        None,
        vec!["project/a".to_owned()],
        vec![NotificationChannel::InApp],
        1,
        100,
        200,
    )
    .unwrap()
}

#[test]
fn notification_contracts_round_trip_and_scope_stays_bounded() {
    let message = message();
    message.validate().unwrap();
    let subscription = subscription();
    let mut notification = Notification::new(
        message.message_id,
        "builder",
        None,
        vec!["project/a/file".to_owned()],
        NotificationChannel::InApp,
        100,
        200,
        subscription.revision,
        None,
    )
    .unwrap();
    notification
        .validate_for_subscription(&subscription)
        .unwrap();
    let encoded = canonical_notification_bytes(&notification).unwrap();
    let decoded: Notification = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, notification);

    let action =
        ActionRef::new("company.review", 7, vec!["project/a".to_owned()], 100, 200).unwrap();
    action.validate().unwrap();
    notification.action_ref_id = Some(action.action_ref_id);
    notification.notification_digest = notification.digest();
    notification.validate().unwrap();
    notification
        .transition_status(NotificationStatus::Queued)
        .unwrap();
    notification
        .transition_status(NotificationStatus::Delivered)
        .unwrap();
    notification
        .transition_status(NotificationStatus::Read)
        .unwrap();
    assert!(notification.status.is_terminal());
    assert_eq!(
        notification
            .transition_status(NotificationStatus::Queued)
            .unwrap_err(),
        "notification_transition_invalid"
    );

    let mut overbroad = Notification::new(
        message.message_id,
        "builder",
        None,
        vec!["project".to_owned()],
        NotificationChannel::InApp,
        100,
        200,
        1,
        None,
    )
    .unwrap();
    overbroad.notification_digest = overbroad.digest();
    assert_eq!(
        overbroad
            .validate_for_subscription(&subscription)
            .unwrap_err(),
        "notification_scope_exceeds_subscription"
    );

    let stale_revision = Notification::new(
        message.message_id,
        "builder",
        None,
        vec!["project/a/file".to_owned()],
        NotificationChannel::InApp,
        100,
        200,
        subscription.revision + 1,
        None,
    )
    .unwrap();
    assert_eq!(
        stale_revision
            .validate_for_subscription(&subscription)
            .unwrap_err(),
        "notification_subscription_revision_mismatch"
    );
}

#[test]
fn message_rejects_empty_recipient_long_body_and_secret_debug_payload() {
    assert_eq!(
        Message::new(
            MessageKind::Chat,
            "planner",
            "",
            None,
            Vec::new(),
            "hello",
            None,
            1,
            None,
        )
        .unwrap_err(),
        "message_recipient_invalid"
    );
    assert_eq!(
        Message::new(
            MessageKind::Chat,
            "planner",
            "builder",
            None,
            Vec::new(),
            "x".repeat(MAX_MESSAGE_BODY_BYTES + 1),
            None,
            1,
            None,
        )
        .unwrap_err(),
        "message_body_invalid"
    );
    assert_eq!(
        Message::new(
            MessageKind::Chat,
            "planner",
            "builder",
            None,
            Vec::new(),
            "Authorization: Bearer top-secret",
            None,
            1,
            None,
        )
        .unwrap_err(),
        "message_body_secret_detected"
    );
}

#[test]
fn message_secret_marker_is_rejected_at_serde_boundaries_and_redacted_from_debug() {
    let sentinel = "nm01-serialization-sentinel";
    let secret_body = format!("Authorization: Bearer {sentinel}");
    let secret_message = message_with_body_directly(&secret_body);
    let debug = format!("{secret_message:?}");
    assert!(!debug.contains(sentinel));

    let valid = message();
    let valid_bytes = serde_json::to_vec(&valid).unwrap();
    let decoded: Message = serde_json::from_slice(&valid_bytes).unwrap();
    assert_eq!(decoded, valid);

    let serialization_error = serde_json::to_string(&secret_message).unwrap_err();
    assert!(serialization_error
        .to_string()
        .contains("message_body_secret_detected"));
    assert!(!serialization_error.to_string().contains(sentinel));

    let payload = json!({
        "schema": MESSAGE_SCHEMA,
        "message_id": secret_message.message_id,
        "kind": "chat",
        "sender_id": "planner",
        "recipient_id": "builder",
        "project_id": null,
        "scope": ["project/a"],
        "body": &secret_message.body,
        "action_ref_id": null,
        "created_at_unix_ms": 100,
        "expires_at_unix_ms": 200,
        "message_digest": &secret_message.message_digest,
    });
    let deserialization_error = serde_json::from_value::<Message>(payload).unwrap_err();
    assert!(deserialization_error
        .to_string()
        .contains("message_body_secret_detected"));
    assert!(!deserialization_error.to_string().contains(sentinel));
}

#[test]
fn message_deserialization_rejects_unknown_kind_and_schema() {
    let valid = serde_json::to_value(message()).unwrap();

    let mut unknown_kind = valid.clone();
    unknown_kind["kind"] = json!("unrecognized_kind");
    assert!(serde_json::from_value::<Message>(unknown_kind).is_err());

    let mut unknown_schema = valid;
    unknown_schema["schema"] = json!("kiana.message.v9");
    assert_eq!(
        serde_json::from_value::<Message>(unknown_schema)
            .unwrap_err()
            .to_string(),
        "message_schema_invalid"
    );
}

#[test]
fn delivery_attempt_receipt_status_and_ttl_transitions_are_fail_closed() {
    let message = message();
    let subscription = subscription();
    let notification = Notification::new(
        message.message_id,
        "builder",
        None,
        vec!["project/a".to_owned()],
        NotificationChannel::InApp,
        100,
        200,
        1,
        None,
    )
    .unwrap();
    let mut attempt = DeliveryAttempt::new(
        notification.notification_id,
        subscription.subscription_id,
        1,
        100,
    )
    .unwrap();
    attempt
        .transition_status(DeliveryAttemptStatus::Claimed, 101)
        .unwrap();
    attempt
        .transition_status(DeliveryAttemptStatus::Submitted, 102)
        .unwrap();
    attempt
        .transition_status(DeliveryAttemptStatus::Acknowledged, 103)
        .unwrap();
    assert_eq!(
        attempt
            .transition_status(DeliveryAttemptStatus::Failed, 104)
            .unwrap_err(),
        "delivery_attempt_transition_invalid"
    );
    let receipt = DeliveryReceipt::new(
        notification.notification_id,
        attempt.delivery_attempt_id,
        "builder",
        DeliveryReceiptStatus::Acknowledged,
        104,
        None,
    )
    .unwrap();
    receipt.validate().unwrap();

    assert_eq!(
        Notification::new(
            message.message_id,
            "builder",
            None,
            vec!["project/a".to_owned()],
            NotificationChannel::InApp,
            100,
            100,
            1,
            None,
        )
        .unwrap_err(),
        "notification_ttl_invalid"
    );
    assert_eq!(
        DeliveryAttempt::new(
            notification.notification_id,
            subscription.subscription_id,
            0,
            100,
        )
        .unwrap_err(),
        "delivery_attempt_header_invalid"
    );
}

#[test]
fn message_v0_upcast_is_explicit_and_unknown_major_or_field_is_rejected() {
    let legacy_id = MessageId::new();
    let legacy = json!({
        "schema": "kiana.message.v0",
        "id": legacy_id,
        "kind": "chat",
        "sender": "planner",
        "recipient": "builder",
        "text": "legacy body",
        "created_at_unix_ms": 1,
        "message_digest": ""
    });
    let message = upcast_message(legacy).unwrap();
    assert_eq!(message.schema, MESSAGE_SCHEMA);
    assert_eq!(message.message_id, legacy_id);
    assert_eq!(message.body, "legacy body");
    assert_eq!(
        upcast_message(json!({
            "schema": "kiana.message.v9",
            "message_id": MessageId::new(),
            "kind": "chat",
            "sender_id": "planner",
            "recipient_id": "builder",
            "body": "future"
        }))
        .unwrap_err(),
        "message_schema_unknown_major"
    );
    let mut unknown = json!({
        "schema": "kiana.message.v0",
        "id": MessageId::new(),
        "sender": "planner",
        "recipient": "builder",
        "text": "legacy",
        "unexpected": true
    });
    unknown["message_digest"] = json!("");
    assert_eq!(
        upcast_message(unknown).unwrap_err(),
        "message_upcast_invalid"
    );

    let a = json!({"z": 1, "a": 2});
    let b = json!({"a": 2, "z": 1});
    assert_eq!(json_digest(&a), json_digest(&b));
}
