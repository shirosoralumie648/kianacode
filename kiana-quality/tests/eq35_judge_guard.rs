use serde_json::Value;

#[test]
fn semantic_judge_evaluator_stays_optional_and_fail_closed() {
    let source = include_str!("../src/judge.rs");
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
            "forbidden semantic judge dependency: {forbidden}"
        );
    }
    for required in [
        "SEMANTIC_JUDGE_INPUT_SCHEMA",
        "SEMANTIC_JUDGE_EVALUATOR_ID",
        "SemanticJudgeEvaluator",
        "SemanticJudgeInput",
        "SemanticJudgeConfig",
        "SemanticJudgeRequest",
        "SemanticJudgeResult",
        "JudgeAvailability::Available",
        "JudgeAvailability::Unavailable",
        "JudgeAvailability::NotConfigured",
        "JudgeVerdict::Pass",
        "JudgeVerdict::Fail",
        "JudgeVerdict::NeedsReview",
        "MAX_JUDGE_TEXT_BYTES",
        "MAX_JUDGE_REASON_BYTES",
        "MAX_JUDGE_TEMPERATURE_MILLI",
        "MAX_JUDGE_SCORE_MILLI",
        "semantic_judge_input_schema_invalid",
        "semantic_judge_reason_limit_exceeded",
        "judge.evidence_missing",
        "judge.unavailable",
        "judge.not_configured",
        "judge.not_evaluated",
        "judge.unavailable_not_pass",
        "judge.evidence_unexpected",
        "judge.unavailable_reason_missing",
        "judge.unavailable_reason_unexpected",
        "judge.config_schema_invalid",
        "judge.config_invalid",
        "judge.config_drift",
        "judge.prompt_drift",
        "judge.request_schema_invalid",
        "judge.request_invalid",
        "judge.result_schema_invalid",
        "judge.result_invalid",
        "judge.result_inconsistent",
        "judge.verdict_failed",
        "judge.verdict_needs_review",
        "judge:availability",
        "judge:config",
        "judge:request",
        "judge:result",
        "is_bounded_identity",
        "is_sha256_digest",
        "redact_text",
        "Finding::redacted",
        "sha256:",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-35 boundary marker: {required}"
        );
    }
}

#[test]
fn eq35_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq35_judge.rs");
    for fixture in [
        "fn judge_unavailable_is_not_a_success",
        "fn available_fixed_judge_evidence_is_valid",
        "fn optional_unavailable_judge_is_never_a_silent_pass",
        "fn not_configured_judge_blocks_when_required",
        "fn unavailable_judge_requires_a_bounded_reason",
        "fn available_judge_requires_fixed_config_request_and_result",
        "fn available_judge_must_not_carry_an_unavailable_reason",
        "fn unavailable_judge_cannot_carry_a_result",
        "fn configuration_and_prompt_drift_are_visible",
        "fn fixed_identity_requires_bounded_fields_and_digests",
        "fn request_digests_must_be_strict_sha256",
        "fn result_score_and_digest_bounds_fail_closed",
        "fn failed_judge_verdict_is_not_silently_passed",
        "fn pass_verdict_with_zero_score_is_inconsistent",
        "fn unknown_fields_and_envelope_drift_fail_closed",
        "fn judge_contracts_round_trip_through_typed_input",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn evidence_sections_are_explicit_per_finding_family",
        "fn judge_output_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-35 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-35 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq35-semantic-judge.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-35 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq35_judge --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq35_judge_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq35_judge --no-run --locked",
        "cargo test -p kiana-quality --test eq35_judge_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-35 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
