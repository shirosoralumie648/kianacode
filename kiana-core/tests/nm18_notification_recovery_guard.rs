#[test]
fn notification_recovery_has_no_connector_or_automatic_retry_authority() {
    let core = include_str!("../src/notification_recovery.rs");
    let domain = include_str!("../../kiana-domain/src/notification_recovery.rs");
    for marker in [
        "plan_notification_recovery",
        "RetryRequiresReAdmission",
        "AwaitReconciliation",
        "TerminalNoSend",
        "delivery_started",
        "new_authority_and_delivery_key_required",
        "cancel_requested_before_send",
        "delivery_outcome_unknown",
    ] {
        assert!(
            core.contains(marker) || domain.contains(marker),
            "NM-18 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "Connector",
        "reqwest::Client",
        "tokio::spawn",
        "std::process::Command",
        "send(",
        "retry(",
    ] {
        assert!(
            !core.contains(forbidden),
            "NM-18 core authority widened: {forbidden}"
        );
        assert!(
            !domain.contains(forbidden),
            "NM-18 domain authority widened: {forbidden}"
        );
    }
}
