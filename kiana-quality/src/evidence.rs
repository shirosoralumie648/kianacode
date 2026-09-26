//! EQ-30 evidence and receipt evaluation over caller-supplied, redacted evidence.
//!
//! This module validates the evidence contract and cross-links only. It does not read an
//! artifact storage adapter, query a Receipt projection, recompute a durable fact, or turn a finding into a
//! release or authorization decision.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EVIDENCE_RECEIPT_INPUT_SCHEMA: &str = "kiana.quality-evidence-receipt-input.v1";
pub const EVIDENCE_RECEIPT_EVALUATOR_ID: &str = "evidence-receipt";
const ARTIFACT_SCHEMA: &str = "kiana.quality-artifact-evidence.v1";
const RECEIPT_SCHEMA: &str = "kiana.quality-receipt-evidence.v1";
const ASSERTION_SCHEMA: &str = "kiana.quality-receipt-assertion.v1";
const REDACTION_SCHEMA: &str = "kiana.quality-redaction-evidence.v1";
const PROVENANCE_SCHEMA: &str = "kiana.quality-provenance-evidence.v1";
const CURSOR_SCHEMA: &str = "kiana.quality-source-cursor-evidence.v1";
const MAX_ENTRIES: usize = 256;
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactEvidence {
    pub schema: String,
    pub artifact_ref: String,
    pub artifact_digest: String,
    pub expected_digest: String,
    pub digest_verified: bool,
    pub redacted: bool,
    pub source_cursor: u64,
    pub provenance_ref: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptEvidence {
    pub schema: String,
    pub receipt_digest: String,
    pub digest_verified: bool,
    pub source_cursor: u64,
    pub artifact_refs: BTreeSet<String>,
    pub provenance_ref: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAssertion {
    pub schema: String,
    pub assertion_id: String,
    pub expected: String,
    #[serde(default)]
    pub actual: Option<String>,
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionEvidence {
    pub schema: String,
    pub redacted: bool,
    pub secret_free: bool,
    pub raw_payload_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceEvidence {
    pub schema: String,
    pub provenance_ref: String,
    pub source_snapshot: String,
    pub fixture_digest: String,
    pub environment_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCursorEvidence {
    pub schema: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub event_count: u64,
    pub source_event_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReceiptInput {
    pub schema: String,
    pub artifacts: Vec<ArtifactEvidence>,
    pub receipt: ReceiptEvidence,
    pub assertions: Vec<ReceiptAssertion>,
    pub redaction: RedactionEvidence,
    pub provenance: ProvenanceEvidence,
    pub source_cursor: SourceCursorEvidence,
}

impl EvidenceReceiptInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != EVIDENCE_RECEIPT_INPUT_SCHEMA {
            return Err("evidence_receipt_input_schema_invalid");
        }
        if self.artifacts.len() > MAX_ENTRIES
            || self.assertions.len() > MAX_ENTRIES
            || self.source_cursor.source_event_ids.len() > MAX_ENTRIES
        {
            return Err("evidence_receipt_input_limit_exceeded");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EvidenceReceiptEvaluator;

impl DeterministicEvaluator for EvidenceReceiptEvaluator {
    fn evaluator_id(&self) -> &str {
        EVIDENCE_RECEIPT_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: EvidenceReceiptInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("evidence_receipt"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_evidence_receipt(&decoded)
    }
}

pub fn evaluate_evidence_receipt(
    input: &EvidenceReceiptInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let cursor = &input.source_cursor;
    let provenance = &input.provenance;
    let receipt = &input.receipt;

    check_schema(
        &mut findings,
        "evidence.provenance_schema_invalid",
        &provenance.schema,
        PROVENANCE_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "evidence.cursor_schema_invalid",
        &cursor.schema,
        CURSOR_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "evidence.receipt_schema_invalid",
        &receipt.schema,
        RECEIPT_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "evidence.redaction_schema_invalid",
        &input.redaction.schema,
        REDACTION_SCHEMA,
    )?;

    if !valid_reference(&provenance.provenance_ref) || !valid_reference(&provenance.source_snapshot)
    {
        emit(
            &mut findings,
            "evidence.provenance_missing",
            "bounded_provenance_and_snapshot",
            "missing",
        )?;
    }
    if !valid_digest(&provenance.fixture_digest) || !valid_digest(&provenance.environment_digest) {
        emit(
            &mut findings,
            "evidence.provenance_invalid",
            "sha256_fixture_and_environment",
            "invalid",
        )?;
    }

    if cursor.source_cursor_start == 0 || cursor.source_cursor_start > cursor.source_cursor_end {
        emit(
            &mut findings,
            "evidence.source_cursor_invalid",
            "positive_ordered_cursor_range",
            "invalid",
        )?;
    }
    if cursor.event_count == 0 || cursor.event_count as usize != cursor.source_event_ids.len() {
        emit(
            &mut findings,
            "evidence.source_cursor_invalid",
            "event_count_matches_event_ids",
            "mismatch",
        )?;
    }
    let mut source_event_ids = BTreeSet::new();
    for event_id in &cursor.source_event_ids {
        if !valid_reference(event_id) || !source_event_ids.insert(event_id) {
            emit(
                &mut findings,
                "evidence.source_event_id_invalid",
                "unique_bounded_event_ids",
                "invalid_or_duplicate",
            )?;
        }
    }

    if !valid_digest(&receipt.receipt_digest) {
        emit(
            &mut findings,
            "evidence.receipt_digest_invalid",
            "sha256_digest",
            "invalid",
        )?;
    }
    if !receipt.digest_verified {
        emit(
            &mut findings,
            "evidence.receipt_digest_unverified",
            "verified",
            "unverified",
        )?;
    }
    if receipt.source_cursor != cursor.source_cursor_end {
        emit(
            &mut findings,
            "evidence.receipt_cursor_mismatch",
            "receipt_at_source_cursor_end",
            "mismatch",
        )?;
    }
    if receipt.provenance_ref != provenance.provenance_ref {
        emit(
            &mut findings,
            "evidence.provenance_mismatch",
            "shared_provenance_ref",
            "mismatch",
        )?;
    }
    for artifact_ref in &receipt.artifact_refs {
        if !valid_reference(artifact_ref) {
            emit(
                &mut findings,
                "evidence.artifact_ref_invalid",
                "bounded_artifact_ref",
                "invalid",
            )?;
        }
    }

    let mut artifact_refs = BTreeSet::new();
    for artifact in &input.artifacts {
        check_schema(
            &mut findings,
            "evidence.artifact_schema_invalid",
            &artifact.schema,
            ARTIFACT_SCHEMA,
        )?;
        if !valid_reference(&artifact.artifact_ref) || !artifact_refs.insert(&artifact.artifact_ref)
        {
            emit(
                &mut findings,
                "evidence.artifact_ref_invalid",
                "unique_bounded_artifact_ref",
                "invalid_or_duplicate",
            )?;
        }
        if !valid_digest(&artifact.artifact_digest) || !valid_digest(&artifact.expected_digest) {
            emit(
                &mut findings,
                "evidence.artifact_hash_invalid",
                "sha256_digest_pair",
                "invalid",
            )?;
        }
        if artifact.artifact_digest != artifact.expected_digest {
            emit(
                &mut findings,
                "evidence.artifact_hash_mismatch",
                "expected_digest",
                "mismatch",
            )?;
        }
        if !artifact.digest_verified {
            emit(
                &mut findings,
                "evidence.artifact_hash_unverified",
                "verified",
                "unverified",
            )?;
        }
        if !artifact.redacted {
            emit(
                &mut findings,
                "evidence.artifact_unredacted",
                "redacted",
                "raw_or_unknown",
            )?;
        }
        if artifact.source_cursor == 0
            || artifact.source_cursor < cursor.source_cursor_start
            || artifact.source_cursor > cursor.source_cursor_end
        {
            emit(
                &mut findings,
                "evidence.artifact_cursor_invalid",
                "cursor_inside_source_range",
                "outside",
            )?;
        }
        if artifact.provenance_ref != provenance.provenance_ref {
            emit(
                &mut findings,
                "evidence.provenance_mismatch",
                "shared_provenance_ref",
                "artifact_mismatch",
            )?;
        }
    }
    for artifact_ref in &receipt.artifact_refs {
        if !artifact_refs.contains(artifact_ref) {
            emit(
                &mut findings,
                "evidence.artifact_missing",
                "receipt_artifact_has_evidence",
                "missing",
            )?;
        }
    }

    if !input.redaction.redacted {
        emit(
            &mut findings,
            "evidence.redaction_missing",
            "redacted",
            "raw_or_unknown",
        )?;
    }
    if !input.redaction.secret_free || input.redaction.raw_payload_count != 0 {
        emit(
            &mut findings,
            "evidence.redaction_secret_detected",
            "secret_free_zero_raw_payload",
            "secret_or_raw_payload",
        )?;
    }

    let mut assertion_ids = BTreeSet::new();
    for assertion in &input.assertions {
        check_schema(
            &mut findings,
            "evidence.assertion_schema_invalid",
            &assertion.schema,
            ASSERTION_SCHEMA,
        )?;
        if !valid_reference(&assertion.assertion_id)
            || !assertion_ids.insert(&assertion.assertion_id)
        {
            emit(
                &mut findings,
                "evidence.assertion_invalid",
                "unique_bounded_assertion_id",
                "invalid_or_duplicate",
            )?;
        }
        if assertion.expected.trim().is_empty() || assertion.expected.len() > MAX_TEXT_BYTES {
            emit(
                &mut findings,
                "evidence.assertion_invalid",
                "bounded_expected_value",
                "invalid",
            )?;
        }
        match assertion.actual.as_deref() {
            Some(actual) if actual != assertion.expected => emit(
                &mut findings,
                "evidence.receipt_assertion_mismatch",
                "expected_equals_actual",
                "mismatch",
            )?,
            None if assertion.required => emit(
                &mut findings,
                "evidence.receipt_assertion_missing",
                "required_assertion_value",
                "missing",
            )?,
            Some(actual) if actual.len() > MAX_TEXT_BYTES => emit(
                &mut findings,
                "evidence.assertion_invalid",
                "bounded_actual_value",
                "invalid",
            )?,
            _ => {}
        }
    }

    Ok(findings)
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, expected, "invalid")?;
    }
    Ok(())
}

fn valid_reference(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            serde_json::Value::String(expected.to_owned()),
            serde_json::Value::String(actual.to_owned()),
            format!("evidence and receipt finding: {code}"),
            "evidence:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
