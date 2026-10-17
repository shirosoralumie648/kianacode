use serde_json::Value;

#[test]
fn context_memory_evaluator_stays_pure_and_acl_first() {
    let source = include_str!("../src/context.rs");
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "tokio::",
        "reqwest::",
        "Provider",
        "MemoryStore",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "ControlPlane",
        "Command::new",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden context evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "CONTEXT_MEMORY_INPUT_SCHEMA",
        "CONTEXT_MEMORY_EVALUATOR_ID",
        "ContextMemoryEvaluator",
        "ContextQueryEvidence",
        "MemoryHitEvidence",
        "ContextBudgetEvidence",
        "CompactionEvidence",
        "MAX_HITS",
        "MAX_TEXT_BYTES",
        "context.query_schema_invalid",
        "context.hit_schema_invalid",
        "context.budget_schema_invalid",
        "context.compaction_schema_invalid",
        "context.query_provenance_invalid",
        "context.selection_bound_invalid",
        "context.acl_order_invalid",
        "context.acl_evidence_incoherent",
        "context.unauthorized_memory_hit",
        "context.hit_identity_invalid",
        "context.provenance_invalid",
        "context.scope_mismatch",
        "context.epoch_mismatch",
        "context.rank_invalid",
        "context.rank_duplicate",
        "context.rank_gap",
        "context.selection_order_invalid",
        "context.selection_over_top_k",
        "context.stale_acknowledgement_unexpected",
        "context.stale_not_acknowledged",
        "context.freshness_unknown",
        "context.evidence_missing",
        "context.budget_invalid",
        "context.budget_estimate_invalid",
        "context.budget_overflow",
        "context.budget_unenforced",
        "context.compaction_summary_missing",
        "context.compaction_summary_unbound",
        "context.compaction_cursor_invalid",
        "context.compaction_integrity_invalid",
        "context.compaction_stale_commit_unfenced",
        "context.compaction_unexpected",
        "context:query",
        "context:acl",
        "context:ranking",
        "context:hit:",
        "context:budget",
        "context:compaction",
        "acl_checked_before_ranking",
        "top_k",
        "summary_digest",
        "stale_acknowledged",
        "is_bounded_reference",
        "is_sha256_digest",
        "redact_text",
        "Finding::redacted",
        "sha256:",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-32 boundary marker: {required}"
        );
    }
}

#[test]
fn eq32_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq32_context.rs");
    for fixture in [
        "fn unauthorized_memory_hit_is_blocking",
        "fn admitted_current_context_has_no_findings",
        "fn acl_evidence_cannot_be_claimed_without_a_recorded_check",
        "fn acl_denied_hits_are_never_ranked_or_selected",
        "fn rank_order_must_be_unique_and_dense",
        "fn selection_respects_the_top_k_bound_and_the_rank_prefix",
        "fn hit_identity_and_provenance_are_bounded",
        "fn hit_scope_and_epoch_must_match_the_query",
        "fn stale_selection_requires_an_acknowledgement",
        "fn freshness_unknown_is_never_silently_selected",
        "fn evidence_status_must_be_explicit_for_selected_hits",
        "fn query_provenance_is_fail_closed",
        "fn budget_overflow_and_estimate_bounds_are_enforced",
        "fn budget_must_be_enforced_for_tokens_and_wire",
        "fn compaction_requires_summary_cursor_and_preservation",
        "fn compaction_summary_digest_must_be_a_strict_sha256",
        "fn compaction_evidence_without_an_attempt_is_unexpected",
        "fn schema_drift_on_every_context_section_is_rejected",
        "fn unknown_fields_and_envelope_drift_fail_closed",
        "fn bounded_references_fail_closed",
        "fn oversized_hits_fail_closed_before_evaluation",
        "fn hit_order_does_not_change_the_findings",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn context_contracts_round_trip_through_typed_input",
        "fn context_output_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-32 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-32 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq32-context-memory.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-32 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq32_context --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq32_context_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq32_context --no-run --locked",
        "cargo test -p kiana-quality --test eq32_context_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-32 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
