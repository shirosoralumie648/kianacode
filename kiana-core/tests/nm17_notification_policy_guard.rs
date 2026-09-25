#[test]
fn notification_policy_is_pure_and_does_not_create_delivery_authority() {
    let source = include_str!("../src/notification_policy.rs");
    let domain = include_str!("../../kiana-domain/src/notification_policy.rs");
    for marker in [
        "plan_notification_policy",
        "NotificationPolicyConfig",
        "quiet_hours",
        "SameDayReminder",
        "NotificationPolicyRoute::Digest",
        "NotificationPolicyRoute::Suppressed",
        "query_original",
        "NotificationEscalationFact",
        "deadline_or_attention_required",
    ] {
        assert!(
            source.contains(marker) || domain.contains(marker),
            "NM-17 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "NotificationDeliveryWorker",
        "tokio::spawn",
        "reqwest::Client",
        "std::process::Command",
        "send(",
        "approve(",
        "retry(",
        "close(",
    ] {
        assert!(
            !source.contains(forbidden),
            "NM-17 policy authority widened: {forbidden}"
        );
        assert!(
            !domain.contains(forbidden),
            "NM-17 domain authority widened: {forbidden}"
        );
    }
}
