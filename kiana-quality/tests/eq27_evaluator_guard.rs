use serde_json::Value;

#[test]
fn evaluator_stays_pure_and_bounded() {
    let source = include_str!("../src/evaluator.rs");
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "tokio::",
        "reqwest::",
        "Provider",
        "CapabilityBroker",
        "DaemonHost",
        "EventStore",
        "ControlPlane",
        "KianaHarness",
        "Command::new",
        "include_str!",
        "#[ignore",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "FINDING_SCHEMA",
        "deny_unknown_fields",
        "MAX_FINDINGS",
        "MAX_FINDING_VALUE_BYTES",
        "MAX_FINDING_VALUE_DEPTH",
        "MAX_FINDING_CODE_BYTES",
        "MAX_FINDING_MESSAGE_BYTES",
        "MAX_FINDING_EVIDENCE_REF_BYTES",
        "MAX_EVALUATOR_INPUT_BYTES",
        "sort_findings",
        "digest_findings",
        "validate_evidence_ref",
        "evidence_ref",
        "redact_value",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-27 boundary marker: {required}"
        );
    }

    // The fixtures pin the card's acceptance name and the rejection paths that make it true.
    let fixtures = include_str!("./eq27_evaluator.rs");
    for fixture in [
        "every_finding_has_stable_code_and_evidence",
        "every_finding_requires_a_stable_code_and_evidence_ref",
        "registry_never_returns_a_finding_without_a_stable_code_or_evidence_ref",
        "registry_limits_and_input_bounds_fail_closed",
        "finding_digest_binds_every_evidence_ref_and_value",
        "evaluator_input_is_versioned_and_bounded",
    ] {
        assert!(
            fixtures.contains(fixture),
            "EQ-27 fixture missing: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-27 fixtures must not disable a test: {disabled}"
        );
    }
}

#[test]
fn eq27_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let lane = include_str!("../../.github/workflows/eq27-evaluator.yml");
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");

    // The manual lane stays manual-only, read-only and bounded.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-27 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq27_evaluator --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq27_evaluator_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq27_evaluator --no-run --locked",
        "cargo test -p kiana-quality --test eq27_evaluator_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-27 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
