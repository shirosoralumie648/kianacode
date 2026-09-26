#[test]
fn cp29_authority_matrix_is_a_validator_not_a_second_execution_or_fake_crash_path() {
    let domain = include_str!("../../kiana-domain/src/control_plane_authority.rs");
    let core = include_str!("../src/control_plane_authority.rs");
    let journal = include_str!("../../kiana-domain/src/journal.rs");
    let eventlog = include_str!("../../kiana-eventlog/src/journal_core.rs");
    for marker in [
        "Cp29AuthorityScenario",
        "Cp29CommandFact",
        "Cp29CrashObservation",
        "Cp29CrashPoint",
        "cp29_terminal_cannot_revive",
        "cp29_command_idempotency_or_payload_drift",
        "cp29_online_replay_digest_mismatch",
        "cp29_crash_started_effect_requires_unknown_fence",
        "TransitionBatch",
        "JournalState",
        "validate_control_plane_authority_scenario",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || journal.contains(marker)
                || eventlog.contains(marker),
            "CP-29 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::process::Command",
        "tokio::process::Command",
        "CapabilityBroker::new",
        "ModelClient::new",
        "kill -9",
        "side_effects = true",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "CP-29 bypass/fake runtime marker present: {forbidden}"
        );
    }
}
