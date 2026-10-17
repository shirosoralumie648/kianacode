use serde_json::Value;

#[test]
fn flake_classifier_stays_pure_retry_once_and_quarantine_first() {
    let source = include_str!("../src/flake.rs");
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
            "forbidden flake classifier dependency: {forbidden}"
        );
    }
    for required in [
        "FLAKE_INPUT_SCHEMA",
        "FLAKE_EVALUATOR_ID",
        "FLAKE_ATTEMPT_SCHEMA",
        "FLAKE_QUARANTINE_SCHEMA",
        "MAX_FLAKE_ATTEMPTS",
        "MAX_FLAKE_TEXT_BYTES",
        "MAX_QUARANTINE_TTL_MS",
        "FlakeClassifierEvaluator",
        "FlakeInput",
        "FlakeAttempt",
        "FlakeQuarantine",
        "AttemptOutcome",
        "AttemptOutcome::Pass",
        "AttemptOutcome::Fail",
        "AttemptOutcome::Infra",
        "AttemptOutcome::OutOfScope",
        "InfraFailureClass",
        "InfraFailureClass::Runner",
        "InfraFailureClass::Fixture",
        "InfraFailureClass::Network",
        "InfraFailureClass::Resource",
        "InfraFailureClass::Unknown",
        "FlakeFinalStatus",
        "FlakeFinalStatus::Pass",
        "FlakeFinalStatus::Fail",
        "FlakeFinalStatus::Infra",
        "FlakeFinalStatus::OutOfScope",
        "FlakeFinalStatus::NeedsReview",
        "FlakeFinalStatus::Quarantined",
        "FlakeClassification",
        "FlakeClassification::Flake",
        "classify_flake",
        "flake_attempt_evidence_digest",
        "json_digest",
        "redact_text",
        "is_bounded_text",
        "is_digest",
        "attempt_actual",
        "quarantine_actual",
        "Finding::redacted",
        "MAX_FINDINGS",
        "flake:input",
        "flake:attempt:",
        "flake:status",
        "flake:quarantine",
        "flake_input_schema_invalid",
        "flake_input_attempts_missing",
        "flake_attempt_limit_exceeded",
        "flake_input_time_invalid",
        "flake.case_invalid",
        "flake.attempt_schema_invalid",
        "flake.attempt_order_invalid",
        "flake.attempt_case_mismatch",
        "flake.attempt_evidence_invalid",
        "flake.attempt_evidence_repeated",
        "flake.seed_mismatch",
        "flake.retry_policy_invalid",
        "flake.retry_after_pass",
        "flake.infra_unclassified",
        "flake.infra_class_unexpected",
        "flake.flaky_counted_as_pass",
        "flake.flaky_counted_as_result",
        "flake.infra_counted_as_result",
        "flake.out_of_scope_counted_as_result",
        "flake.status_attempt_mismatch",
        "flake.quarantine_missing",
        "flake.quarantine_unexpected",
        "flake.quarantine_schema_invalid",
        "flake.quarantine_invalid",
        "flake.quarantine_case_mismatch",
        "flake.quarantine_expired",
        "flake.quarantine_expiry_unbounded",
        "flake.quarantine_evidence_mismatch",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-37 boundary marker: {required}"
        );
    }
}

#[test]
fn eq37_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq37_flake.rs");
    for fixture in [
        "fn flake_is_never_counted_as_pass",
        "fn stable_failures_stay_failures_and_never_become_a_flake",
        "fn retry_is_bounded_to_one_retry_with_a_fixed_seed",
        "fn retry_after_pass_is_refused",
        "fn infra_failures_require_a_known_failure_class",
        "fn infra_attempts_are_never_case_results",
        "fn out_of_scope_attempts_are_never_counted_as_results",
        "fn quarantine_records_must_be_bounded",
        "fn quarantine_must_bind_the_case_and_the_attempt_evidence",
        "fn quarantine_must_expire_inside_a_bounded_window",
        "fn quarantine_is_required_for_flakes_and_refused_for_stable_outcomes",
        "fn attempt_schema_order_and_case_bindings_fail_closed",
        "fn attempt_evidence_digests_must_be_strict_and_fresh",
        "fn envelope_schema_attempts_time_and_unknown_fields_fail_closed",
        "fn case_identity_must_be_bounded_and_redacted",
        "fn classification_only_follows_attempt_evidence",
        "fn flake_contracts_round_trip_through_typed_input",
        "fn flake_output_is_deterministic_evidence_bound_and_registry_wired",
        "fn clean_evidence_stays_visible_across_every_outcome",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-37 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-37 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq37-flake.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-37 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq37_flake --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq37_flake_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq37_flake --no-run --locked",
        "cargo test -p kiana-quality --test eq37_flake_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-37 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
