use serde_json::Value;

#[test]
fn workflow_swarm_evaluator_stays_pure_and_bounded() {
    let source = include_str!("../src/workflow.rs");
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
        "TeamCreate",
        "SendMessage",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden workflow evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "WORKFLOW_SWARM_INPUT_SCHEMA",
        "WORKFLOW_SWARM_EVALUATOR_ID",
        "WorkflowSwarmEvaluator",
        "WorkflowSwarmInput",
        "WorkflowNodeEvidence",
        "WorkflowNodeStatus",
        "MAX_NODES",
        "MAX_ATTEMPTS_PER_NODE",
        "MAX_FAN_IN",
        "MAX_SCOPE_ENTRIES",
        "MAX_TEXT_BYTES",
        "MAX_FINDINGS",
        "workflow.bounds_invalid",
        "workflow.node_schema_invalid",
        "workflow.node_identity_invalid",
        "workflow.parent_identity_invalid",
        "workflow.parent_self",
        "workflow.parent_missing",
        "workflow.depth_exceeded",
        "workflow.depth_inconsistent",
        "workflow.attempt_invalid",
        "workflow.attempt_unbounded",
        "workflow.fanout_exceeded",
        "workflow.fanout_mismatch",
        "workflow.fanin_unbounded",
        "workflow.fanin_mismatch",
        "workflow.scope_invalid",
        "workflow.child_scope_expanded",
        "workflow.parent_scope_expanded",
        "workflow.merge_missing",
        "workflow.merge_invalid",
        "workflow.merge_mismatch",
        "workflow.merge_unexpected",
        "workflow.compensation_missing",
        "workflow.compensation_unexpected",
        "workflow.compensation_receipt_missing",
        "workflow.compensation_receipt_unexpected",
        "workflow.compensation_invalid",
        "workflow.dag_cycle",
        "workflow:input",
        "workflow:dag",
        "workflow:node:",
        "compensation_receipt_ref",
        "merge_expected_digest",
        "merge_observed_digest",
        "is_bounded_reference",
        "is_sha256_digest",
        "node_sort_key",
        "redact_text",
        "Finding::redacted",
        "sha256:",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-33 boundary marker: {required}"
        );
    }
}

#[test]
fn eq33_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq33_workflow.rs");
    for fixture in [
        "fn bounded_dag_attempts_and_scopes_have_no_findings",
        "fn child_scope_expansion_fails_quality_gate",
        "fn dag_attempt_fanout_merge_and_compensation_fail_closed",
        "fn unknown_fields_fail_closed",
        "fn dag_cycles_missing_parents_and_self_parents_are_rejected",
        "fn node_identity_parent_identity_and_bounded_references_are_rejected",
        "fn depth_must_increase_along_every_parent_edge",
        "fn attempts_must_be_positive_strictly_increasing_and_bounded",
        "fn fan_out_bounds_and_expected_observed_children_must_agree",
        "fn fan_in_counts_must_be_coherent_with_declared_parents",
        "fn merge_evidence_is_required_for_fan_in_and_unexpected_below_it",
        "fn merge_digests_must_be_strict_sha256_and_equal",
        "fn parent_scope_cannot_exceed_parent_child_scope",
        "fn scope_entries_are_bounded_and_deduplicated",
        "fn compensation_requires_recorded_receipt_evidence",
        "fn compensating_and_compensated_statuses_are_coherent",
        "fn schema_drift_on_envelope_and_nodes_is_rejected",
        "fn oversized_node_inputs_fail_closed_before_evaluation",
        "fn envelope_bounds_are_positive_evidence",
        "fn node_order_does_not_change_the_findings",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn workflow_contracts_round_trip_through_typed_input",
        "fn workflow_output_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-33 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-33 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq33-workflow-swarm.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-33 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq33_workflow --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq33_workflow_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq33_workflow --no-run --locked",
        "cargo test -p kiana-quality --test eq33_workflow_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-33 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
