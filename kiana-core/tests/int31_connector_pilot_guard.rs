#[test]
fn connector_pilot_gate_is_default_off_and_effect_free() {
    let domain = include_str!("../../kiana-domain/src/connector_pilot.rs");
    let core = include_str!("../src/connector_pilot.rs");
    for marker in [
        "DefaultOff",
        "OptedIn",
        "LiveNetwork",
        "operator_approval_ref",
        "provider_receipt_digest",
        "cleanup_plan_digest",
        "revocation_epoch",
        "validate_connector_pilot_gate",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "INT-31 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "reqwest",
        "invoke_external",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "INT-31 pilot gate executes effect: {forbidden}"
        );
    }
}
