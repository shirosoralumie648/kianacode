use serde_json::Value;

#[test]
fn performance_cost_evaluator_stays_pure_and_bucketed() {
    let source = include_str!("../src/metrics.rs");
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
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden metrics evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "PERFORMANCE_COST_INPUT_SCHEMA",
        "PERFORMANCE_COST_EVALUATOR_ID",
        "PerformanceCostEvaluator",
        "PerformanceCostInput",
        "PerformanceMetrics",
        "PerformanceThresholds",
        "CostBucketEvidence",
        "CostBucket::Estimated",
        "CostBucket::Measured",
        "CostBucket::Unknown",
        "MAX_COST_BUCKETS",
        "MAX_CACHE_LOOKUPS",
        "performance_cost_input_schema_invalid",
        "performance_cost_cost_bucket_limit_exceeded",
        "metrics.metrics_schema_invalid",
        "metrics.threshold_schema_invalid",
        "metrics.bucket_schema_invalid",
        "metrics.threshold_missing",
        "metrics.cost_limit_invalid",
        "metrics.measurement_provenance_invalid",
        "metrics.duration_unmeasured",
        "metrics.duration_exceeded",
        "metrics.usage_incomplete",
        "metrics.token_total_invalid",
        "metrics.tokens_exceeded",
        "metrics.tool_calls_exceeded",
        "metrics.cache_bounds_exceeded",
        "metrics.cache_count_mismatch",
        "metrics.cache_key_missing",
        "metrics.cache_key_unexpected",
        "metrics.bucket_duplicate",
        "metrics.bucket_currency_invalid",
        "metrics.bucket_amount_invalid",
        "metrics.bucket_estimate_unbound",
        "metrics.bucket_measured_unbound",
        "metrics.bucket_unknown_unbound",
        "metrics.bucket_reason_unexpected",
        "metrics.bucket_currency_mismatch",
        "metrics.cost_unmeasured",
        "metrics.cost_unknown",
        "metrics.cost_unenforced",
        "metrics.cost_exceeded",
        "metrics:input",
        "metrics:thresholds",
        "metrics:duration",
        "metrics:tokens",
        "metrics:tools",
        "metrics:cache",
        "metrics:usage",
        "metrics:cost",
        "metrics:bucket:",
        "is_bounded_reference",
        "is_sha256_digest",
        "is_currency_code",
        "bucket_sort_key",
        "redact_text",
        "Finding::redacted",
        "sha256:",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-34 boundary marker: {required}"
        );
    }
}

#[test]
fn eq34_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq34_metrics.rs");
    for fixture in [
        "fn budget_and_latency_thresholds_are_explicit",
        "fn performance_evidence_within_explicit_budgets_has_no_findings",
        "fn unmeasured_duration_is_not_latency_evidence",
        "fn duration_token_and_tool_overages_are_blocking",
        "fn token_total_must_equal_input_plus_output_without_overflow",
        "fn incomplete_usage_is_never_a_pass",
        "fn cache_usage_requires_a_strict_key_digest",
        "fn cache_key_without_usage_is_unexpected",
        "fn cache_counts_must_be_bounded_and_consistent",
        "fn measurement_digest_must_be_a_strict_sha256",
        "fn measured_cost_requires_a_receipt_rate_card_and_usage_digest",
        "fn estimated_cost_requires_a_rate_card_and_usage_digest",
        "fn unknown_cost_is_never_a_numeric_zero",
        "fn cost_buckets_must_not_mix_currencies",
        "fn enforced_cost_limit_blocks_known_bucket_overage",
        "fn cost_limit_must_be_explicitly_enforced",
        "fn measured_cost_requirement_rejects_estimates_and_unknown",
        "fn duplicate_cost_buckets_are_refused",
        "fn bucket_schema_drift_on_every_section_is_rejected",
        "fn unknown_fields_and_envelope_drift_fail_closed",
        "fn bounded_bucket_references_fail_closed",
        "fn oversized_bucket_sets_fail_closed_before_evaluation",
        "fn bucket_order_does_not_change_the_findings",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn performance_cost_contracts_round_trip_through_typed_input",
        "fn performance_output_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-34 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-34 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq34-performance-cost.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-34 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq34_metrics --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq34_metrics_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq34_metrics --no-run --locked",
        "cargo test -p kiana-quality --test eq34_metrics_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-34 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
