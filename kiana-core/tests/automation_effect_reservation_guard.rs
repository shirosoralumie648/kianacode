#[test]
fn automation_reservation_is_fenced_before_effects() {
    let domain = include_str!("../../kiana-domain/src/automation_effect_reservation.rs");
    let core = include_str!("../src/automation_effect_reservation.rs");
    for marker in [
        "AutomationEffectReservation",
        "AutomationReservationLedger",
        "authority_epoch",
        "config_revision",
        "policy_revision",
        "approval_digest",
        "idempotency_key",
        "reserve_automation_effect",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-14 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "dispatch_effect",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-14 reservation boundary must not execute effects: {forbidden}"
        );
    }
}
