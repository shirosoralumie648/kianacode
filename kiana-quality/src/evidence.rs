//! EQ-30 evidence and receipt evaluation over caller-supplied, redacted evidence.
//!
//! The evaluator binds artifact references and strict SHA-256 digests, the receipt
//! digest/cursor/artifact links, required receipt assertions, redaction and secret-free
//! status, provenance references and the source cursor/event ID range.  Missing artifacts,
//! hash drift, unverified receipts, assertion gaps, redaction failure, provenance drift and
//! cursor mismatch emit stable EQ-27 findings.
//!
//! It is a pure diagnostic over the evidence it is given: it never reads an artifact storage
//! adapter, queries a Receipt projection, recomputes a durable fact, fetches a provider
//! receipt, executes a capability or turns a finding into a release or authorization
//! decision.  Findings carry a per-section evidence reference and every caller-supplied
//! value is passed through [`Finding::redacted`] before it can reach a finding.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Versioned envelope for one evidence/receipt input.
pub const EVIDENCE_RECEIPT_INPUT_SCHEMA: &str = "kiana.quality-evidence-receipt-input.v1";
/// Stable registry identifier for the evidence/receipt evaluator.
pub const EVIDENCE_RECEIPT_EVALUATOR_ID: &str = "evidence-receipt";
const ARTIFACT_SCHEMA: &str = "kiana.quality-artifact-evidence.v1";
const RECEIPT_SCHEMA: &str = "kiana.quality-receipt-evidence.v1";
const ASSERTION_SCHEMA: &str = "kiana.quality-receipt-assertion.v1";
const REDACTION_SCHEMA: &str = "kiana.quality-redaction-evidence.v1";
const PROVENANCE_SCHEMA: &str = "kiana.quality-provenance-evidence.v1";
const CURSOR_SCHEMA: &str = "kiana.quality-source-cursor-evidence.v1";
const MAX_ARTIFACTS: usize = 256;
const MAX_ASSERTIONS: usize = 256;
const MAX_SOURCE_EVENT_IDS: usize = 1_024;
const MAX_TEXT_BYTES: usize = 512;
const PROVENANCE_EVIDENCE: &str = "evidence:provenance";
const CURSOR_EVIDENCE: &str = "evidence:source-cursor";
const RECEIPT_EVIDENCE: &str = "evidence:receipt";
const REDACTION_EVIDENCE: &str = "evidence:redaction";

/// One artifact reference with its observed and expected content digest.
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

/// The receipt under evaluation and the artifacts it claims to link.
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

/// One required (or optional) receipt assertion declared by the eval case.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAssertion {
    pub schema: String,
    pub assertion_id: String,
    pub expected: String,
    /// `None` means the receipt carried no value for this assertion.  A required
    /// assertion without a value cannot pass; an optional one may stay absent.
    #[serde(default)]
    pub actual: Option<String>,
    pub required: bool,
}

/// Evidence that the payloads were redacted and carried no raw secret payload.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionEvidence {
    pub schema: String,
    pub redacted: bool,
    pub secret_free: bool,
    pub raw_payload_count: u64,
}

/// Where the evidence came from: a bounded provenance reference, the source snapshot and
/// the fixture/environment digests the captured trace was produced under.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceEvidence {
    pub schema: String,
    pub provenance_ref: String,
    pub source_snapshot: String,
    pub fixture_digest: String,
    pub environment_digest: String,
}

/// The source cursor range the evidence was selected from, with the exact event IDs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCursorEvidence {
    pub schema: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub event_count: u64,
    pub source_event_ids: Vec<String>,
}

/// Versioned input envelope for [`EvidenceReceiptEvaluator`].
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
    pub fn new(
        artifacts: Vec<ArtifactEvidence>,
        receipt: ReceiptEvidence,
        assertions: Vec<ReceiptAssertion>,
        redaction: RedactionEvidence,
        provenance: ProvenanceEvidence,
        source_cursor: SourceCursorEvidence,
    ) -> Self {
        Self {
            schema: EVIDENCE_RECEIPT_INPUT_SCHEMA.to_owned(),
            artifacts,
            receipt,
            assertions,
            redaction,
            provenance,
            source_cursor,
        }
    }

    /// Hard input bounds.  Structural drift fails closed before any finding is produced;
    /// entry shape problems remain findings so the report explains *why* the evidence is
    /// unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != EVIDENCE_RECEIPT_INPUT_SCHEMA {
            return Err("evidence_receipt_input_schema_invalid");
        }
        if self.artifacts.len() > MAX_ARTIFACTS {
            return Err("evidence_receipt_artifact_limit_exceeded");
        }
        if self.assertions.len() > MAX_ASSERTIONS {
            return Err("evidence_receipt_assertion_limit_exceeded");
        }
        if self.source_cursor.source_event_ids.len() > MAX_SOURCE_EVENT_IDS {
            return Err("evidence_receipt_event_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("evidence receipt input is serializable")
    }
}

