use serde_json::Value;

#[test]
fn aggregation_evaluator_stays_pure_and_explicit() {
    let source = include_str!("../src/aggregation.rs");
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
            "forbidden aggregation evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "AGGREGATION_INPUT_SCHEMA",
        "AGGREGATION_EVALUATOR_ID",
        "DIMENSION_AGGREGATE_SCHEMA",
        "AggregationEvaluator",
        "AggregationInput",
        "DimensionAggregate",
        "AggregateVerdict",
        "AggregateVerdict::Pass",
        "AggregateVerdict::Fail",
        "AggregateVerdict::NeedsReview",
        "AggregateVerdict::Blocked",
        "MAX_AGGREGATION_DIMENSIONS",
        "MAX_AGGREGATION_TEXT_BYTES",
        "aggregation_input_schema_invalid",
        "aggregation_input_dimensions_missing",
        "aggregation_dimension_limit_exceeded",
        "aggregation.dimension_schema_invalid",
        "aggregation.dimension_invalid",
        "aggregation.minimum_sample_invalid",
        "aggregation.insufficient_sample",
        "aggregation.threshold_missing",
        "aggregation.absolute_threshold_exceeded",
        "aggregation.relative_threshold_invalid",
        "aggregation.relative_threshold_exceeded",
        "aggregation.confidence_missing",
        "aggregation.confidence_invalid",
        "aggregation.false_pass",
        "aggregation.sample_verdict_invalid",
        "aggregation:dimension:",
        "aggregation:verdict",
        "is_bounded_dimension",
        "dimension_sort_key",
        "dimension_actual",
        "redact_text",
        "Finding::redacted",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-36 boundary marker: {required}"
        );
    }
}

#[test]
fn eq36_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq36_aggregation.rs");
    for fixture in [
        "fn insufficient_sample_is_blocked_or_needs_review",
        "fn sufficient_sample_with_explicit_thresholds_and_interval_is_valid",
        "fn dimension_schema_drift_is_rejected",
        "fn dimension_identity_must_be_bounded_and_unique",
        "fn minimum_sample_must_be_positive",
        "fn missing_threshold_policy_cannot_pass",
        "fn absolute_threshold_overage_is_blocking",
        "fn relative_threshold_requires_a_positive_baseline",
        "fn relative_threshold_overage_is_blocking",
        "fn confidence_interval_is_required_explicitly",
        "fn inverted_or_partial_confidence_intervals_are_invalid",
        "fn observed_outside_the_confidence_interval_is_invalid",
        "fn false_pass_is_never_valid_for_blocking_evidence",
        "fn envelope_schema_dimension_limits_and_unknown_fields_fail_closed",
        "fn dimension_order_does_not_change_the_findings",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn aggregation_contracts_round_trip_through_typed_input",
        "fn aggregation_output_is_deterministic_evidence_bound_and_registry_wired",
        "fn sample_sufficiency_is_independent_of_threshold_and_confidence_evidence",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-36 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-36 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq36-aggregation.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-36 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq36_aggregation --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq36_aggregation_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq36_aggregation --no-run --locked",
        "cargo test -p kiana-quality --test eq36_aggregation_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-36 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
