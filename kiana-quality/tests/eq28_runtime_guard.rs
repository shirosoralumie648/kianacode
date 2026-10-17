#[test]
fn runtime_evaluator_stays_pure_and_bounded() {
    let source = include_str!("../src/runtime.rs");
    for forbidden in [
        "std::fs",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "Command::new",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden runtime evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "RUNTIME_CORRECTNESS_INPUT_SCHEMA",
        "RuntimeCorrectnessEvaluator",
        "runtime.event_order_invalid",
        "runtime.invocation_correlation_mismatch",
        "runtime.terminal_duplicate",
        "runtime.retry_order_invalid",
        "runtime.approval_unmatched",
        "runtime.cancel_unfenced",
        "runtime.unknown_retry_forbidden",
        "is_post_terminal_projection",
        "lifecycle_scope_keys",
        "run.receipt",
        "run.snapshot",
        "no_invocation_events_after_run_terminal",
        "next_attempt_after_retry_ordinal",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-28 boundary marker: {required}"
        );
    }
}

#[test]
fn runtime_correctness_fixture_matrix_covers_every_lifecycle_family() {
    let fixtures = include_str!("eq28_runtime.rs");
    for required in [
        "fn valid_runtime_lifecycle_has_no_findings",
        "fn post_terminal_projections_and_additional_attempts_are_not_findings",
        "fn runtime_evaluator_catches_unmatched_and_duplicate_terminal",
        "fn events_after_run_terminal_are_rejected",
        "fn event_order_and_correlation_regressions_are_rejected",
        "fn malformed_event_envelopes_are_rejected",
        "fn invocation_identity_is_required_for_attempt_events",
        "fn invocation_alias_conflicts_are_rejected",
        "fn retry_lifecycle_with_scheduled_attempts_has_no_findings",
        "fn multiple_model_calls_in_one_run_are_not_terminal_conflicts",
        "fn retry_metadata_and_order_are_checked",
        "fn approval_lifecycle_order_is_enforced",
        "fn cancel_requires_a_fence_and_a_terminal",
        "fn unknown_outcome_blocks_a_later_retry_and_cancel_requires_a_fence",
        "fn unknown_must_be_reconciled_before_a_duplicate_or_retry",
        "fn runtime_evaluator_output_is_deterministic_and_evidence_bound",
        "fn malformed_runtime_input_is_rejected_before_evaluation",
        "fn empty_trace_is_a_finding_not_a_panic",
    ] {
        assert!(
            fixtures.contains(required),
            "missing EQ-28 lifecycle fixture: {required}"
        );
    }
}
