//! EQ-30 evidence/receipt fixtures.
//!
//! Rejection paths come first. Every dimension named by the card - artifact hash, receipt
//! assertions, redaction, provenance and source cursor - has a dedicated rejection fixture
//! before the success paths that keep the full evidence spine green.

use kiana_quality::{
    canonical_findings, digest_findings, ArtifactEvidence, DeterministicEvaluator,
    EvaluatorRegistry, EvidenceReceiptEvaluator, EvidenceReceiptInput, Finding, ProvenanceEvidence,
    ReceiptAssertion, ReceiptEvidence, RedactionEvidence, SourceCursorEvidence,
    EVALUATION_RESULT_SCHEMA, EVIDENCE_RECEIPT_EVALUATOR_ID, EVIDENCE_RECEIPT_INPUT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const ARTIFACT_SCHEMA: &str = "kiana.quality-artifact-evidence.v1";
const RECEIPT_SCHEMA: &str = "kiana.quality-receipt-evidence.v1";
const ASSERTION_SCHEMA: &str = "kiana.quality-receipt-assertion.v1";
const REDACTION_SCHEMA: &str = "kiana.quality-redaction-evidence.v1";
const PROVENANCE_SCHEMA: &str = "kiana.quality-provenance-evidence.v1";
const CURSOR_SCHEMA: &str = "kiana.quality-source-cursor-evidence.v1";

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn codes(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

fn count(findings: &[Finding], code: &str) -> usize {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .count()
}

fn text_actuals(findings: &[Finding], code: &str) -> BTreeSet<String> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .filter_map(|finding| finding.actual.as_str().map(str::to_owned))
        .collect()
}

fn evidence_refs(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.evidence_ref.clone())
        .collect()
}

fn evaluate(input: &Value) -> Vec<Finding> {
    EvidenceReceiptEvaluator
        .evaluate(input)
        .expect("evidence receipt evidence decodes")
}

fn valid_input() -> Value {
    json!({
        "schema": EVIDENCE_RECEIPT_INPUT_SCHEMA,
        "artifacts": [{
            "schema": ARTIFACT_SCHEMA,
            "artifact_ref": "artifact:one",
            "artifact_digest": DIGEST,
            "expected_digest": DIGEST,
            "digest_verified": true,
            "redacted": true,
            "source_cursor": 2,
            "provenance_ref": "provenance:one",
        }],
        "receipt": {
            "schema": RECEIPT_SCHEMA,
            "receipt_digest": DIGEST,
            "digest_verified": true,
            "source_cursor": 2,
            "artifact_refs": ["artifact:one"],
            "provenance_ref": "provenance:one",
        },
        "assertions": [{
            "schema": ASSERTION_SCHEMA,
            "assertion_id": "terminal-status",
            "expected": "completed",
            "actual": "completed",
            "required": true,
        }],
        "redaction": {
            "schema": REDACTION_SCHEMA,
            "redacted": true,
            "secret_free": true,
            "raw_payload_count": 0,
        },
        "provenance": {
            "schema": PROVENANCE_SCHEMA,
            "provenance_ref": "provenance:one",
            "source_snapshot": "snapshot:one",
            "fixture_digest": DIGEST,
            "environment_digest": DIGEST,
        },
        "source_cursor": {
            "schema": CURSOR_SCHEMA,
            "source_cursor_start": 1,
            "source_cursor_end": 2,
            "event_count": 2,
            "source_event_ids": ["event:one", "event:two"],
        },
    })
}

/// An input that drifts every dimension the card names at once, used for the deterministic
/// and registry-wiring fixture.
fn violated_input() -> Value {
    let mut input = valid_input();
    input["artifacts"][0]["artifact_digest"] = json!(OTHER_DIGEST);
    input["artifacts"][0]["digest_verified"] = json!(false);
    input["provenance"]["fixture_digest"] = json!("sha256:not-a-digest");
    input["assertions"][0]["actual"] = json!("failed");
    input["source_cursor"]["source_event_ids"] = json!(["event:one", "event:one"]);
    input["redaction"]["secret_free"] = json!(false);
    input
}

