use serde_json::Value;

#[test]
fn evidence_receipt_evaluator_stays_pure_and_fail_closed() {
    let source = include_str!("../src/evidence.rs");
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
            "forbidden evidence evaluator dependency: {forbidden}"
        );
    }
    for required in [
        "EVIDENCE_RECEIPT_INPUT_SCHEMA",
        "EVIDENCE_RECEIPT_EVALUATOR_ID",
        "EvidenceReceiptEvaluator",
        "MAX_ARTIFACTS",
        "MAX_ASSERTIONS",
        "MAX_SOURCE_EVENT_IDS",
        "evidence.artifact_schema_invalid",
        "evidence.assertion_schema_invalid",
        "evidence.cursor_schema_invalid",
        "evidence.provenance_schema_invalid",
        "evidence.receipt_schema_invalid",
        "evidence.redaction_schema_invalid",
        "evidence.artifact_hash_invalid",
        "evidence.artifact_hash_mismatch",
        "evidence.artifact_hash_unverified",
        "evidence.artifact_unredacted",
        "evidence.artifact_cursor_invalid",
        "evidence.artifact_ref_invalid",
        "evidence.artifact_missing",
        "evidence.artifact_unreferenced",
        "evidence.receipt_digest_invalid",
        "evidence.receipt_digest_unverified",
        "evidence.receipt_cursor_mismatch",
        "evidence.provenance_missing",
        "evidence.provenance_invalid",
        "evidence.provenance_mismatch",
        "evidence.source_cursor_invalid",
        "evidence.source_event_id_invalid",
        "evidence.receipt_assertion_mismatch",
        "evidence.receipt_assertion_missing",
        "evidence.assertion_invalid",
        "evidence.redaction_missing",
        "evidence.redaction_secret_detected",
        "evidence:provenance",
        "evidence:source-cursor",
        "evidence:receipt",
        "evidence:redaction",
        "evidence:artifact:",
        "evidence:assertion:",
        "is_bounded_reference",
        "is_sha256_digest",
        "redact_text",
        "Finding::redacted",
        "sha256:",
        "MAX_FINDINGS",
    ] {
        assert!(
            source.contains(required),
            "missing EQ-30 boundary marker: {required}"
        );
    }
}

#[test]
fn eq30_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shard() {
    let fixtures = include_str!("./eq30_evidence.rs");
    for fixture in [
        "fn missing_evidence_is_blocking",
        "fn hash_drift_and_unverified_digests_are_rejected",
        "fn receipt_assertions_must_be_present_and_match",
        "fn redaction_and_secret_free_status_are_enforced",
        "fn provenance_drift_is_bound_to_the_mismatching_entry",
        "fn source_cursor_range_and_event_ids_are_bounded",
        "fn artifact_cursor_and_receipt_cursor_must_stay_inside_the_source_range",
        "fn artifact_evidence_must_be_linked_from_the_receipt",
        "fn schema_drift_on_every_evidence_section_is_rejected",
        "fn unknown_fields_and_envelope_drift_fail_closed",
        "fn bounded_references_and_assertion_values_fail_closed",
        "fn oversized_evidence_fails_closed_before_evaluation",
        "fn evidence_order_does_not_change_the_findings",
        "fn redaction_boundary_covers_caller_supplied_values",
        "fn complete_evidence_and_receipt_has_no_findings",
        "fn evidence_contracts_round_trip_through_typed_input",
        "fn evidence_evaluator_output_is_deterministic_evidence_bound_and_registry_wired",
    ] {
        assert!(
            fixtures.contains(fixture),
            "missing EQ-30 fixture: {fixture}"
        );
    }
    for disabled in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(disabled),
            "EQ-30 fixtures must not disable a test: {disabled}"
        );
    }

    // The manual lane stays manual-only, read-only, bounded and fully compiled.
    let lane = include_str!("../../.github/workflows/eq30-evidence-receipt.yml");
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("contents: read"));
    for automatic in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(automatic),
            "EQ-30 lane must stay manual-only: {automatic}"
        );
    }
    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-quality --test eq30_evidence --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq30_evidence_guard --locked -- --test-threads=1",
        "cargo test -p kiana-quality --test eq30_evidence --no-run --locked",
        "cargo test -p kiana-quality --test eq30_evidence_guard --no-run --locked",
    ] {
        assert!(lane.contains(step), "EQ-30 lane step missing: {step}");
    }

    // `targets: null` keeps both targets in the automatic kiana-quality shard, so the
    // unified ci.yml discovers them without a separate registration.
    let shards: Value = serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
        .expect("test shard matrix");
    assert_eq!(shards["kiana-quality"]["crate"], "kiana-quality");
    assert_eq!(shards["kiana-quality"]["targets"], Value::Null);
}