/// Pure evidence/receipt evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct EvidenceReceiptEvaluator;

impl DeterministicEvaluator for EvidenceReceiptEvaluator {
    fn evaluator_id(&self) -> &str {
        EVIDENCE_RECEIPT_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: EvidenceReceiptInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("evidence_receipt"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_evidence_receipt(&decoded)
    }
}

/// Evaluate the deny-first evidence/receipt matrix without reading any store.
///
/// Check order is deliberate: section schema identity, provenance, source cursor and event
/// IDs, the receipt's digest/cursor/provenance links, every artifact, artifact/receipt link
/// containment in both directions, redaction status and finally every receipt assertion.
/// Artifacts, source event IDs and assertions are ordered by their canonical identity before
/// findings are emitted, so the finding set does not depend on the caller's array order while
/// duplicate evidence stays duplicated and visible.
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
        PROVENANCE_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "evidence.cursor_schema_invalid",
        &cursor.schema,
        CURSOR_SCHEMA,
        CURSOR_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "evidence.receipt_schema_invalid",
        &receipt.schema,
        RECEIPT_SCHEMA,
        RECEIPT_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "evidence.redaction_schema_invalid",
        &input.redaction.schema,
        REDACTION_SCHEMA,
        REDACTION_EVIDENCE,
    )?;

    if !is_bounded_reference(&provenance.provenance_ref)
        || !is_bounded_reference(&provenance.source_snapshot)
    {
        emit(
            &mut findings,
            "evidence.provenance_missing",
            text("bounded_provenance_and_snapshot"),
            provenance_actual(provenance),
            PROVENANCE_EVIDENCE,
        )?;
    }
    if !is_sha256_digest(&provenance.fixture_digest)
        || !is_sha256_digest(&provenance.environment_digest)
    {
        emit(
            &mut findings,
            "evidence.provenance_invalid",
            text("sha256_fixture_and_environment"),
            provenance_actual(provenance),
            PROVENANCE_EVIDENCE,
        )?;
    }

    if cursor.source_cursor_start == 0 || cursor.source_cursor_start > cursor.source_cursor_end {
        emit(
            &mut findings,
            "evidence.source_cursor_invalid",
            text("positive_ordered_cursor_range"),
            cursor_actual(cursor),
            CURSOR_EVIDENCE,
        )?;
    }
    if cursor.event_count == 0 || cursor.event_count as usize != cursor.source_event_ids.len() {
        emit(
            &mut findings,
            "evidence.source_cursor_invalid",
            text("event_count_matches_event_ids"),
            cursor_actual(cursor),
            CURSOR_EVIDENCE,
        )?;
    }
    if cursor.event_count > 0 {
        let span = cursor
            .source_cursor_end
            .saturating_sub(cursor.source_cursor_start)
            .saturating_add(1);
        if span < cursor.event_count {
            emit(
                &mut findings,
                "evidence.source_cursor_invalid",
                text("cursor_range_covers_event_count"),
                cursor_actual(cursor),
                CURSOR_EVIDENCE,
            )?;
        }
    }
    let mut event_ids: Vec<&String> = cursor.source_event_ids.iter().collect();
    event_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    let mut seen_event_ids = BTreeSet::new();
    for (index, event_id) in event_ids.iter().enumerate() {
        if !is_bounded_reference(event_id) || !seen_event_ids.insert(event_id.as_str()) {
            emit(
                &mut findings,
                "evidence.source_event_id_invalid",
                text("unique_bounded_event_ids"),
                text(event_id),
                &format!("{CURSOR_EVIDENCE}:{index}"),
            )?;
        }
    }

    if !is_sha256_digest(&receipt.receipt_digest) {
        emit(
            &mut findings,
            "evidence.receipt_digest_invalid",
            text("sha256_digest"),
            receipt_actual(receipt),
            RECEIPT_EVIDENCE,
        )?;
    }
    if !receipt.digest_verified {
        emit(
            &mut findings,
            "evidence.receipt_digest_unverified",
            text("verified"),
            receipt_actual(receipt),
            RECEIPT_EVIDENCE,
        )?;
    }
    if receipt.source_cursor != cursor.source_cursor_end {
        emit(
            &mut findings,
            "evidence.receipt_cursor_mismatch",
            text("receipt_at_source_cursor_end"),
            receipt_actual(receipt),
            RECEIPT_EVIDENCE,
        )?;
    }
    if receipt.provenance_ref != provenance.provenance_ref {
        emit(
            &mut findings,
            "evidence.provenance_mismatch",
            text("shared_provenance_ref"),
            text(&receipt.provenance_ref),
            RECEIPT_EVIDENCE,
        )?;
    }
    for artifact_ref in &receipt.artifact_refs {
        if !is_bounded_reference(artifact_ref) {
            emit(
                &mut findings,
                "evidence.artifact_ref_invalid",
                text("bounded_artifact_ref"),
                text(artifact_ref),
                RECEIPT_EVIDENCE,
            )?;
        }
    }

    let mut artifacts: Vec<&ArtifactEvidence> = input.artifacts.iter().collect();
    artifacts.sort_by(|left, right| artifact_sort_key(left).cmp(&artifact_sort_key(right)));
    let mut artifacts_seen = BTreeSet::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        let evidence_ref = format!("evidence:artifact:{index}");
        check_schema(
            &mut findings,
            "evidence.artifact_schema_invalid",
            &artifact.schema,
            ARTIFACT_SCHEMA,
            &evidence_ref,
        )?;
        if !is_bounded_reference(&artifact.artifact_ref)
            || !artifacts_seen.insert(artifact.artifact_ref.clone())
        {
            emit(
                &mut findings,
                "evidence.artifact_ref_invalid",
                text("unique_bounded_artifact_ref"),
                text(&artifact.artifact_ref),
                &evidence_ref,
            )?;
        }
        if !is_sha256_digest(&artifact.artifact_digest)
            || !is_sha256_digest(&artifact.expected_digest)
        {
            emit(
                &mut findings,
                "evidence.artifact_hash_invalid",
                text("sha256_digest_pair"),
                artifact_actual(artifact),
                &evidence_ref,
            )?;
        } else if !artifact
            .artifact_digest
            .eq_ignore_ascii_case(&artifact.expected_digest)
        {
            emit(
                &mut findings,
                "evidence.artifact_hash_mismatch",
                text("expected_digest"),
                artifact_actual(artifact),
                &evidence_ref,
            )?;
        }
        if !artifact.digest_verified {
            emit(
                &mut findings,
                "evidence.artifact_hash_unverified",
                text("verified"),
                artifact_actual(artifact),
                &evidence_ref,
            )?;
        }
        if !artifact.redacted {
            emit(
                &mut findings,
                "evidence.artifact_unredacted",
                text("redacted"),
                artifact_actual(artifact),
                &evidence_ref,
            )?;
        }
        if artifact.source_cursor == 0
            || artifact.source_cursor < cursor.source_cursor_start
            || artifact.source_cursor > cursor.source_cursor_end
        {
            emit(
                &mut findings,
                "evidence.artifact_cursor_invalid",
                text("cursor_inside_source_range"),
                artifact_actual(artifact),
                &evidence_ref,
            )?;
        }
        if artifact.provenance_ref != provenance.provenance_ref {
            emit(
                &mut findings,
                "evidence.provenance_mismatch",
                text("shared_provenance_ref"),
                text(&artifact.provenance_ref),
                &evidence_ref,
            )?;
        }
        if !receipt.artifact_refs.contains(&artifact.artifact_ref) {
            emit(
                &mut findings,
                "evidence.artifact_unreferenced",
                text("artifact_linked_from_receipt"),
                text(&artifact.artifact_ref),
                &evidence_ref,
            )?;
        }
    }
    for artifact_ref in &receipt.artifact_refs {
        if !artifacts_seen.contains(artifact_ref) {
            emit(
                &mut findings,
                "evidence.artifact_missing",
                text("receipt_artifact_has_evidence"),
                text(artifact_ref),
                RECEIPT_EVIDENCE,
            )?;
        }
    }

    if !input.redaction.redacted {
        emit(
            &mut findings,
            "evidence.redaction_missing",
            text("redacted"),
            redaction_actual(&input.redaction),
            REDACTION_EVIDENCE,
        )?;
    }
    if !input.redaction.secret_free || input.redaction.raw_payload_count != 0 {
        emit(
            &mut findings,
            "evidence.redaction_secret_detected",
            text("secret_free_zero_raw_payload"),
            redaction_actual(&input.redaction),
            REDACTION_EVIDENCE,
        )?;
    }

    let mut assertions: Vec<&ReceiptAssertion> = input.assertions.iter().collect();
    assertions.sort_by(|left, right| assertion_sort_key(left).cmp(&assertion_sort_key(right)));
    let mut assertion_ids = BTreeSet::new();
    for (index, assertion) in assertions.iter().enumerate() {
        let evidence_ref = format!("evidence:assertion:{index}");
        check_schema(
            &mut findings,
            "evidence.assertion_schema_invalid",
            &assertion.schema,
            ASSERTION_SCHEMA,
            &evidence_ref,
        )?;
        if !is_bounded_reference(&assertion.assertion_id)
            || !assertion_ids.insert(assertion.assertion_id.clone())
        {
            emit(
                &mut findings,
                "evidence.assertion_invalid",
                text("unique_bounded_assertion_id"),
                text(&assertion.assertion_id),
                &evidence_ref,
            )?;
        }
        let expected_bounded = is_bounded_text(&assertion.expected);
        if !expected_bounded {
            emit(
                &mut findings,
                "evidence.assertion_invalid",
                text("bounded_expected_value"),
                assertion_actual(assertion),
                &evidence_ref,
            )?;
        }
        match assertion.actual.as_deref() {
            Some(actual) if !is_bounded_text(actual) => emit(
                &mut findings,
                "evidence.assertion_invalid",
                text("bounded_actual_value"),
                assertion_actual(assertion),
                &evidence_ref,
            )?,
            Some(actual) if expected_bounded && actual != assertion.expected => emit(
                &mut findings,
                "evidence.receipt_assertion_mismatch",
                text(&assertion.expected),
                text(actual),
                &evidence_ref,
            )?,
            None if assertion.required => emit(
                &mut findings,
                "evidence.receipt_assertion_missing",
                text(&assertion.expected),
                text("missing"),
                &evidence_ref,
            )?,
            _ => {}
        }
    }

    Ok(findings)
}

