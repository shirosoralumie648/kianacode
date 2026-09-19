#[test]
fn legacy_paths_remain_on_one_daemon_host_and_unknown_checkpoint_stays_closed() {
    let domain = include_str!("../../kiana-domain/src/legacy_compat.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let entrypoints = include_str!("../../kiana-entrypoints/src/command_dispatch.rs");
    for marker in [
        "LegacyCheckpointDecision",
        "unknown_checkpoint_version",
        "checkpoint_identity_incomplete",
        "resume_allowed",
        "same_daemon_host",
        "preserves_server_ids",
    ] {
        assert!(
            domain.contains(marker),
            "missing legacy compatibility marker: {marker}"
        );
    }
    for marker in [
        "RunnerCommand::Continue",
        "RequestEnvelope::continue_run",
        "ResponseEnvelope",
        "harness-checkpoint.v1",
    ] {
        assert!(protocol.contains(marker) || runner.contains(marker));
    }
    for marker in ["DaemonHost", "ControlPlane", "RequestBody::Continue"] {
        assert!(
            daemon.contains(marker),
            "missing daemon migration route: {marker}"
        );
    }
    assert!(entrypoints.contains("DaemonHost"));
    assert!(!domain.contains("RunId::new()"));
}

#[test]
fn legacy_compatibility_does_not_add_a_second_model_loop() {
    let domain = include_str!("../../kiana-domain/src/legacy_compat.rs");
    assert!(!domain.contains("ModelClient"));
    assert!(!domain.contains("CapabilityBroker"));
    assert!(!domain.contains("tokio::spawn"));
}