#[test]
fn missing_evidence_is_blocking() {
    let mut input = valid_input();
    input["artifacts"] = json!([]);
    input["assertions"][0]["actual"] = Value::Null;
    input["redaction"]["redacted"] = json!(false);
    input["redaction"]["secret_free"] = json!(false);
    input["redaction"]["raw_payload_count"] = json!(1);

    let findings = evaluate(&input);
    let seen = codes(&findings);
    assert!(seen.contains("evidence.artifact_missing"));
    assert_eq!(
        text_actuals(&findings, "evidence.artifact_missing"),
        set(&["artifact:one"])
    );
    assert_eq!(
        text_actuals(&findings, "evidence.receipt_assertion_missing"),
        set(&["missing"])
    );
    assert!(seen.contains("evidence.redaction_missing"));
    assert!(seen.contains("evidence.redaction_secret_detected"));
    assert_eq!(
        evidence_refs(&findings)
            .into_iter()
            .filter(|reference| reference == "evidence:receipt")
            .count(),
        1
    );

    // A receipt section that is absent (not merely empty) is denied before evaluation.
    let mut missing_receipt = valid_input();
    missing_receipt.as_object_mut().unwrap().remove("receipt");
    assert!(EvidenceReceiptEvaluator.evaluate(&missing_receipt).is_err());
}

#[test]
fn hash_drift_and_unverified_digests_are_rejected() {
    let mut input = valid_input();
    input["artifacts"][0]["artifact_digest"] = json!(OTHER_DIGEST);
    input["artifacts"][0]["digest_verified"] = json!(false);
    input["receipt"]["receipt_digest"] = json!("sha256:short");
    input["receipt"]["digest_verified"] = json!(false);

    let findings = evaluate(&input);
    let seen = codes(&findings);
    assert!(seen.contains("evidence.artifact_hash_mismatch"));
    assert!(seen.contains("evidence.artifact_hash_unverified"));
    assert!(seen.contains("evidence.receipt_digest_invalid"));
    assert!(seen.contains("evidence.receipt_digest_unverified"));
    assert_eq!(count(&findings, "evidence.artifact_hash_mismatch"), 1);

    // An unparsable digest pair is rejected once as invalid instead of also reporting a
    // second, redundant mismatch.
    let mut invalid_pair = valid_input();
    invalid_pair["artifacts"][0]["expected_digest"] = json!("sha256:zz");
    let findings = evaluate(&invalid_pair);
    assert_eq!(count(&findings, "evidence.artifact_hash_invalid"), 1);
    assert_eq!(count(&findings, "evidence.artifact_hash_mismatch"), 0);
}

#[test]
fn receipt_assertions_must_be_present_and_match() {
    let mut missing = valid_input();
    missing["assertions"][0]["actual"] = Value::Null;
    let findings = evaluate(&missing);
    assert_eq!(count(&findings, "evidence.receipt_assertion_missing"), 1);
    let missing_finding = findings
        .iter()
        .find(|finding| finding.code == "evidence.receipt_assertion_missing")
        .unwrap();
    assert_eq!(missing_finding.expected, json!("completed"));
    assert_eq!(missing_finding.actual, json!("missing"));
    assert_eq!(missing_finding.evidence_ref, "evidence:assertion:0");

    let mut mismatched = valid_input();
    mismatched["assertions"][0]["actual"] = json!("failed");
    let findings = evaluate(&mismatched);
    assert_eq!(count(&findings, "evidence.receipt_assertion_mismatch"), 1);
    let mismatch = findings
        .iter()
        .find(|finding| finding.code == "evidence.receipt_assertion_mismatch")
        .unwrap();
    assert_eq!(mismatch.expected, json!("completed"));
    assert_eq!(mismatch.actual, json!("failed"));

    // An optional assertion may stay absent, and a matching assertion never reports.
    let mut optional = valid_input();
    optional["assertions"][0]["actual"] = Value::Null;
    optional["assertions"][0]["required"] = json!(false);
    assert!(evaluate(&optional).is_empty());
}

