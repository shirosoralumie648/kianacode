use serde_json::Value;

#[test]
fn baseline_registry_stays_expiry_digest_and_provenance_bound() {
    let source = include_str!("../src/baseline.rs");
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
            "forbidden baseline evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "BASELINE_RECORD_SCHEMA",
        "BASELINE_REGISTRY_SCHEMA",
        "BASELINE_INPUT_SCHEMA",
        "BASELINE_REGISTRY_INPUT_SCHEMA",
        "BASELINE_EVALUATOR_ID",
        "BASELINE_LOOKUP_EVALUATOR_ID",
        "MAX_BASELINE_ENTRIES",
        "MAX_BASELINE_TEXT_BYTES",
        "MAX_BASELINE_TTL_MS",
        "BaselineRecord",
        "BaselineRegistry",
        "BaselineComparisonInput",
        "BaselineRegistryInput",
        "BaselineEvaluator",
        "BaselineRegistryEvaluator",
        "evaluate_baseline_comparison",
        "evaluate_baseline_lookup",
        "refreshed_from",
        "validate_refresh_chains",
        "version_key",
        "lookup",
        "valid_text",
        "valid_digest",
        "deny_unknown_fields",
        "json_digest",
        "Finding::new",
        "MAX_FINDINGS",
        "BTreeSet",
        "BTreeMap",
        "baseline:input",
        "baseline:registry",
        "baseline.record_invalid",
        "baseline.registry_invalid",
        "baseline.record_missing",
        "baseline.not_yet_valid",
        "baseline.stale",
        "baseline.incompatible",
        "baseline_record_schema_invalid",
        "baseline_record_identity_invalid",
        "baseline_record_window_invalid",
        "baseline_record_digest_invalid",
        "baseline_record_digest_mismatch",
        "baseline_registry_schema_invalid",
        "baseline_registry_empty",
        "baseline_registry_size_exceeded",
        "baseline_registry_digest_invalid",
        "baseline_registry_digest_mismatch",
        "baseline_registry_duplicate",
        "baseline_registry_overlap",
        "baseline_registry_provenance_mismatch",
        "baseline_registry_provenance_reused",
        "baseline_input_schema_invalid",
        "baseline_input_time_invalid",
        "baseline_input_digest_invalid",
        "baseline_registry_input_schema_invalid",
        "baseline_registry_input_identity_invalid",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-39 boundary marker: {required}"
        );
    }
}

#[test]
fn eq39_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq39_baseline.rs");
    for fixture in [
        "fn compatible_unexpired_baseline_can_compare",
        "fn stale_or_incompatible_baseline_cannot_compare",
        "fn tampered_baseline_records_fail_closed",
        "fn expiry_window_is_bounded_and_exclusive",
        "fn comparison_envelope_fails_closed",
        "fn registry_requires_digest_binding_and_bounded_entries",
        "fn registry_rejects_duplicate_or_tampered_entries",
        "fn refreshed_baselines_must_link_the_predecessor",
        "fn registry_lookup_resolves_the_named_baseline",
        "fn lookup_envelope_fails_closed",
        "fn baseline_output_is_deterministic_and_registry_wired",
        "fn typed_constructors_round_trip",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-39 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-39 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq39-baseline.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-39 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq39_baseline --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq39_baseline_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-39 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
