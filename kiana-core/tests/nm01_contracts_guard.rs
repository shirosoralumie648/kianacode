#[test]
fn notification_contracts_are_domain_owned_and_do_not_create_a_delivery_loop() {
    let domain = include_str!("../../kiana-domain/src/notifications.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let core = include_str!("../src/communication.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "pub struct Message",
        "pub struct Notification",
        "pub struct Subscription",
        "pub struct DeliveryAttempt",
        "pub struct ActionRef",
        "pub struct DeliveryReceipt",
        "deny_unknown_fields",
        "upcast_message",
        "canonical_notification_bytes",
        "notification_scope_exceeds_subscription",
        "message_body_secret_detected",
    ] {
        assert!(
            domain.contains(marker),
            "notification marker missing: {marker}"
        );
    }
    for marker in [
        "kiana.message.v1",
        "kiana.notification.v1",
        "kiana.notification-subscription.v1",
        "kiana.notification-delivery-attempt.v1",
        "kiana.notification-action-ref.v1",
        "kiana.notification-delivery-receipt.v1",
    ] {
        assert!(
            contracts.contains(marker),
            "schema marker missing: {marker}"
        );
    }
    assert!(core.contains("CommunicationMessage"));
    assert!(ports.contains("CommunicationPort"));
    assert!(!domain.contains("DeliveryWorker"));
}