fn artifact_sort_key(artifact: &ArtifactEvidence) -> (&str, &str, &str, u64, &str) {
    (
        artifact.artifact_ref.as_str(),
        artifact.artifact_digest.as_str(),
        artifact.expected_digest.as_str(),
        artifact.source_cursor,
        artifact.provenance_ref.as_str(),
    )
}

fn assertion_sort_key(assertion: &ReceiptAssertion) -> (&str, &str, bool) {
    (
        assertion.assertion_id.as_str(),
        assertion.expected.as_str(),
        assertion.required,
    )
}

fn artifact_actual(artifact: &ArtifactEvidence) -> Value {
    serde_json::json!({
        "artifact_ref": artifact.artifact_ref,
        "artifact_digest": artifact.artifact_digest,
        "expected_digest": artifact.expected_digest,
        "digest_verified": artifact.digest_verified,
        "redacted": artifact.redacted,
        "source_cursor": artifact.source_cursor,
        "provenance_ref": artifact.provenance_ref,
    })
}

fn receipt_actual(receipt: &ReceiptEvidence) -> Value {
    serde_json::json!({
        "receipt_digest": receipt.receipt_digest,
        "digest_verified": receipt.digest_verified,
        "source_cursor": receipt.source_cursor,
        "provenance_ref": receipt.provenance_ref,
    })
}

