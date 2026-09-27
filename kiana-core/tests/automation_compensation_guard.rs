#[test]
fn compensation_is_fresh_authorization_only() {
    let domain = include_str!("../../kiana-domain/src/automation_compensation.rs");
    let core = include_str!("../src/automation_compensation.rs");
    for marker in [
        "AutomationCompensationPlan",
        "original_execution_id",
        "compensation_action_digest",
        "fresh_authorization_digest",
        "reused_original_permit",
        "validate_compensation_plan",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-20 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-20 boundary executes effects: {forbidden}"
        );
    }
}
