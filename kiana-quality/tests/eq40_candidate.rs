use kiana_quality::{
    CandidateDimension, CandidateEvaluator, CandidateInput, CandidateStatus,
    DeterministicEvaluator, QualityCandidate, CANDIDATE_INPUT_SCHEMA,
};
use serde_json::json;
use std::collections::BTreeMap;

const BASE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROMPT: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MODEL: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const TOOLS: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn candidate() -> QualityCandidate {
    let base_versions = BTreeMap::from([
        ("model".to_owned(), MODEL.to_owned()),
        ("prompt".to_owned(), BASE.to_owned()),
        ("tool_catalog".to_owned(), TOOLS.to_owned()),
    ]);
    let passive_version_refs = BTreeMap::from([
        ("model".to_owned(), MODEL.to_owned()),
        ("tool_catalog".to_owned(), TOOLS.to_owned()),
    ]);
    let mut snapshot_versions = base_versions.clone();
    snapshot_versions.insert("prompt".to_owned(), PROMPT.to_owned());
    let mut candidate = QualityCandidate {
        schema: "kiana.quality-candidate.v1".to_owned(),
        candidate_id: "candidate:one".to_owned(),
        baseline_id: "baseline:one".to_owned(),
        suite_digest: BASE.to_owned(),
        changed_dimension: CandidateDimension::Prompt,
        changed_version_ref: PROMPT.to_owned(),
        base_versions,
        passive_version_refs,
        snapshot_versions,
        snapshot_digest: String::new(),
        owner_id: "quality-owner".to_owned(),
        status: CandidateStatus::Draft,
        candidate_digest: String::new(),
    };
    candidate.snapshot_digest = QualityCandidate::snapshot_digest(&candidate.snapshot_versions);
    candidate.candidate_digest = candidate.digest();
    candidate
}

fn input(candidate: QualityCandidate) -> serde_json::Value {
    serde_json::to_value(CandidateInput {
        schema: CANDIDATE_INPUT_SCHEMA.to_owned(),
        candidate,
    })
    .unwrap()
}

#[test]
fn single_prompt_change_with_explicit_passive_versions_is_valid() {
    let value = input(candidate());
    let findings = CandidateEvaluator.evaluate(&value).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn candidate_cannot_hide_model_or_prompt_drift() {
    let mut value = input(candidate());
    value["candidate"]["snapshot_versions"]["model"] = json!(TOOLS);
    let findings = CandidateEvaluator.evaluate(&value).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "candidate.binding_invalid"));

    let mut changed_dimension = input(candidate());
    changed_dimension["candidate"]["passive_version_refs"]["prompt"] = json!(BASE);
    let findings = CandidateEvaluator.evaluate(&changed_dimension).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "candidate.binding_invalid"));
}

#[test]
fn snapshot_or_candidate_digest_tamper_is_rejected() {
    let mut value = input(candidate());
    value["candidate"]["snapshot_digest"] = json!(BASE);
    assert!(CandidateEvaluator
        .evaluate(&value)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "candidate.binding_invalid"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = input(candidate());
    value["unexpected"] = json!(true);
    assert!(CandidateEvaluator.evaluate(&value).is_err());
}
