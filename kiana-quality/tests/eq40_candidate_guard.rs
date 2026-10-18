use serde_json::Value;

#[test]
fn candidate_binding_stays_single_dimension_and_pure() {
    let source = include_str!("../src/candidate.rs");
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "std::env",
        "tokio::",
        "reqwest::",
        "Provider",
        "EventStore",
        "EventLog",
        "DaemonHost",
        "KianaHarness",
        "CapabilityBroker",
        "ControlPlane",
        "Command::new",
        "CostLedger",
        "append_event",
        "promote",
        "authorize",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden candidate evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "CANDIDATE_SCHEMA",
        "CANDIDATE_INPUT_SCHEMA",
        "CANDIDATE_ADMISSION_INPUT_SCHEMA",
        "CANDIDATE_EVALUATOR_ID",
        "CANDIDATE_ADMISSION_EVALUATOR_ID",
        "MAX_CANDIDATE_DIMENSIONS",
        "MAX_CANDIDATE_TEXT_BYTES",
        "QualityCandidate",
        "CandidateDimension",
        "CandidateStatus",
        "CandidateInput",
        "CandidateAdmissionInput",
        "CandidateEvaluator",
        "CandidateAdmissionEvaluator",
        "evaluate_candidate_admission",
        "BaselineRecord",
        "changed_dimension",
        "changed_version_ref",
        "base_versions",
        "passive_version_refs",
        "snapshot_versions",
        "snapshot_digest",
        "candidate_digest",
        "baseline_digest",
        "suite_digest",
        "owner_id",
        "from_key",
        "valid_text",
        "valid_digest",
        "all_versions_valid",
        "deny_unknown_fields",
        "json_digest",
        "Finding::new",
        "MAX_FINDINGS",
        "BTreeMap",
        "candidate:input",
        "candidate:admission",
        "candidate_header_invalid",
        "candidate_dimension_unknown",
        "candidate_changed_dimension_missing",
        "candidate_changed_dimension_unchanged",
        "candidate_primary_dimension_invalid",
        "candidate_passive_version_drift",
        "candidate_snapshot_digest_mismatch",
        "candidate_digest_mismatch",
        "candidate_input_schema_invalid",
        "candidate_admission_input_schema_invalid",
        "candidate_admission_input_time_invalid",
        "candidate.binding_invalid",
        "candidate.baseline_invalid",
        "candidate.baseline_identity_mismatch",
        "candidate.suite_digest_mismatch",
        "candidate.baseline_not_yet_valid",
        "candidate.baseline_stale",
        "candidate.status_not_admissible",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-40 boundary marker: {required}"
        );
    }
}

#[test]
fn eq40_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq40_candidate.rs");
    for fixture in [
        "fn candidate_cannot_hide_model_or_prompt_drift",
        "fn passive_version_references_must_cover_every_unchanged_dimension",
        "fn a_second_changed_dimension_is_refused",
        "fn unknown_dimensions_and_no_op_changes_are_refused",
        "fn snapshot_or_candidate_digest_tamper_is_rejected",
        "fn rewriting_the_version_snapshot_changes_the_candidate_identity",
        "fn candidate_envelopes_and_unknown_fields_fail_closed",
        "fn admission_requires_the_bound_baseline_identity_and_digest",
        "fn admission_refuses_a_baseline_outside_its_validity_window",
        "fn admission_refuses_a_candidate_that_is_not_a_draft",
        "fn invalid_candidates_are_never_admitted",
        "fn a_single_prompt_change_is_admissible_against_its_baseline",
        "fn admission_output_is_deterministic_and_registry_wired",
        "fn typed_constructors_round_trip",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-40 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-40 fixtures must not disable a test: {disabled}"
        );
    }

    // Deny-first: the card acceptance fixture precedes the success fixture.
    let acceptance = fixtures
        .find("fn candidate_cannot_hide_model_or_prompt_drift")
        .expect("acceptance fixture");
    let success = fixtures
        .find("fn a_single_prompt_change_is_admissible_against_its_baseline")
        .expect("success fixture");
    assert!(
        acceptance < success,
        "EQ-40 fixtures must keep the deny paths before the success path"
    );

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq40-candidate.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-40 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq40_candidate --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq40_candidate_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-40 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
