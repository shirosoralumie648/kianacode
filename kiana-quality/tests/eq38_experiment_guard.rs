#[test]
fn experiment_replay_stays_pure_and_indexed() {
    let source = include_str!("../src/experiment.rs");
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
        "promote",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden experiment replay dependency: {forbidden}"
        );
    }
    for required in [
        "EXPERIMENT_REPLAY_INPUT_SCHEMA",
        "ExperimentReplayEvaluator",
        "ExperimentStatus",
        "case_results",
        "experiment_event_sequence_invalid",
        "experiment_case_result_conflict",
        "experiment_event_after_terminal",
        "experiment_terminal_missing",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-38 boundary marker: {required}"
        );
    }
}
