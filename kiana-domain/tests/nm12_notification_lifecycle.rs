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