fn assertion_actual(assertion: &ReceiptAssertion) -> Value {
    serde_json::json!({
        "assertion_id": assertion.assertion_id,
        "expected": assertion.expected,
        "actual": assertion.actual,
        "required": assertion.required,
    })
}

fn provenance_actual(provenance: &ProvenanceEvidence) -> Value {
    serde_json::json!({
        "provenance_ref": provenance.provenance_ref,
        "source_snapshot": provenance.source_snapshot,
    })
}

fn cursor_actual(cursor: &SourceCursorEvidence) -> Value {
    serde_json::json!({
        "source_cursor_start": cursor.source_cursor_start,
        "source_cursor_end": cursor.source_cursor_end,
        "event_count": cursor.event_count,
    })
}

fn redaction_actual(redaction: &RedactionEvidence) -> Value {
    serde_json::json!({
        "redacted": redaction.redacted,
        "secret_free": redaction.secret_free,
        "raw_payload_count": redaction.raw_payload_count,
    })
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(
            findings,
            code,
            text(expected),
            text("invalid"),
            evidence_ref,
        )?;
    }
    Ok(())
}

/// Bounded, single-line, secret-free reference accepted in artifact/receipt/provenance and
/// event identity positions.
fn is_bounded_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
}

/// Bounded, non-empty, single-line assertion text.
fn is_bounded_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.chars().any(char::is_control)
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a
/// secret-shaped reference or assertion value cannot escape through the finding itself.
fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: Value,
    actual: Value,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::redacted(
            code,
            expected,
            actual,
            format!("evidence and receipt finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
