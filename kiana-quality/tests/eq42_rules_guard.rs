use serde_json::Value;

#[test]
fn blocking_rules_stay_precedence_first_and_pure() {
    let source = include_str!("../src/rules.rs");
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
        "append_event",
        "authorize",
        "promote",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden blocking evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "BLOCKING_RULE_INPUT_SCHEMA",
        "BLOCKING_RULE_EVALUATOR_ID",
        "BLOCKING_RULE_EVIDENCE_SCHEMA",
        "BLOCKING_RULE_DECISION_INPUT_SCHEMA",
        "BlockingRuleEvaluator",
        "BlockingRuleDecisionInput",
        "BLOCKING_RULE_KNOWN_RULES",
        "MAX_BLOCKING_RULE_EVIDENCE",
        "REQUIRED_RULES",
        "build_gate_blocking_decision",
        "verify_gate_blocking_decision",
        "declared_rules",
        "evidence_matrix",
        "resolve_verdict",
        "blocking_findings",
        "quality_gate_decision_binding",
        "QualityGateConfig",
        "QualityGateDecision",
        "GateVerdict",
        "blocking.rule_precedes_score",
        "blocking.rule_missing",
        "blocking.finding_ref_missing",
        "blocking.score_threshold_invalid",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-42 boundary marker: {required}"
        );
    }
}

#[test]
fn blocking_rules_guard_covers_the_refusal_matrix_and_lane_wiring() {
    let source = include_str!("../src/rules.rs");
    for refusal in [
        "blocking_rule_config_invalid",
        "blocking_rule_config_rule_unknown",
        "blocking_rule_config_matrix_incomplete",
        "blocking_rule_missing",
        "blocking_rule_unknown",
        "blocking_rule_duplicate",
        "blocking_rule_config_drift",
        "blocking_rule_evidence_invalid",
        "blocking_rule_input_invalid",
        "blocking_rule_finding_ref_missing",
        "blocking_rule_finding_ref_invalid",
        "blocking_rule_score_out_of_bounds",
        "blocking_rule_triggered_forces_reject",
        "blocking_rule_score_below_threshold",
        "blocking_rule_verdict_forged",
        "blocking_rule_decision_invalid",
        "blocking_rule_decision_unbound",
        "blocking_rule_decision_forged",
    ] {
        assert!(
            source.contains(refusal),
            "missing EQ-42 refusal code: {refusal}"
        );
    }

    // Every refusal code is a stable `pub const`.
    for refusal in [
        "BLOCKING_RULE_CONFIG_INVALID",
        "BLOCKING_RULE_CONFIG_RULE_UNKNOWN",
        "BLOCKING_RULE_CONFIG_MATRIX_INCOMPLETE",
        "BLOCKING_RULE_MISSING",
        "BLOCKING_RULE_UNKNOWN",
        "BLOCKING_RULE_DUPLICATE",
        "BLOCKING_RULE_CONFIG_DRIFT",
        "BLOCKING_RULE_EVIDENCE_INVALID",
        "BLOCKING_RULE_INPUT_INVALID",
        "BLOCKING_RULE_FINDING_REF_MISSING",
        "BLOCKING_RULE_FINDING_REF_INVALID",
        "BLOCKING_RULE_SCORE_OUT_OF_BOUNDS",
        "BLOCKING_RULE_TRIGGERED_FORCES_REJECT",
        "BLOCKING_RULE_BELOW_THRESHOLD",
        "BLOCKING_RULE_VERDICT_FORGED",
        "BLOCKING_RULE_DECISION_INVALID",
        "BLOCKING_RULE_DECISION_UNBOUND",
        "BLOCKING_RULE_DECISION_FORGED",
    ] {
        assert!(
            source.contains(&format!("pub const {refusal}: &str =")),
            "EQ-42 refusal code must stay a public constant: {refusal}"
        );
    }

    let fixtures = include_str!("./eq42_rules.rs");
    for fixture in [
        "fn high_score_cannot_override_a_triggered_safety_rule",
        "fn below_threshold_score_cannot_be_a_pass_decision",
        "fn missing_rule_evidence_fails_closed",
        "fn unknown_rule_evidence_fails_closed",
        "fn duplicate_rule_evidence_fails_closed",
        "fn config_declaring_a_narrower_matrix_fails_closed",
        "fn evidence_for_an_undeclared_rule_is_config_drift",
        "fn config_declaring_an_unknown_rule_fails_closed",
        "fn triggered_rule_without_finding_refs_fails_closed",
        "fn malformed_headers_scores_and_schemas_fail_closed",
        "fn forged_decision_and_unbound_version_are_refused",
        "fn triggered_rule_forces_reject_or_rollback_never_pass",
        "fn clean_matrix_below_threshold_cannot_be_reported_as_pass",
        "fn clean_untriggered_evidence_with_passing_score_builds_a_bound_pass",
        "fn standalone_rule_evaluator_keeps_its_first_slice_matrix",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-42 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-42 fixtures must not disable a test: {disabled}"
        );
    }

    // Deny-first: every refusal fixture precedes the single success fixture.
    let success = fixtures
        .find("fn clean_untriggered_evidence_with_passing_score_builds_a_bound_pass")
        .expect("success fixture");
    for deny in [
        "fn high_score_cannot_override_a_triggered_safety_rule",
        "fn below_threshold_score_cannot_be_a_pass_decision",
        "fn missing_rule_evidence_fails_closed",
        "fn unknown_rule_evidence_fails_closed",
        "fn duplicate_rule_evidence_fails_closed",
        "fn config_declaring_a_narrower_matrix_fails_closed",
        "fn evidence_for_an_undeclared_rule_is_config_drift",
        "fn config_declaring_an_unknown_rule_fails_closed",
        "fn triggered_rule_without_finding_refs_fails_closed",
        "fn malformed_headers_scores_and_schemas_fail_closed",
        "fn forged_decision_and_unbound_version_are_refused",
    ] {
        let position = fixtures.find(deny).expect("deny fixture");
        assert!(
            position < success,
            "EQ-42 deny fixture must precede the success fixture: {deny}"
        );
    }

    // The manual lane stays manual-only, read-only, fetches locked deps, gates formatting, runs
    // both targets serially and compiles the whole crate's test targets.
    let lane = include_str!("../../.github/workflows/eq42-blocking-rules.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-42 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq42_rules --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq42_rules_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-42 lane step missing: {step}");
    }
    // The lane must not adopt the two pre-existing `eq41_gate` failures into this card's gate.
    assert!(
        !lane.contains("--test eq41_gate"),
        "EQ-42 lane must not run the unfixed EQ-41 regression as a green step"
    );

    // `targets: null` keeps both targets in the automatic kiana-quality shard.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
