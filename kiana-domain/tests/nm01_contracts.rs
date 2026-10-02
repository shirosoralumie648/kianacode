use kiana_domain::*;
use serde::de::DeserializeOwned;
use serde::Serialize;
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

fn notification() -> Notification {
    Notification::new(
        message().message_id,
        "builder",
        None,
        vec!["project/a/file".to_owned()],
        NotificationChannel::InApp,
        100,
        200,
        1,
        None,
    )
    .unwrap()
}

fn delivery_attempt() -> DeliveryAttempt {
    DeliveryAttempt::new(
        notification().notification_id,
        subscription().subscription_id,
        1,
        100,
    )
    .unwrap()
}

fn delivery_receipt() -> DeliveryReceipt {
    DeliveryReceipt::new(
        notification().notification_id,
        delivery_attempt().delivery_attempt_id,
        "builder",
        DeliveryReceiptStatus::Acknowledged,
        101,
        None,
    )
    .unwrap()
}

fn action_ref() -> ActionRef {
    ActionRef::new("company.review", 7, vec!["project/a".to_owned()], 100, 200).unwrap()
}

fn assert_json_field_order_and_round_trip<T>(value: &T, fields: &[&str])
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let encoded = serde_json::to_string(value).unwrap();
    let mut previous = 0;
    for field in fields {
        let position = encoded.find(&format!("\"{field}\":")).unwrap();
        assert!(position >= previous, "field order changed at {field}");
        previous = position;
    }
    let decoded: T = serde_json::from_str(&encoded).unwrap();
    assert_eq!(&decoded, value);
}

fn assert_secret_rejected_at_wire_boundaries<T>(
    value: &T,
    payload: serde_json::Value,
    sentinel: &str,
    expected_error: &str,
) where
    T: Serialize + DeserializeOwned + std::fmt::Debug,
{
    assert!(!format!("{value:?}").contains(sentinel));

    let serialization_error = serde_json::to_vec(value).unwrap_err().to_string();
    assert!(serialization_error.contains(expected_error));
    assert!(!serialization_error.contains(sentinel));

    let deserialization_error = serde_json::from_value::<T>(payload)
        .unwrap_err()
        .to_string();
    assert!(deserialization_error.contains(expected_error));
    assert!(!deserialization_error.contains(sentinel));
}

fn assert_serde_rejects<T>(value: &T, payload: serde_json::Value, expected_error: &str)
where
    T: Serialize + DeserializeOwned,
{
    let serialization_error = serde_json::to_vec(value).unwrap_err().to_string();
    assert!(serialization_error.contains(expected_error));
    let deserialization_error = serde_json::from_value::<T>(payload)
        .unwrap_err()
        .to_string();
    assert!(deserialization_error.contains(expected_error));
}

#[test]
fn notification_dto_serde_preserves_wire_layout_and_option_defaults() {
    let notification = notification();
    assert_json_field_order_and_round_trip(
        &notification,
        &[
            "schema",
            "notification_id",
            "message_id",
            "recipient_id",
            "project_id",
            "scope",
            "channel",
            "status",
            "created_at_unix_ms",
            "expires_at_unix_ms",
            "subscription_revision",
            "action_ref_id",
            "notification_digest",
        ],
    );
    let encoded = serde_json::to_value(&notification).unwrap();
    assert!(encoded["project_id"].is_null());
    assert!(encoded["action_ref_id"].is_null());
    let mut legacy_notification = encoded;
    legacy_notification
        .as_object_mut()
        .unwrap()
        .remove("project_id");
    legacy_notification
        .as_object_mut()
        .unwrap()
        .remove("action_ref_id");
    assert_eq!(
        serde_json::from_value::<Notification>(legacy_notification).unwrap(),
        notification
    );

    let subscription = subscription();
    assert_json_field_order_and_round_trip(
        &subscription,
        &[
            "schema",
            "subscription_id",
            "recipient_id",
            "project_id",
            "scope",
            "channels",
            "revision",
            "status",
            "created_at_unix_ms",
            "expires_at_unix_ms",
            "subscription_digest",
        ],
    );
    let mut encoded = serde_json::to_value(&subscription).unwrap();
    assert!(encoded["project_id"].is_null());
    encoded.as_object_mut().unwrap().remove("project_id");
    assert_eq!(
        serde_json::from_value::<Subscription>(encoded).unwrap(),
        subscription
    );

    let attempt = delivery_attempt();
    assert_json_field_order_and_round_trip(
        &attempt,
        &[
            "schema",
            "delivery_attempt_id",
            "notification_id",
            "subscription_id",
            "attempt_number",
            "status",
            "authority_epoch",
            "lease_expires_at_unix_ms",
            "created_at_unix_ms",
            "updated_at_unix_ms",
            "attempt_digest",
        ],
    );
    let mut encoded = serde_json::to_value(&attempt).unwrap();
    assert!(encoded["lease_expires_at_unix_ms"].is_null());
    encoded
        .as_object_mut()
        .unwrap()
        .remove("lease_expires_at_unix_ms");
    assert_eq!(
        serde_json::from_value::<DeliveryAttempt>(encoded).unwrap(),
        attempt
    );

    let receipt = delivery_receipt();
    assert_json_field_order_and_round_trip(
        &receipt,
        &[
            "schema",
            "delivery_receipt_id",
            "notification_id",
            "delivery_attempt_id",
            "recipient_id",
            "status",
            "observed_at_unix_ms",
            "response_digest",
            "receipt_digest",
        ],
    );
    let mut encoded = serde_json::to_value(&receipt).unwrap();
    assert!(encoded["response_digest"].is_null());
    encoded.as_object_mut().unwrap().remove("response_digest");
    assert_eq!(
        serde_json::from_value::<DeliveryReceipt>(encoded).unwrap(),
        receipt
    );

    assert_json_field_order_and_round_trip(
        &action_ref(),
        &[
            "schema",
            "action_ref_id",
            "command",
            "target_revision",
            "scope",
            "created_at_unix_ms",
            "expires_at_unix_ms",
            "action_digest",
        ],
    );
}