#[test]
fn redaction_and_secret_free_status_are_enforced() {
    let mut unredacted_artifact = valid_input();
    unredacted_artifact["artifacts"][0]["redacted"] = json!(false);
    assert_eq!(
        count(
            &evaluate(&unredacted_artifact),
            "evidence.artifact_unredacted"
        ),
        1
    );

    let mut unredacted_payload = valid_input();
    unredacted_payload["redaction"]["redacted"] = json!(false);
    assert_eq!(
        count(&evaluate(&unredacted_payload), "evidence.redaction_missing"),
        1
    );

    let mut secret = valid_input();
    secret["redaction"]["secret_free"] = json!(false);
    assert_eq!(
        count(&evaluate(&secret), "evidence.redaction_secret_detected"),
        1
    );

    let mut raw_payload = valid_input();
    raw_payload["redaction"]["raw_payload_count"] = json!(2);
    assert_eq!(
        count(
            &evaluate(&raw_payload),
            "evidence.redaction_secret_detected"
        ),
        1
    );
}

#[test]
fn provenance_drift_is_bound_to_the_mismatching_entry() {
    let mut artifact_drift = valid_input();
    artifact_drift["artifacts"][0]["provenance_ref"] = json!("provenance:other");
    let findings = evaluate(&artifact_drift);
    assert_eq!(count(&findings, "evidence.provenance_mismatch"), 1);
    assert_eq!(
        text_actuals(&findings, "evidence.provenance_mismatch"),
        set(&["provenance:other"])
    );
    assert_eq!(
        findings
            .iter()
            .find(|finding| finding.code == "evidence.provenance_mismatch")
            .unwrap()
            .evidence_ref,
        "evidence:artifact:0"
    );

    let mut receipt_drift = valid_input();
    receipt_drift["receipt"]["provenance_ref"] = json!("provenance:other");
    let findings = evaluate(&receipt_drift);
    assert_eq!(count(&findings, "evidence.provenance_mismatch"), 1);
    assert_eq!(
        findings
            .iter()
            .find(|finding| finding.code == "evidence.provenance_mismatch")
            .unwrap()
            .evidence_ref,
        "evidence:receipt"
    );

    let mut both_drift = valid_input();
    both_drift["artifacts"][0]["provenance_ref"] = json!("provenance:other");
    both_drift["receipt"]["provenance_ref"] = json!("provenance:other");
    assert_eq!(
        count(&evaluate(&both_drift), "evidence.provenance_mismatch"),
        2
    );

    let mut missing = valid_input();
    missing["provenance"]["provenance_ref"] = json!("");
    missing["provenance"]["source_snapshot"] = json!("");
    assert_eq!(count(&evaluate(&missing), "evidence.provenance_missing"), 1);

    let mut invalid = valid_input();
    invalid["provenance"]["fixture_digest"] = json!("sha256:nothex");
    invalid["provenance"]["environment_digest"] = json!("md5:abcd");
    assert_eq!(count(&evaluate(&invalid), "evidence.provenance_invalid"), 1);
}

#[test]
fn source_cursor_range_and_event_ids_are_bounded() {
    let mut zero_start = valid_input();
    zero_start["source_cursor"]["source_cursor_start"] = json!(0);
    assert!(codes(&evaluate(&zero_start)).contains("evidence.source_cursor_invalid"));

    let mut inverted = valid_input();
    inverted["source_cursor"]["source_cursor_start"] = json!(5);
    inverted["source_cursor"]["source_cursor_end"] = json!(4);
    inverted["receipt"]["source_cursor"] = json!(4);
    assert!(codes(&evaluate(&inverted)).contains("evidence.source_cursor_invalid"));

    let mut count_mismatch = valid_input();
    count_mismatch["source_cursor"]["event_count"] = json!(3);
    assert!(codes(&evaluate(&count_mismatch)).contains("evidence.source_cursor_invalid"));

    let mut zero_events = valid_input();
    zero_events["source_cursor"]["event_count"] = json!(0);
    zero_events["source_cursor"]["source_event_ids"] = json!([]);
    assert!(codes(&evaluate(&zero_events)).contains("evidence.source_cursor_invalid"));

    // The range must be wide enough to actually contain the declared event count.
    let mut too_narrow = valid_input();
    too_narrow["source_cursor"]["event_count"] = json!(3);
    too_narrow["source_cursor"]["source_event_ids"] =
        json!(["event:one", "event:two", "event:three"]);
    assert!(codes(&evaluate(&too_narrow)).contains("evidence.source_cursor_invalid"));

    let mut duplicate = valid_input();
    duplicate["source_cursor"]["source_event_ids"] = json!(["event:one", "event:one"]);
    let findings = evaluate(&duplicate);
    assert_eq!(count(&findings, "evidence.source_event_id_invalid"), 1);
    assert_eq!(
        text_actuals(&findings, "evidence.source_event_id_invalid"),
        set(&["event:one"])
    );

    let mut unbounded = valid_input();
    unbounded["source_cursor"]["source_event_ids"] = json!(["event one", "event:two"]);
    assert_eq!(
        count(&evaluate(&unbounded), "evidence.source_event_id_invalid"),
        1
    );
}

