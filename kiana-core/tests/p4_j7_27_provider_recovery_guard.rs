use std::fs;

#[test]
fn provider_recovery_reuses_history_resume_and_single_execution_spine() {
    let recovery = fs::read_to_string("../kiana-domain/src/provider_recovery.rs")
        .expect("provider recovery source");
    for marker in [
        "PROVIDER_RECOVERY_SCHEMA",
        "ProviderResumeBinding",
        "PROVIDER_RESUME_BINDING_SCHEMA",
        "validate_for_resume",
        "ProtectedReplayMaterial",
        "missing_reasoning_material_blocks_resume",
        "stale_route_authority_cannot_resume",
        "ProviderHistoryFact",
        "ProviderHistoryProjection",
        "validate_model_history",
        "ProviderInFlightPhase",
        "ProviderReconciliationCase",
        "ReconcileModelUnknown",
        "ReconcileCapabilityUnknown",
        "automatic_retry_allowed",
        "PROVIDER_RECOVERY_NO_MODEL_RESUBMIT",
        "PROVIDER_ROUND_PLAN_SCHEMA",
        "ProviderRoundPlan",
        "ProviderResumeCheck",
        "authorize_resume",
        "provider_recovery_round_resume_blocked_by_unknown",
        "provider_recovery_round_attempt_already_consumed",
        "provider_recovery_round_scope_mismatch",
    ] {
        assert!(
            recovery.contains(marker),
            "P4-J7-27 marker missing: {marker}"
        );
    }
    let recovery_code: String = recovery
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "tokio::spawn",
        "reqwest::Client",
        "TcpStream",
        "CapabilityBrokerPort",
        "EventStorePort",
        "ControlPlane",
        "std::fs",
        "std::net",
        "SecretRef",
        "CredentialLease",
        "EventLog",
        "drive_run",
        "unsafe",
    ] {
        assert!(
            !recovery_code.contains(forbidden),
            "provider recovery contract introduces an authority/effect path: {forbidden}"
        );
    }
}

#[test]
fn provider_recovery_uses_committed_facts_and_explicit_resume_claim() {
    let history = fs::read_to_string("../kiana-core/src/history.rs").expect("history source");
    assert!(history.contains("model_protocol_history"));
    assert!(history.contains("read_all_events"));
    assert!(history.contains("validate_model_history"));

    let recovery = fs::read_to_string("../kiana-core/src/recovery.rs").expect("recovery source");
    for marker in [
        "pub async fn resume_run",
        "run.resume_prepared",
        "self.runner.restore",
        "run_resume_claim_conflict",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
    ] {
        assert!(recovery.contains(marker), "resume marker missing: {marker}");
    }
    let lifecycle = fs::read_to_string("../kiana-core/src/lifecycle.rs").expect("lifecycle source");
    assert!(lifecycle.contains("self.drive_run"));
    assert!(lifecycle.contains("pub(crate) async fn drive_run"));
}

#[test]
fn provider_continuation_is_validated_at_wire_boundary_without_replay_authority() {
    let request = fs::read_to_string("../kiana-provider/src/request.rs").expect("request source");
    for marker in [
        "continuation.validate()",
        "opaque_item_cannot_cross_provider",
        "provider_continuation_unsupported",
    ] {
        assert!(
            request.contains(marker),
            "continuation marker missing: {marker}"
        );
    }
    let replay = fs::read_to_string("../kiana-domain/src/protected_replay.rs")
        .expect("protected replay source");
    assert!(replay.contains("missing_reasoning_material_blocks_resume"));
    assert!(replay.contains("validate_for_call"));
}

#[test]
fn round_plan_recovery_fixtures_stay_wired_to_the_manual_lane_and_auto_shards() {
    let fixtures = fs::read_to_string("../kiana-domain/tests/p4_j7_27_provider_recovery.rs")
        .expect("domain provider recovery fixtures");
    for fixture in [
        "restart_with_missing_replay_material_fails_closed",
        "stale_route_authority_cannot_resume",
        "inflight_model_attempt_is_not_resubmitted_on_restart",
        "fresh_process_resumes_completed_model_turn_with_tool_history",
        "resume_never_reexecutes_completed_invocation",
        "expired_resume_binding_and_widened_ttl_are_rejected_before_any_send",
        "tampered_binding_digest_cannot_be_trusted_even_with_matching_fields",
        "revoked_deleted_or_route_switched_replay_material_blocks_resume",
        "history_rejects_partial_duplicate_and_out_of_scope_facts",
        "every_crash_point_classifies_without_automatic_retry",
        "round_plan_rejects_missing_foreign_duplicate_and_consumed_rounds",
        "round_plan_blocks_resume_while_an_inflight_attempt_is_unknown",
        "capability_completed_before_delivery_never_fabricates_history_or_reexecutes",
        "round_plan_authorizes_a_prepared_attempt_after_explicit_scope_recheck",
    ] {
        assert!(
            fixtures.contains(fixture),
            "P4-J7-27 fixture missing: {fixture}"
        );
    }
    for rejection in [
        "PROVIDER_RECOVERY_ROUND_SCOPE_MISMATCH",
        "PROVIDER_RECOVERY_ROUND_OBSERVATION_CONFLICT",
        "PROVIDER_RECOVERY_ROUND_INPUT_MISSING",
        "PROVIDER_RECOVERY_ROUND_RESUME_BLOCKED",
        "PROVIDER_RECOVERY_ROUND_ATTEMPT_CONSUMED",
        "PROVIDER_RECOVERY_ROUND_HISTORY_INCOMPLETE",
        "PROVIDER_RESUME_CHECK_INVALID",
    ] {
        assert!(
            fixtures.contains(rejection),
            "P4-J7-27 rejection code missing from fixtures: {rejection}"
        );
    }
    let lane = fs::read_to_string("../.github/workflows/p4-j7-27-provider-recovery.yml")
        .expect("manual provider recovery lane");
    for marker in [
        "workflow_dispatch",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test p4_j7_27_provider_recovery --locked -- --test-threads=1",
        "cargo test -p kiana-core --test p4_j7_27_provider_recovery_guard --locked -- --test-threads=1",
    ] {
        assert!(lane.contains(marker), "manual lane marker missing: {marker}");
    }
    for automatic in ["push:", "pull_request:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "manual lane must stay dispatch-only: {automatic}"
        );
    }
    let shards = fs::read_to_string("../scripts/ci/test-shards.json").expect("test shards");
    for target in [
        "p4_j7_27_provider_recovery",
        "p4_j7_27_provider_recovery_guard",
    ] {
        assert!(shards.contains(target), "auto shard missing: {target}");
    }
}