#[test]
fn notification_dtos_validate_and_redact_at_wire_boundaries() {
    let sentinel = "nm01-dto-boundary-sentinel";

    let mut invalid_notification = notification();
    invalid_notification.recipient_id = format!("Authorization: Bearer {sentinel}");
    invalid_notification.notification_digest = invalid_notification.digest();
    let mut notification_payload = serde_json::to_value(notification()).unwrap();
    notification_payload["recipient_id"] = json!(&invalid_notification.recipient_id);
    notification_payload["notification_digest"] = json!(&invalid_notification.notification_digest);
    assert_secret_rejected_at_wire_boundaries(
        &invalid_notification,
        notification_payload,
        sentinel,
        "notification_recipient_secret_detected",
    );

    let mut invalid_subscription = subscription();
    invalid_subscription.recipient_id = format!("Authorization: Bearer {sentinel}");
    invalid_subscription.subscription_digest = invalid_subscription.digest();
    let mut subscription_payload = serde_json::to_value(subscription()).unwrap();
    subscription_payload["recipient_id"] = json!(&invalid_subscription.recipient_id);
    subscription_payload["subscription_digest"] = json!(&invalid_subscription.subscription_digest);
    assert_secret_rejected_at_wire_boundaries(
        &invalid_subscription,
        subscription_payload,
        sentinel,
        "subscription_recipient_secret_detected",
    );

    let mut invalid_attempt = delivery_attempt();
    invalid_attempt.attempt_digest = format!("Authorization: Bearer {sentinel}");
    let mut attempt_payload = serde_json::to_value(delivery_attempt()).unwrap();
    attempt_payload["attempt_digest"] = json!(&invalid_attempt.attempt_digest);
    assert_secret_rejected_at_wire_boundaries(
        &invalid_attempt,
        attempt_payload,
        sentinel,
        "delivery_attempt_digest_invalid",
    );

    let mut invalid_receipt = delivery_receipt();
    invalid_receipt.recipient_id = format!("Authorization: Bearer {sentinel}");
    invalid_receipt.receipt_digest = invalid_receipt.digest();
    let mut receipt_payload = serde_json::to_value(delivery_receipt()).unwrap();
    receipt_payload["recipient_id"] = json!(&invalid_receipt.recipient_id);
    receipt_payload["receipt_digest"] = json!(&invalid_receipt.receipt_digest);
    assert_secret_rejected_at_wire_boundaries(
        &invalid_receipt,
        receipt_payload,
        sentinel,
        "delivery_receipt_recipient_secret_detected",
    );

    let mut invalid_action = action_ref();
    invalid_action.command = format!("Authorization: Bearer {sentinel}");
    invalid_action.action_digest = invalid_action.digest();
    let mut action_payload = serde_json::to_value(action_ref()).unwrap();
    action_payload["command"] = json!(&invalid_action.command);
    action_payload["action_digest"] = json!(&invalid_action.action_digest);
    assert_secret_rejected_at_wire_boundaries(
        &invalid_action,
        action_payload,
        sentinel,
        "action_ref_command_secret_detected",
    );
}

#[test]
fn notification_dtos_reject_unknown_schema_versions_at_wire_boundaries() {
    let mut notification = notification();
    notification.schema = "kiana.notification.v9".to_owned();
    let mut notification_payload = serde_json::to_value(notification()).unwrap();
    notification_payload["schema"] = json!("kiana.notification.v9");
    assert_serde_rejects(
        &notification,
        notification_payload,
        "notification_schema_invalid",
    );

    let mut subscription = subscription();
    subscription.schema = "kiana.notification-subscription.v9".to_owned();
    let mut subscription_payload = serde_json::to_value(subscription()).unwrap();
    subscription_payload["schema"] = json!("kiana.notification-subscription.v9");
    assert_serde_rejects(
        &subscription,
        subscription_payload,
        "subscription_schema_invalid",
    );

    let mut attempt = delivery_attempt();
    attempt.schema = "kiana.notification-delivery-attempt.v9".to_owned();
    let mut attempt_payload = serde_json::to_value(delivery_attempt()).unwrap();
    attempt_payload["schema"] = json!("kiana.notification-delivery-attempt.v9");
    assert_serde_rejects(&attempt, attempt_payload, "delivery_attempt_header_invalid");

    let mut receipt = delivery_receipt();
    receipt.schema = "kiana.notification-delivery-receipt.v9".to_owned();
    let mut receipt_payload = serde_json::to_value(delivery_receipt()).unwrap();
    receipt_payload["schema"] = json!("kiana.notification-delivery-receipt.v9");
    assert_serde_rejects(&receipt, receipt_payload, "delivery_receipt_schema_invalid");

    let mut action = action_ref();
    action.schema = "kiana.notification-action-ref.v9".to_owned();
    let mut action_payload = serde_json::to_value(action_ref()).unwrap();
    action_payload["schema"] = json!("kiana.notification-action-ref.v9");
    assert_serde_rejects(&action, action_payload, "action_ref_header_invalid");
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
