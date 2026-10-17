//! BQ-13 cost/receipt projection guard (deny first, success after).
//!
//! The cost contract is an observation only: it never appends events, consumes or releases a
//! reservation, authorizes a capability or turns a projection into a financial budget.  The
//! fixtures below pin the source markers, the refusal-code matrix, the fixture names and the CI
//! wiring (manual lane plus the automatic shards).

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/billing_cost.rs");
const AGGREGATION_SOURCE: &str = include_str!("../../kiana-domain/src/receipt_aggregation.rs");
const QUERY_SOURCE: &str = include_str!("../../kiana-query/src/cost_projector.rs");
const RECEIPTS_SOURCE: &str = include_str!("../src/receipts.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq13_cost_receipt.rs");
const CORE_FIXTURES: &str = include_str!("bq13_cost_receipt.rs");
const QUERY_FIXTURES: &str = include_str!("../../kiana-query/tests/bq13_cost_projector.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq13-cost-receipt.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

#[test]
fn bq13_cost_receipt_contract_is_pinned_and_read_only() {
    for marker in [
        "CostBreakdown",
        "ReceiptCostBreakdown",
        "CostBreakdownKind::Estimated",
        "CostBreakdownKind::Measured",
        "CostBreakdownKind::Unknown",
        "rate_card_version",
        "provider_receipt",
        "unknown_reasons",
        "project_receipt_cost_breakdown",
        "cost_breakdown",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker)
                || AGGREGATION_SOURCE.contains(marker)
                || QUERY_SOURCE.contains(marker)
                || RECEIPTS_SOURCE.contains(marker),
            "BQ-13 source marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBrokerPort",
        "EventStorePort",
        "tokio::spawn",
        "reqwest",
        "commit_transition",
        "append_event(",
        "std::fs",
        "std::net",
        "TcpStream",
        "unsafe",
        "#[ignore",
        "FinancialBudget",
        "cost = 0",
        "estimate_settle",
        "Uuid::new_v4",
    ] {
        for (name, source) in [
            ("domain", DOMAIN_SOURCE),
            ("aggregation", AGGREGATION_SOURCE),
            ("query", QUERY_SOURCE),
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-13 {name} authority widened: {forbidden}"
            );
        }
    }
    // The core receipt path consumes the read-only projectors and keeps the optional typed field.
    for marker in [
        "project_receipt_cost_breakdown",
        "with_cost_breakdown",
        "cost_breakdown",
        "projection_only",
    ] {
        assert!(
            RECEIPTS_SOURCE.contains(marker) || DOMAIN_SOURCE.contains(marker),
            "BQ-13 receipt wiring marker missing: {marker}"
        );
    }
}

#[test]
fn bq13_refusal_codes_stay_deny_first_in_the_fixtures() {
    for code in [
        "cost_observation_invalid",
        "cost_rate_card_scope_mismatch",
        "cost_measured_rate_card_missing",
        "cost_measured_usage_incomplete",
        "cost_measured_amount_negative",
        "cost_measured_currency_mismatch",
        "cost_measured_rate_card_forbidden",
        "receipt_cost_attempt_missing",
        "receipt_cost_attempt_duplicate",
        "receipt_cost_entry_duplicate",
        "receipt_cost_run_mismatch",
        "receipt_cost_breakdown_total_mismatch",
        "receipt_cost_duplicate_source_event",
        "receipt_cost_event_decode_failed",
        "receipt_cost_event_revision_gap",
        "receipt_cost_event_kind_mismatch",
        "receipt_cost_event_transition_invalid",
        "receipt_cost_event_stream_mismatch",
        "receipt_cost_breakdown_legacy_mismatch",
        "receipt_cost_unknown_must_remain_visible",
        "event_required_id_missing:attempt_id",
        "event_payload_unknown_field",
        "cost_query_projection_header_invalid",
        "cost_query_projection_invalid",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(code)
                || CORE_FIXTURES.contains(code)
                || QUERY_FIXTURES.contains(code)
                || DOMAIN_SOURCE.contains(code)
                || AGGREGATION_SOURCE.contains(code)
                || QUERY_SOURCE.contains(code),
            "BQ-13 refusal code is not covered by a fixture: {code}"
        );
    }
    for fixtures in [DOMAIN_FIXTURES, CORE_FIXTURES, QUERY_FIXTURES] {
        assert!(
            !fixtures.contains("#[ignore"),
            "BQ-13 fixtures must not ignore a denial path"
        );
    }
}