#[test]
fn artifact_cursor_and_receipt_cursor_must_stay_inside_the_source_range() {
    let mut outside = valid_input();
    outside["receipt"]["source_cursor"] = json!(1);
    let findings = evaluate(&outside);
    assert_eq!(count(&findings, "evidence.receipt_cursor_mismatch"), 1);

    let mut zero_cursor = valid_input();
    zero_cursor["artifacts"][0]["source_cursor"] = json!(0);
    zero_cursor["receipt"]["artifact_refs"] = json!(["artifact:one"]);
    assert_eq!(
        count(&evaluate(&zero_cursor), "evidence.artifact_cursor_invalid"),
        1
    );

    let mut beyond_end = valid_input();
    beyond_end["artifacts"][0]["source_cursor"] = json!(3);
    assert_eq!(
        count(&evaluate(&beyond_end), "evidence.artifact_cursor_invalid"),
        1
    );
}

#[test]
fn artifact_evidence_must_be_linked_from_the_receipt() {
    let mut missing = valid_input();
    missing["receipt"]["artifact_refs"] = json!(["artifact:one", "artifact:missing"]);
    let findings = evaluate(&missing);
    assert_eq!(count(&findings, "evidence.artifact_missing"), 1);
    assert_eq!(
        text_actuals(&findings, "evidence.artifact_missing"),
        set(&["artifact:missing"])
    );

    let mut extra = valid_input();
    let first = extra["artifacts"][0].clone();
    let mut second = first.clone();
    second["artifact_ref"] = json!("artifact:two");
    extra["artifacts"] = json!([first, second]);
    let findings = evaluate(&extra);
    assert_eq!(count(&findings, "evidence.artifact_unreferenced"), 1);
    assert_eq!(
        text_actuals(&findings, "evidence.artifact_unreferenced"),
        set(&["artifact:two"])
    );
    assert!(evidence_refs(&findings).contains("evidence:artifact:1"));

    let mut duplicate = valid_input();
    let artifact = duplicate["artifacts"][0].clone();
    duplicate["artifacts"] = json!([artifact.clone(), artifact]);
    let findings = evaluate(&duplicate);
    assert_eq!(count(&findings, "evidence.artifact_ref_invalid"), 1);
    assert_eq!(count(&findings, "evidence.artifact_missing"), 0);

    let mut invalid_receipt_ref = valid_input();
    invalid_receipt_ref["receipt"]["artifact_refs"] = json!([""]);
    let findings = evaluate(&invalid_receipt_ref);
    assert!(codes(&findings).contains("evidence.artifact_ref_invalid"));
}

#[test]
fn schema_drift_on_every_evidence_section_is_rejected() {
    let mut input = valid_input();
    input["provenance"]["schema"] = json!("kiana.quality-provenance-evidence.v0");
    input["source_cursor"]["schema"] = json!("kiana.quality-source-cursor-evidence.v0");
    input["receipt"]["schema"] = json!("kiana.quality-receipt-evidence.v0");
    input["redaction"]["schema"] = json!("kiana.quality-redaction-evidence.v0");
    input["artifacts"][0]["schema"] = json!("kiana.quality-artifact-evidence.v0");
    input["assertions"][0]["schema"] = json!("kiana.quality-receipt-assertion.v0");

    let findings = evaluate(&input);
    let seen = codes(&findings);
    for code in [
        "evidence.provenance_schema_invalid",
        "evidence.cursor_schema_invalid",
        "evidence.receipt_schema_invalid",
        "evidence.redaction_schema_invalid",
        "evidence.artifact_schema_invalid",
        "evidence.assertion_schema_invalid",
    ] {
        assert_eq!(count(&findings, code), 1, "missing schema drift: {code}");
        assert!(seen.contains(code));
    }
}

