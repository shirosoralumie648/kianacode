#[test]
fn recovery_replay_evaluator_stays_pure_and_separates_unknown() {
    let source = include_str!("../src/recovery.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
        "std::process",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden recovery evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "RECOVERY_REPLAY_INPUT_SCHEMA",
        "RecoveryReplayEvaluator",
        "replay.divergence",
        "replay.result_unknown",
        "replay.logic_version_drift",
        "replay.fence_invalid",
        "replay.resume_missing",
        "replay.unknown_retry_forbidden",
        "replay.reconcile_missing",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-31 boundary marker: {required}"
        );
    }
}
