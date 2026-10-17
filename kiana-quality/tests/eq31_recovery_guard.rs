use serde_json::Value;

#[test]
fn recovery_replay_evaluator_stays_pure_and_fail_closed() {
    let source = include_str!("../src/recovery.rs");
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "tokio::",
        "reqwest::",
        "Provider",
        "ArtifactStore",
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
            "forbidden recovery evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "RECOVERY_REPLAY_INPUT_SCHEMA",
        "RECOVERY_REPLAY_EVALUATOR_ID",
        "RecoveryReplayEvaluator",
        "MAX_ATTEMPTS",
        "MAX_TEXT_BYTES",
        "replay.crash_schema_invalid",
        "replay.fence_schema_invalid",
        "replay.attempt_schema_invalid",
        "replay.unknown_schema_invalid",
        "replay.evidence_schema_invalid",
        "replay.cursor_invalid",
        "replay.restart_missing",
        "replay.resume_missing",
        "replay.resume_ref_missing",
        "replay.resume_ref_unexpected",
        "replay.checkpoint_invalid",
        "replay.checkpoint_cursor_mismatch",
        "replay.fence_invalid",
        "replay.fence_reused",
        "replay.fence_epoch_not_advanced",
        "replay.fence_scope_invalid",
        "replay.fence_scope_mismatch",
        "replay.attempt_invalid",
        "replay.attempt_fence_invalid",
        "replay.attempt_fence_reused",
        "replay.attempt_cursor_invalid",
        "replay.attempt_cursor_regressed",
        "replay.attempt_after_terminal",
        "replay.fence_epoch_mismatch",
        "replay.logic_version_invalid",
        "replay.logic_version_drift",
        "replay.digest_invalid",
        "replay.divergence",
        "replay.divergence_ref_missing",
        "replay.divergence_ref_unexpected",
        "replay.cursor_not_covered",
        "replay.result_unknown",
        "replay.reconcile_missing",
        "replay.reconciliation_outcome_invalid",
        "replay.reconciliation_ref_missing",
        "replay.reconciliation_ref_unexpected",
        "replay.unknown_retry_forbidden",
        "replay.unknown_contract_invalid",
        "replay:crash-restart",
        "replay:fence",
        "replay:attempt:",
        "replay:unknown",
        "replay:replay",
        "not_attempted",
        "reconciled_applied",
        "reconciled_absent",
        "is_bounded_reference",
        "is_sha256_digest",
        "redact_text",
        "Finding::redacted",
        "sha256:",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-31 boundary marker: {required}"
        );
    }
}

#[test]
fn eq31_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq31_recovery.rs");
    for fixture in [
        "fn replay_divergence_and_unknown_are_distinct_findings",
        "fn equivalent_restart_replay_has_no_findings",
        "fn crash_restart_and_resume_contract_is_fail_closed",
        "fn resume_and_checkpoint_refs_are_bounded_and_bound_to_the_restart",
        "fn fence_revocation_reuse_and_epoch_advance_are_enforced",
        "fn fence_scope_must_stay_bounded_and_stable",
        "fn attempt_epochs_cursors_and_terminal_order_are_bounded",
        "fn last_attempt_epoch_must_match_the_new_fence",
        "fn restart_replay_must_cover_the_checkpoint_and_post_restart_cursor",
        "fn logic_version_drift_is_separate_from_digest_divergence",
        "fn unknown_result_requires_reconciliation_and_never_permits_retry",
        "fn retry_after_a_proven_absent_effect_is_the_only_retry_path",
        "fn known_result_cannot_carry_unknown_retry_contract",
        "fn schema_drift_on_every_recovery_section_is_rejected",
        "fn unknown_fields_and_envelope_drift_fail_closed",
        "fn bounded_references_and_versions_fail_closed",
        "fn oversized_attempts_fail_closed_before_evaluation",
        "fn attempt_order_does_not_change_the_findings",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn recovery_contracts_round_trip_through_typed_input",
        "fn recovery_output_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-31 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-31 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq31-recovery-replay.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-31 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq31_recovery --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq31_recovery_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq31_recovery --no-run --locked",
        "cargo test -p kiana-quality --test eq31_recovery_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-31 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