#[test]
fn unknown_fields_and_envelope_drift_fail_closed() {
    let mut unknown_envelope = valid_input();
    unknown_envelope["unexpected"] = json!(true);
    assert!(EvidenceReceiptEvaluator
        .evaluate(&unknown_envelope)
        .is_err());

    let mut unknown_artifact = valid_input();
    unknown_artifact["artifacts"][0]["unexpected"] = json!(true);
    assert!(EvidenceReceiptEvaluator
        .evaluate(&unknown_artifact)
        .is_err());

    let mut unknown_assertion = valid_input();
    unknown_assertion["assertions"][0]["unexpected"] = json!(true);
    assert!(EvidenceReceiptEvaluator
        .evaluate(&unknown_assertion)
        .is_err());

    let mut wrong_schema = valid_input();
    wrong_schema["schema"] = json!("kiana.quality-evidence-receipt-input.v0");
    assert!(EvidenceReceiptEvaluator.evaluate(&wrong_schema).is_err());

    let mut missing_receipt = valid_input();
    missing_receipt.as_object_mut().unwrap().remove("receipt");
    assert!(EvidenceReceiptEvaluator.evaluate(&missing_receipt).is_err());

    let mut artifacts_not_an_array = valid_input();
    artifacts_not_an_array["artifacts"] = json!("artifact:one");
    assert!(EvidenceReceiptEvaluator
        .evaluate(&artifacts_not_an_array)
        .is_err());

    let mut assertions_not_an_array = valid_input();
    assertions_not_an_array["assertions"] = json!({"assertion_id": "terminal-status"});
    assert!(EvidenceReceiptEvaluator
        .evaluate(&assertions_not_an_array)
        .is_err());

    let mut event_ids_not_an_array = valid_input();
    event_ids_not_an_array["source_cursor"]["source_event_ids"] = json!("event:one");
    assert!(EvidenceReceiptEvaluator
        .evaluate(&event_ids_not_an_array)
        .is_err());

    let mut verified_wrong_type = valid_input();
    verified_wrong_type["receipt"]["digest_verified"] = json!("true");
    assert!(EvidenceReceiptEvaluator
        .evaluate(&verified_wrong_type)
        .is_err());

    let mut redaction_wrong_shape = valid_input();
    redaction_wrong_shape["redaction"] = json!(["redacted"]);
    assert!(EvidenceReceiptEvaluator
        .evaluate(&redaction_wrong_shape)
        .is_err());

    assert!(EvidenceReceiptEvaluator.evaluate(&json!([])).is_err());
    assert!(EvidenceReceiptEvaluator
        .evaluate(&json!("evidence"))
        .is_err());
}

#[test]
fn bounded_references_and_assertion_values_fail_closed() {
    let mut spaced_reference = valid_input();
    spaced_reference["artifacts"][0]["artifact_ref"] = json!("artifact one");
    assert_eq!(
        count(
            &evaluate(&spaced_reference),
            "evidence.artifact_ref_invalid"
        ),
        1
    );

    let mut oversized_reference = valid_input();
    oversized_reference["artifacts"][0]["artifact_ref"] = json!("a".repeat(513));
    assert_eq!(
        count(
            &evaluate(&oversized_reference),
            "evidence.artifact_ref_invalid"
        ),
        1
    );

    let mut duplicate_assertions = valid_input();
    let assertion = duplicate_assertions["assertions"][0].clone();
    duplicate_assertions["assertions"] = json!([assertion.clone(), assertion]);
    assert_eq!(
        count(
            &evaluate(&duplicate_assertions),
            "evidence.assertion_invalid"
        ),
        1
    );

    let mut oversized_expected = valid_input();
    oversized_expected["assertions"][0]["expected"] = json!("x".repeat(600));
    assert_eq!(
        count(&evaluate(&oversized_expected), "evidence.assertion_invalid"),
        1
    );

    let mut control_actual = valid_input();
    control_actual["assertions"][0]["actual"] = json!("a\u{7}b");
    assert_eq!(
        count(&evaluate(&control_actual), "evidence.assertion_invalid"),
        1
    );

    let mut empty_assertion_id = valid_input();
    empty_assertion_id["assertions"][0]["assertion_id"] = json!("");
    assert_eq!(
        count(&evaluate(&empty_assertion_id), "evidence.assertion_invalid"),
        1
    );

    let mut empty_event_id = valid_input();
    empty_event_id["source_cursor"]["source_event_ids"] = json!(["", "event:two"]);
    assert_eq!(
        count(
            &evaluate(&empty_event_id),
            "evidence.source_event_id_invalid"
        ),
        1
    );
}

