#[test]
fn swarm_recovery_keeps_unknown_and_re_admission_explicit() {
    let domain = include_str!("../../kiana-domain/src/swarm_recovery.rs");
    let core = include_str!("../src/swarm_recovery.rs");
    for marker in [
        "SwarmRecoveryFact",
        "PendingWrite",
        "ReAdmissionRequired",
        "ResultUnknown",
        "last_committed_cursor",
        "effect_known",
        "re_admission_authorized",
        "validate_child_recovery",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "SW-11 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
        "auto_success",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "SW-11 recovery boundary must not execute effects: {forbidden}"
        );
    }
}
