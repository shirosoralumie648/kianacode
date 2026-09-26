use kiana_quality::{
    DeterministicEvaluator, EvidenceReceiptEvaluator, EVIDENCE_RECEIPT_INPUT_SCHEMA,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn valid_input() -> Value {
    json!({
        "schema": EVIDENCE_RECEIPT_INPUT_SCHEMA,
        "artifacts": [{
            "schema": "kiana.quality-artifact-evidence.v1",
            "artifact_ref": "artifact:one",
            "artifact_digest": DIGEST,
            "expected_digest": DIGEST,
            "digest_verified": true,
            "redacted": true,
            "source_cursor": 2,
            "provenance_ref": "provenance:one",
        }],
        "receipt": {
            "schema": "kiana.quality-receipt-evidence.v1",
            "receipt_digest": DIGEST,
            "digest_verified": true,
            "source_cursor": 2,
            "artifact_refs": ["artifact:one"],
            "provenance_ref": "provenance:one",
        },
        "assertions": [{
            "schema": "kiana.quality-receipt-assertion.v1",
            "assertion_id": "terminal-status",
            "expected": "completed",
            "actual": "completed",
            "required": true,
        }],
        "redaction": {
            "schema": "kiana.quality-redaction-evidence.v1",
            "redacted": true,
            "secret_free": true,
            "raw_payload_count": 0,
        },
        "provenance": {
            "schema": "kiana.quality-provenance-evidence.v1",
            "provenance_ref": "provenance:one",
            "source_snapshot": "snapshot:one",
            "fixture_digest": DIGEST,
            "environment_digest": DIGEST,
        },
        "source_cursor": {
            "schema": "kiana.quality-source-cursor-evidence.v1",
            "source_cursor_start": 1,
            "source_cursor_end": 2,
            "event_count": 2,
            "source_event_ids": ["event:one", "event:two"],
        },
    })
}

#[test]
fn complete_evidence_and_receipt_has_no_findings() {
    let findings = EvidenceReceiptEvaluator.evaluate(&valid_input()).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn missing_evidence_is_blocking() {
    let mut input = valid_input();
    input["artifacts"] = json!([]);
    let findings = EvidenceReceiptEvaluator.evaluate(&input).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "evidence.artifact_missing"));
}

#[test]
fn hash_redaction_provenance_cursor_and_assertion_drift_are_findings() {
    let mut input = valid_input();
    input["artifacts"][0]["artifact_digest"] = json!(OTHER_DIGEST);
    input["artifacts"][0]["digest_verified"] = json!(false);
    input["artifacts"][0]["redacted"] = json!(false);
    input["artifacts"][0]["provenance_ref"] = json!("provenance:other");
    input["receipt"]["digest_verified"] = json!(false);
    input["receipt"]["source_cursor"] = json!(1);
    input["assertions"][0]["actual"] = Value::Null;
    input["redaction"]["secret_free"] = json!(false);
    input["redaction"]["raw_payload_count"] = json!(1);
    input["source_cursor"]["source_event_ids"] = json!(["event:one", "event:one"]);

    let findings = EvidenceReceiptEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"evidence.artifact_hash_mismatch"));
    assert!(codes.contains(&"evidence.artifact_hash_unverified"));
    assert!(codes.contains(&"evidence.artifact_unredacted"));
    assert!(codes.contains(&"evidence.receipt_digest_unverified"));
    assert!(codes.contains(&"evidence.receipt_cursor_mismatch"));
    assert!(codes.contains(&"evidence.provenance_mismatch"));
    assert!(codes.contains(&"evidence.redaction_secret_detected"));
    assert!(codes.contains(&"evidence.receipt_assertion_missing"));
    assert!(codes.contains(&"evidence.source_event_id_invalid"));
}

#[test]
fn unknown_fields_and_oversized_evidence_fail_closed() {
    let mut unknown = valid_input();
    unknown["unexpected"] = json!(true);
    assert!(EvidenceReceiptEvaluator.evaluate(&unknown).is_err());

    let mut oversized = valid_input();
    oversized["artifacts"] = Value::Array(
        (0..257)
            .map(|_| valid_input()["artifacts"][0].clone())
            .collect(),
    );
    assert!(EvidenceReceiptEvaluator.evaluate(&oversized).is_err());
}