#[test]
fn oversized_evidence_fails_closed_before_evaluation() {
    let mut too_many_artifacts = valid_input();
    too_many_artifacts["artifacts"] = Value::Array(
        (0..257)
            .map(|_| valid_input()["artifacts"][0].clone())
            .collect(),
    );
    assert!(EvidenceReceiptEvaluator
        .evaluate(&too_many_artifacts)
        .is_err());

    let mut too_many_assertions = valid_input();
    too_many_assertions["assertions"] = Value::Array(
        (0..257)
            .map(|_| valid_input()["assertions"][0].clone())
            .collect(),
    );
    assert!(EvidenceReceiptEvaluator
        .evaluate(&too_many_assertions)
        .is_err());

    let mut too_many_events = valid_input();
    let event_ids: Vec<String> = (0..1_025).map(|index| format!("event:{index}")).collect();
    too_many_events["source_cursor"]["event_count"] = json!(1_025);
    too_many_events["source_cursor"]["source_event_ids"] = json!(event_ids);
    assert!(EvidenceReceiptEvaluator.evaluate(&too_many_events).is_err());
}

#[test]
fn evidence_order_does_not_change_the_findings() {
    let mut input = valid_input();
    let first_artifact = input["artifacts"][0].clone();
    let mut second_artifact = first_artifact.clone();
    second_artifact["artifact_ref"] = json!("artifact:two");
    second_artifact["source_cursor"] = json!(1);
    input["artifacts"] = json!([first_artifact, second_artifact]);
    let first_assertion = input["assertions"][0].clone();
    let second_assertion = json!({
        "schema": ASSERTION_SCHEMA,
        "assertion_id": "artifact-count",
        "expected": "2",
        "actual": "1",
        "required": true,
    });
    input["assertions"] = json!([first_assertion, second_assertion.clone()]);

    let mut reversed = input.clone();
    let mut artifacts = reversed["artifacts"].as_array().unwrap().clone();
    artifacts.reverse();
    reversed["artifacts"] = json!(artifacts);
    let mut assertions = reversed["assertions"].as_array().unwrap().clone();
    assertions.reverse();
    reversed["assertions"] = json!(assertions);

    let first_findings = canonical_findings(evaluate(&input)).unwrap();
    let second_findings = canonical_findings(evaluate(&reversed)).unwrap();
    assert_eq!(first_findings, second_findings);
    assert_eq!(
        text_actuals(&first_findings, "evidence.receipt_assertion_mismatch"),
        set(&["1"])
    );
    assert_eq!(
        text_actuals(&first_findings, "evidence.artifact_unreferenced"),
        set(&["artifact:two"])
    );

    // Duplicate evidence stays duplicated and visible under distinct evidence references.
    let mut duplicate = valid_input();
    let artifact = duplicate["artifacts"][0].clone();
    duplicate["artifacts"] = json!([artifact.clone(), artifact]);
    let findings = evaluate(&duplicate);
    assert_eq!(count(&findings, "evidence.artifact_ref_invalid"), 1);
    assert!(evidence_refs(&findings).contains("evidence:artifact:0"));
    assert!(evidence_refs(&findings).contains("evidence:artifact:1"));
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let mut input = valid_input();
    input["assertions"][0]["actual"] = json!("token=supersecret");
    let findings = evaluate(&input);
    let mismatch = findings
        .iter()
        .find(|finding| finding.code == "evidence.receipt_assertion_mismatch")
        .unwrap();
    assert_eq!(mismatch.actual, json!("token=[REDACTED]"));
    assert!(!mismatch.actual.to_string().contains("supersecret"));

    let mut reference = valid_input();
    reference["artifacts"][0]["artifact_ref"] = json!("token=supersecret");
    let findings = evaluate(&reference);
    let invalid = findings
        .iter()
        .find(|finding| finding.code == "evidence.artifact_ref_invalid")
        .unwrap();
    assert_eq!(invalid.actual, json!("token=[REDACTED]"));
    assert!(!invalid.actual.to_string().contains("supersecret"));
}

