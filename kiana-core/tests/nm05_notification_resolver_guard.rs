#[test]
fn notification_resolver_requires_server_context_and_never_dispatches() {
    let source = include_str!("../src/notification_resolver.rs");
    for marker in [
        "resolve_notification_subscriptions",
        "context.project_trusted",
        "context.actor_id",
        "server_project_id",
        "validate_for_subscription",
        "notification_recipient_subscription_not_found",
    ] {
        assert!(source.contains(marker), "resolver marker missing: {marker}");
    }
    for forbidden in [
        "CapabilityBroker",
        "KianaHarness",
        "reqwest",
        "tokio::spawn",
        "owner_id",
    ] {
        assert!(
            !source.contains(forbidden),
            "resolver authority widened: {forbidden}"
        );
    }
}
