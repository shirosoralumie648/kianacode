use kiana_domain::*;

#[test]
fn known_checkpoint_with_complete_identity_is_resume_compatible() {
    let decision = LegacyCheckpointDecision::assess(
        "kiana.harness-checkpoint.v1",
        1,
        Some(RunId::new()),
        Some(TurnId::new()),
    )
    .unwrap();
    assert_eq!(decision.disposition, LegacyDisposition::Compatible);
    assert!(decision.resume_allowed);
    decision.validate().unwrap();
}

#[test]
fn unknown_checkpoint_version_is_not_resumed() {
    let decision = LegacyCheckpointDecision::assess(
        "kiana.harness-checkpoint.v1",
        99,
        Some(RunId::new()),
        Some(TurnId::new()),
    )
    .unwrap();
    assert_eq!(decision.disposition, LegacyDisposition::NotSupported);
    assert!(!decision.resume_allowed);
    assert_eq!(decision.reason, "unknown_checkpoint_version");

    let missing =
        LegacyCheckpointDecision::assess("kiana.harness-checkpoint.v1", 1, None, None).unwrap();
    assert_eq!(missing.disposition, LegacyDisposition::MigrateReadOnly);
    assert!(!missing.resume_allowed);
}

#[test]
fn legacy_continue_is_additive_and_preserves_one_daemon_route() {
    let decision = LegacyCompatibilityDecision::new(
        LegacyWireKind::ContinueV1,
        "kiana.protocol.v1",
        "kiana.protocol.v1",
        LegacyDisposition::Compatible,
        true,
        true,
        "legacy_continue_adapter",
    )
    .unwrap();
    decision.validate().unwrap();
    assert!(decision.preserves_server_ids);
    assert!(decision.same_daemon_host);
}
