//! OA-21 source guard: replay/reconciliation diagnostics compare committed projections,
//! they never call a model, Provider or Broker, never guess an unknown audit schema, never let a
//! metric/trace projection write a fact back and never swallow a divergence.

#[test]
fn oa21_replay_stays_read_only_and_compares_audit_receipt_health_against_a_baseline() {
    let domain = include_str!("../../kiana-domain/src/observability.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let core = include_str!("../src/replay_diagnostics.rs");
    let lib = include_str!("../src/lib.rs");
    let fixture = include_str!("fixtures/oa21-replay-consistency.json");
    let core_fixture = include_str!("oa21_replay_diagnostics.rs");
    let workflow = include_str!("../../.github/workflows/oa21-replay-diagnostics.yml");
    let baseline = include_str!("../../docs/roadmap/observability-audit-baseline.md");
    let shards = include_str!("../../scripts/ci/test-shards.json");

    // Domain contract: a baseline pins named deterministic projections, the snapshot carries a
    // canonical locator, and forged digests/unknown projectors/unsorted diagnostics fail closed.
    for marker in [
        "kiana.replay-consistency-baseline.v1",
        "ReplayConsistencyBaseline",
        "REPLAY_CONSISTENCY_PROJECTIONS",
        "ProjectionMismatch",
        "first_divergence",
        "locator_key",
        "is_consistent",
        "replay_diagnostic_order_invalid",
        "replay_baseline_projection_required",
        "replay_baseline_projection_unknown",
        "replay_baseline_projection_digest",
        "replay_baseline_source_event_ids_duplicate",
        "replay_baseline_digest_mismatch",
        "replay_baseline_digest",
        "expected_error_code",
        "observed_error_code",
        "trace_status_name",
    ] {
        assert!(
            domain.contains(marker),
            "OA-21 domain marker missing: {marker}"
        );
    }
    assert!(contracts.contains("kiana.replay-consistency-baseline.v1"));

    // Core comparison: audit/receipt/health spans are replayed read-only, baseline mismatches get
    // named error codes and expectations cannot be silently ignored.
    for marker in [
        "replay_projection_only_read_only",
        "replay_baseline_not_provided",
        "replay_source_event_ids_truncated",
        "replay_receipt_projection_unavailable",
        "diagnose_replay_with_baseline",
        "compare_consistency_baseline",
        "aggregate_receipt_facts",
        "rebuild_audit_projection",
        "project_health_snapshot",
        "project_span_lifecycle",
        "replay_baseline_source_mismatch",
        "digest_mismatch",
        "digest_unavailable",
        "replay_expectation_invocation_missing",
        "replay_error_code_mismatch",
        "replay_expectation_run_required",
        "replay_read_all_unsupported",
        "first_divergence()",
    ] {
        assert!(core.contains(marker), "OA-21 core marker missing: {marker}");
    }
    assert!(lib.contains("diagnose_replay_with_baseline"));

    // No second execution surface: replay never reaches a model, Provider, Broker, harness,
    // recovery path, approval consumer or retry/compensation, and never writes a fact.
    for forbidden in [
        "CapabilityBroker",
        "ModelClient",
        "KianaHarness",
        "MetricSink",
        "TraceSink",
        "RecoveryAction",
        "approval",
        "reconcile_effect",
        "authorize_and_execute",
        "record_observation",
        "append_fact",
        "write_all_events",
        "compensate",
        ".append(",
    ] {
        assert!(
            !core.contains(forbidden),
            "OA-21 replay must not contain: {forbidden}"
        );
    }

    for fixture_case in [
        "duplicate_source_is_rejected_before_any_projection_or_side_effect",
        "undeclared_run_expectations_and_unmatched_invocations_are_refused",
        "expectation_error_codes_are_compared_without_collapsing_into_status",
        "unknown_audit_schema_is_refused_instead_of_guessed",
        "baseline_pins_audit_receipt_and_health_projectors",
        "replay_bridge_compares_a_baseline_without_appending_any_fact",
        "replay_bridge_reports_unavailable_read_all_instead_of_an_empty_verdict",
    ] {
        assert!(
            core_fixture.contains(fixture_case),
            "OA-21 fixture missing: {fixture_case}"
        );
    }

    // The declarative fixture names the same tamper, source and missing-projection boundaries.
    for marker in [
        "\"tamper\"",
        "replay_receipt_digest_mismatch",
        "replay_baseline_source_mismatch",
        "replay_receipt_digest_unavailable",
        "audit.forged",
    ] {
        assert!(
            fixture.contains(marker),
            "OA-21 fixture marker missing: {marker}"
        );
    }

    // Focused lane and automatic shard both run the new targets.
    for target in [
        "cargo test -p kiana-core --test oa21_replay_diagnostics",
        "cargo test -p kiana-core --test oa21_replay_diagnostics_guard",
        "cargo test -p kiana-core --test h25_replay_fault_guard",
        "cargo fmt --all --check",
    ] {
        assert!(
            workflow.contains(target),
            "OA-21 focused lane missing target: {target}"
        );
    }
    for target in ["oa21_replay_diagnostics", "oa21_replay_diagnostics_guard"] {
        assert!(
            shards.contains(target),
            "OA-21 shard registration missing: {target}"
        );
    }
    assert!(baseline.contains("kiana.replay-consistency-baseline.v1"));
}