#[test]
fn bq13_fixture_names_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for name in [
        // Domain fixtures, denials first.
        "estimate_is_card_version_bound_and_has_dimension_lines",
        "overlapping_usage_is_unknown_before_cost_lines_are_emitted",
        "missing_card_or_partial_usage_is_unknown_and_never_zero",
        "measured_requires_card_and_complete_usage_but_serializes_only_receipt",
        "receipt_breakdown_keeps_estimate_measured_and_unknown_separate",
        "receipt_breakdown_rejects_cross_run_entries",
        "cost_events_replay_latest_measured_without_double_counting_estimate",
        "estimate_scope_drift_and_invalid_observation_are_refused",
        "measured_rejects_negative_amount_and_scope_or_currency_drift",
        "estimate_lines_use_checked_integer_micros",
        "measured_zero_is_an_explicit_observation_and_unknown_never_carries_an_amount",
        "receipt_breakdown_requires_unique_attempts_and_rejects_replayed_entries",
        "receipt_breakdown_recomputes_totals_and_rejects_tampered_totals",
        "cost_reducer_refuses_duplicate_stream_gaps_kind_drift_and_usage_digest_drift",
        "estimate_and_measured_facts_are_registered_billing_attempt_events",
        // Core fixtures.
        "aggregate_receipt_exposes_cost_breakdown_and_unknown_reasons",
        "aggregate_rejects_cost_event_run_or_stream_drift",
        "receipts_cost_boundary_is_projection_only",
        "receipt_aggregation_keeps_legacy_cost_fields_in_sync_with_the_breakdown",
        "receipt_aggregation_rejects_tampered_cost_consistency",
        "aggregate_receipt_facts_rejects_duplicate_and_malformed_cost_facts",
        // Query fixtures.
        "query_projector_preserves_cursor_and_measured_unknown_split",
        "query_projector_does_not_turn_empty_source_into_zero_cost",
        "query_projector_source_guard_is_read_only",
        "query_projector_rejects_envelope_cursor_drift",
        "query_projector_keeps_unknown_reasons_and_measured_total_separate",
        "query_projector_rejects_duplicate_and_malformed_cost_facts",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name)
                || CORE_FIXTURES.contains(name)
                || QUERY_FIXTURES.contains(name),
            "BQ-13 fixture missing: {name}"
        );
    }

    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq13_cost_receipt --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq13_cost_receipt --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq13_cost_receipt_guard --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq13_cost_projector --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test bq13_cost_receipt --no-run --locked",
        "cargo test -p kiana-core --test bq13_cost_receipt --no-run --locked",
        "cargo test -p kiana-core --test bq13_cost_receipt_guard --no-run --locked",
        "cargo test -p kiana-query --test bq13_cost_projector --no-run --locked",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-13 manual lane missing: {command}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch"));
    assert!(MANUAL_LANE.contains("contents: read"));
    assert!(
        !MANUAL_LANE.contains("push:") && !MANUAL_LANE.contains("pull_request:"),
        "BQ-13 lane must stay workflow_dispatch only"
    );
    for shard_target in [
        "\"bq13_cost_receipt\"",
        "\"bq13_cost_receipt_guard\"",
        "\"kiana-query\"",
    ] {
        assert!(
            SHARDS.contains(shard_target),
            "BQ-13 automatic shard registration missing: {shard_target}"
        );
    }
}
