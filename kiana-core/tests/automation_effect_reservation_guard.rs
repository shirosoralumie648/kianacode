#[test]
fn automation_reservation_is_fenced_before_effects() {
    let domain = include_str!("../../kiana-domain/src/automation_effect_reservation.rs");
    let core = include_str!("../src/automation_effect_reservation.rs");
    for marker in [
        "AutomationEffectReservation",
        "AutomationReservationLedger",
        "AutomationExecutionBinding",
        "AutomationEffectPermit",
        "AutomationEffectAttempt",
        "AutomationEffectAttemptBegin",
        "authority_epoch",
        "config_revision",
        "policy_revision",
        "approval_digest",
        "idempotency_key",
        "register_execution",
        "issue_permit",
        "begin_attempt",
        "fetch_attempt",
        "reserve_automation_effect",
        "issue_automation_effect_permit",
        "dispatch_reserved_automation_effect",
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

#[test]
fn effect_dispatch_cannot_precede_permit_and_attempt_checks() {
    let domain = include_str!("../../kiana-domain/src/automation_effect_reservation.rs");
    let core = include_str!("../src/automation_effect_reservation.rs");
    for marker in [
        "automation_effect_reservation_fence_drift",
        "automation_effect_reservation_execution_unknown",
        "automation_effect_reservation_scope_drift",
        "automation_effect_reservation_idempotency_conflict",
        "automation_effect_permit_reservation_unknown",
        "automation_effect_permit_fence_drift",
        "automation_effect_permit_already_consumed",
        "automation_effect_permit_conflict",
        "automation_effect_attempt_conflict",
        "automation_effect_attempt_unknown",
        "automation_effect_attempt_already_started",
        "automation_effect_execution_binding_conflict",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "AUT-14 rejection code missing: {marker}"
        );
    }
    let check = core
        .find("ledger.begin_attempt(")
        .expect("dispatch facade must prepare the attempt through the ledger");
    let call = core
        .find("effect(begin.attempt())")
        .expect("dispatch facade must call the supplied effect");
    assert!(
        check < call,
        "AUT-14 effect must be reached only after begin_attempt succeeds"
    );
    assert!(
        core.contains("if !begin.started()")
            && core.contains("automation_effect_attempt_already_started"),
        "AUT-14 replays must not re-execute the effect"
    );
    for forbidden in [
        "kiana_capability_broker",
        "KianaHarness",
        "CapabilityHandler",
        "Broker dispatch",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "AUT-14 facade must stay outside the broker/handler boundary: {forbidden}"
        );
    }
}
