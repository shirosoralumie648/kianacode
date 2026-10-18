use serde_json::Value;

#[test]
fn experiment_replay_stays_pure_and_indexed() {
    let source = include_str!("../src/experiment.rs");
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "ControlPlane",
        "Command::new",
        "CostLedger",
        "promote",
        "authorize",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden experiment replay dependency: {forbidden}"
        );
    }
    for required in [
        "EXPERIMENT_REPLAY_INPUT_SCHEMA",
        "EXPERIMENT_REPLAY_EVALUATOR_ID",
        "EXPERIMENT_EVENT_SCHEMA",
        "EXPERIMENT_SCHEMA",
        "MAX_EXPERIMENT_EVENTS",
        "MAX_EXPERIMENT_TEXT_BYTES",
        "ExperimentReplayEvaluator",
        "ExperimentReplayInput",
        "ExperimentReplayError",
        "ExperimentEvent",
        "ExperimentEventKind::Admitted",
        "ExperimentEventKind::Started",
        "ExperimentEventKind::CaseCompleted",
        "ExperimentEventKind::Completed",
        "ExperimentEventKind::Failed",
        "ExperimentEventKind::Cancelled",
        "EvalResultIndex",
        "EvalExperiment",
        "ExperimentStatus",
        "ExperimentStatus::Admitted",
        "ExperimentStatus::Running",
        "ExperimentStatus::Completed",
        "ExperimentStatus::Failed",
        "ExperimentStatus::Cancelled",
        "replay_experiment",
        "case_results",
        "source_sequence",
        "terminal_sequence",
        "replay_digest",
        "json_digest",
        "Finding::redacted",
        "MAX_FINDINGS",
        "experiment:input",
        "experiment_replay_schema_invalid",
        "experiment_replay_identity_invalid",
        "experiment_event_limit_exceeded",
        "experiment_event_schema_invalid",
        "experiment_event_sequence_invalid",
        "experiment_admission_missing",
        "experiment_transition_invalid",
        "experiment_case_result_invalid",
        "experiment_case_result_conflict",
        "experiment_case_result_repeated",
        "experiment_event_after_terminal",
        "experiment_terminal_missing",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-38 boundary marker: {required}"
        );
    }
}

#[test]
fn eq38_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq38_experiment.rs");
    for fixture in [
        "fn experiment_state_is_replayable_from_events",
        "fn sequence_gaps_repeats_and_reordering_are_refused",
        "fn admission_must_be_the_first_event_and_appear_once",
        "fn illegal_transitions_are_refused",
        "fn case_results_require_bounded_identity_and_strict_digest",
        "fn duplicated_and_drifted_case_results_are_refused",
        "fn events_after_a_terminal_state_are_refused",
        "fn a_log_without_a_terminal_state_is_refused",
        "fn every_terminal_state_reduces_deterministically",
        "fn replay_digest_binds_identity_events_and_order",
        "fn envelope_schema_identity_and_event_limit_fail_closed",
        "fn event_schema_drift_and_unknown_fields_are_refused",
        "fn evaluator_findings_are_deterministic_and_registry_wired",
        "fn typed_constructors_round_trip_and_the_index_keeps_source_sequences",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-38 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-38 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq38-experiment.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-38 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq38_experiment --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq38_experiment_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq38_experiment --no-run --locked",
        "cargo test -p kiana-quality --test eq38_experiment_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-38 lane step missing: {step}");
    }
    assert!(!lane.contains("|| true"), "EQ-38 lane must fail closed");

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