#[test]
fn complete_evidence_and_receipt_has_no_findings() {
    let findings = evaluate(&valid_input());
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn evidence_contracts_round_trip_through_typed_input() {
    let typed = EvidenceReceiptInput::new(
        vec![ArtifactEvidence {
            schema: ARTIFACT_SCHEMA.to_owned(),
            artifact_ref: "artifact:one".to_owned(),
            artifact_digest: DIGEST.to_owned(),
            expected_digest: DIGEST.to_owned(),
            digest_verified: true,
            redacted: true,
            source_cursor: 2,
            provenance_ref: "provenance:one".to_owned(),
        }],
        ReceiptEvidence {
            schema: RECEIPT_SCHEMA.to_owned(),
            receipt_digest: DIGEST.to_owned(),
            digest_verified: true,
            source_cursor: 2,
            artifact_refs: set(&["artifact:one"]),
            provenance_ref: "provenance:one".to_owned(),
        },
        vec![ReceiptAssertion {
            schema: ASSERTION_SCHEMA.to_owned(),
            assertion_id: "terminal-status".to_owned(),
            expected: "completed".to_owned(),
            actual: Some("completed".to_owned()),
            required: true,
        }],
        RedactionEvidence {
            schema: REDACTION_SCHEMA.to_owned(),
            redacted: true,
            secret_free: true,
            raw_payload_count: 0,
        },
        ProvenanceEvidence {
            schema: PROVENANCE_SCHEMA.to_owned(),
            provenance_ref: "provenance:one".to_owned(),
            source_snapshot: "snapshot:one".to_owned(),
            fixture_digest: DIGEST.to_owned(),
            environment_digest: DIGEST.to_owned(),
        },
        SourceCursorEvidence {
            schema: CURSOR_SCHEMA.to_owned(),
            source_cursor_start: 1,
            source_cursor_end: 2,
            event_count: 2,
            source_event_ids: vec!["event:one".to_owned(), "event:two".to_owned()],
        },
    );
    assert_eq!(typed.validate(), Ok(()));
    let value = typed.as_value();
    assert_eq!(value["schema"], json!(EVIDENCE_RECEIPT_INPUT_SCHEMA));
    let decoded: EvidenceReceiptInput = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, typed);
    assert!(EvidenceReceiptEvaluator
        .evaluate(&value)
        .unwrap()
        .is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-evidence-receipt-input.v0");
    assert!(EvidenceReceiptEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn evidence_evaluator_output_is_deterministic_evidence_bound_and_registry_wired() {
    let input = violated_input();
    let first = evaluate(&input);
    let second = evaluate(&input);
    assert_eq!(first, second);
    let digest = digest_findings(&first).unwrap();
    assert_eq!(digest, digest_findings(&second).unwrap());

    for finding in &first {
        assert!(
            finding.code.starts_with("evidence."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("evidence:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }
    assert!(evidence_refs(&first).contains("evidence:artifact:0"));
    assert!(evidence_refs(&first).contains("evidence:assertion:0"));

    let mut registry = EvaluatorRegistry::new();
    registry.register(EvidenceReceiptEvaluator).unwrap();
    assert_eq!(
        registry.evaluator_ids(),
        vec![EVIDENCE_RECEIPT_EVALUATOR_ID.to_owned()]
    );
    let result = registry.evaluate_with_digest(&input).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical_findings(first.clone()).unwrap());
    assert_eq!(
        result.findings_digest,
        digest_findings(&canonical_findings(first).unwrap()).unwrap()
    );

    // An unclean envelope never reaches the evaluator through the registry either.
    let mut unknown_field = input;
    unknown_field["unexpected"] = json!(true);
    assert!(registry.evaluate(&unknown_field).is_err());
}
