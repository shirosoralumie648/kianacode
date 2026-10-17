use kiana_domain::{
    EventId, MessageId, NotificationId, NotificationLifecycleFact, NotificationLifecycleKind,
};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[test]
fn fixture_declares_append_only_lifecycle_contract() {
    let value: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/nm12-notification-lifecycle.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "kiana.notification-lifecycle-fixture.v1");
    assert!(value["denied"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("delete")));
}

#[test]
fn lifecycle_fact_requires_epoch_cursor_reason_and_valid_supersede_target() {
    let notification = NotificationId::new();
    let replacement = NotificationId::new();
    let fact = NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        4,
        2,
        NotificationLifecycleKind::Supersede,
        Some(replacement),
        "retention policy superseded",
    )
    .unwrap();
    fact.validate().unwrap();

    assert!(NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        4,
        2,
        NotificationLifecycleKind::Supersede,
        None,
        "missing replacement",
    )
    .is_err());
    assert!(NotificationLifecycleFact::new(
        notification,
        EventId::new(),
        0,
        2,
        NotificationLifecycleKind::Withdraw,
        None,
        "invalid cursor",
    )
    .is_err());
    let _ = MessageId::new();
}

#[test]
fn lifecycle_fact_decoding_refuses_smuggled_payload_or_approval_metadata() {
    let fact = NotificationLifecycleFact::new(
        NotificationId::new(),
        EventId::new(),
        1,
        1,
        NotificationLifecycleKind::Withdraw,
        None,
        "withdrawn after delivery",
    )
    .unwrap();
    let encoded = serde_json::to_value(&fact).unwrap();
    let decoded: NotificationLifecycleFact = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, fact);

    // A payload reference is not a lifecycle field: it fails to decode instead of being ignored.
    let mut smuggled = encoded.clone();
    smuggled["payload_refs"] = serde_json::json!(["notification:payload"]);
    assert!(serde_json::from_value::<NotificationLifecycleFact>(smuggled).is_err());

    // Neither is an approval/decision field; a lifecycle fact can never carry an approval.
    let mut smuggled = encoded.clone();
    smuggled["approved"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NotificationLifecycleFact>(smuggled).is_err());

    let mut smuggled = encoded.clone();
    smuggled["decision"] = serde_json::json!({"action": "approve"});
    assert!(serde_json::from_value::<NotificationLifecycleFact>(smuggled).is_err());

    // Rewriting a fact without re-signing it is refused by the digest self-check.
    let mut tampered = fact;
    tampered.reason = "a different reason".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "notification_lifecycle_digest_mismatch"
    );
}
