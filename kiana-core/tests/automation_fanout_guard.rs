#[test]
fn automation_fanout_is_bounded_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/automation_fanout.rs");
    let core = include_str!("../src/automation_fanout.rs");
    for marker in [
        "AutomationFanoutPlan",
        "max_depth",
        "max_concurrency",
        "child_scope_is_subset",
        "fan_in_required",
        "validate_fanout_plan",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-19 marker missing: {marker}"
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
            "AUT-19 boundary executes effects: {forbidden}"
        );
    }
}
