use serde_json::Value;

#[test]
fn capability_safety_evaluator_stays_pure_and_deny_first() {
    let source = include_str!("../src/safety.rs");
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
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden capability evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "CAPABILITY_SAFETY_INPUT_SCHEMA",
        "CAPABILITY_SAFETY_EVALUATOR_ID",
        "CapabilitySafetyEvaluator",
        "SafetyObservation",
        "SafetyFinalStatus",
        "safety.action_schema_invalid",
        "safety.grant_schema_invalid",
        "safety.policy_schema_invalid",
        "safety.hook_schema_invalid",
        "safety.bound_set_invalid",
        "safety.action_identity_invalid",
        "safety.action_capability_unbound",
        "safety.grant_capability_missing",
        "safety.grant_scope_expanded",
        "safety.declared_effect_forbidden",
        "safety.policy_not_allow",
        "safety.hook_not_allow",
        "safety.effect_undeclared",
        "safety.effect_forbidden",
        "safety.effect_not_permitted",
        "safety.effect_unknown",
        "safety.effect_scope_invalid",
        "safety.effect_scope_expanded",
        "safety.effect_scope_undeclared",
        "safety.secret_effect",
        "safety.success_despite_violation",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-29 boundary marker: {required}"
        );
    }
}

#[test]
fn capability_safety_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq29_safety.rs");
    for fixture in [
        "fn fully_intersected_allow_trace_has_no_findings",
        "fn allowed_evidence_without_observed_effects_stays_clean",
        "fn safety_failure_blocks_even_with_good_final_text",
        "fn success_status_never_suppresses_a_violation",
        "fn schema_drift_on_every_evidence_section_is_rejected",
        "fn unknown_fields_and_envelope_drift_fail_closed",
        "fn grant_capability_and_scope_expansion_are_bound_to_the_missing_entry",
        "fn policy_and_hook_verdicts_must_be_allow",
        "fn declared_effects_must_stay_inside_the_grant_allowlist",
        "fn observed_effects_must_be_declared_and_allowlisted",
        "fn secret_effect_is_always_a_finding",
        "fn denied_and_unknown_observations_are_not_allowed",
        "fn effect_scope_must_stay_inside_grant_and_requested_scope",
        "fn bounded_identities_and_sets_fail_closed",
        "fn action_capability_must_be_requested",
        "fn effect_evidence_order_does_not_change_the_findings",
        "fn safety_contracts_round_trip_through_typed_input",
        "fn malformed_or_oversized_input_is_rejected_before_evaluation",
        "fn evaluation_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-29 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-29 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq29-capability-safety.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-29 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq29_safety --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq29_safety_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq29_safety --no-run --locked",
        "cargo test -p kiana-quality --test eq29_safety_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-29 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
