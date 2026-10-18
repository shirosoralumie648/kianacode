use serde_json::Value;

#[test]
fn quality_gate_keeps_config_and_decision_separate_and_pure() {
    let source = include_str!("../src/gate.rs");
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "std::time",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "EventLog",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "ControlPlane",
        "Command::new",
        "CostLedger",
        "append_event",
        "promote",
        "authorize",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden quality gate dependency: {forbidden}"
        );
    }
    for required in [
        "GATE_CONFIG_SCHEMA",
        "GATE_DECISION_SCHEMA",
        "GATE_CONFIG_HISTORY_SCHEMA",
        "GATE_DECISION_LOG_SCHEMA",
        "GATE_INPUT_SCHEMA",
        "GATE_LOG_INPUT_SCHEMA",
        "GATE_EVALUATOR_ID",
        "GATE_LOG_EVALUATOR_ID",
        "MAX_GATE_TEXT_BYTES",
        "MAX_GATE_THRESHOLDS",
        "MAX_GATE_BLOCKING_RULES",
        "MAX_GATE_EVIDENCE_REFS",
        "MAX_GATE_CONFIG_VERSIONS",
        "MAX_GATE_DECISIONS",
        "MAX_GATE_SCORE_MILLI",
        "GATE_THRESHOLD_DIMENSIONS",
        "GATE_BLOCKING_RULES",
        "GateVerdict",
        "QualityGateConfig",
        "QualityGateDecision",
        "GateConfigHistory",
        "QualityGateDecisionLog",
        "GateInput",
        "GateLogInput",
        "GateEvaluator",
        "GateLogEvaluator",
        "evaluate_gate",
        "evaluate_gate_log",
        "quality_gate_config_update",
        "quality_gate_decision_binding",
        "thresholds_digest",
        "config_digest",
        "decision_digest",
        "history_digest",
        "log_digest",
        "gate_version",
        "source_cursor",
        "known_regressions",
        "blocking_findings",
        "deny_unknown_fields",
        "json_digest",
        "Finding::new",
        "MAX_FINDINGS",
        "BTreeMap",
        "BTreeSet",
        "valid_text",
        "valid_digest",
        "bounded_sorted_texts",
        "gate:input",
        "gate:log",
        "gate_config_header_invalid",
        "gate_config_threshold_unknown",
        "gate_config_blocking_rule_unknown",
        "gate_config_digest_mismatch",
        "gate_config_update_current_invalid",
        "gate_config_update_identity_mismatch",
        "gate_config_update_version_not_incremented",
        "gate_config_history_header_invalid",
        "gate_config_history_entry_invalid",
        "gate_config_history_identity_mismatch",
        "gate_config_history_digest_mismatch",
        "gate_config_version_rewritten",
        "gate_config_version_regressed",
        "gate_config_version_gap",
        "gate_input_schema_invalid",
        "gate_log_input_schema_invalid",
        "gate_decision_header_invalid",
        "gate_decision_verdict_inconsistent",
        "gate_decision_digest_mismatch",
        "gate_decision_config_identity_mismatch",
        "gate_decision_gate_version_mismatch",
        "gate_decision_config_digest_mismatch",
        "gate_decision_suite_digest_mismatch",
        "gate_decision_log_header_invalid",
        "gate_decision_log_entry_invalid",
        "gate_decision_log_identity_mismatch",
        "gate_decision_log_version_regressed",
        "gate_decision_log_cursor_regressed",
        "gate_decision_log_time_regressed",
        "gate_decision_log_digest_mismatch",
        "gate_decision_candidate_redecided",
        "gate_decision_rewritten",
        "gate.config_invalid",
        "gate.decision_invalid",
        "gate.decision_config_mismatch",
        "gate.pass_with_blocking_findings",
        "gate.updated_config_invalid",
        "gate.config_update_invalid",
        "gate.config_history_invalid",
        "gate.log_invalid",
        "gate.decision_rewritten",
        "gate.candidate_redecided",
        "gate.config_version_regressed",
        "gate.log_order_invalid",
        "gate.log_identity_mismatch",
        "gate.log_entry_config_mismatch",
        "gate.log_entry_version_ahead",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-41 boundary marker: {required}"
        );
    }
}

#[test]
fn eq41_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq41_gate.rs");
    for fixture in [
        "fn gate_config_update_does_not_mutate_old_decision",
        "fn gate_config_and_decision_are_separate_value_contracts",
        "fn gate_config_rejects_unknown_thresholds_and_blocking_rules",
        "fn gate_config_digest_tamper_is_refused",
        "fn gate_decision_verdict_matrix_is_enforced",
        "fn gate_decision_digest_and_header_tamper_is_refused",
        "fn gate_decision_must_bind_the_exact_config_version",
        "fn gate_update_requires_a_newer_distinct_version",
        "fn gate_decision_log_is_append_only_and_replay_safe",
        "fn gate_decision_log_refuses_a_rewritten_decision",
        "fn gate_decision_log_refuses_redeciding_a_candidate_under_the_same_config",
        "fn gate_decision_log_refuses_a_config_version_regression",
        "fn gate_log_entries_must_bind_the_published_config_history",
        "fn gate_contracts_round_trip_and_are_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-41 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-41 fixtures must not disable a test: {disabled}"
        );
    }

    // Deny-first: the card acceptance fixture precedes every success fixture.
    let acceptance = fixtures
        .find("fn gate_config_update_does_not_mutate_old_decision")
        .expect("acceptance fixture");
    let round_trip = fixtures
        .find("fn gate_contracts_round_trip_and_are_registry_wired")
        .expect("round-trip fixture");
    assert!(
        acceptance < round_trip,
        "EQ-41 fixtures must keep the deny paths before the success path"
    );

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq41-gate.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-41 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq41_gate --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq41_gate_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-41 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
