//! EQ-27 deterministic evaluator and finding contracts.
//!
//! This module is deliberately a value-only boundary.  An evaluator receives a caller supplied
//! JSON value and returns bounded, validated findings.  It cannot start a runner, contact a
//! provider, read the filesystem, or dispatch a capability.  Later evaluator steps (EQ-28+)
//! provide the domain-specific input and checks while reusing this contract.

use kiana_domain::{canonical_journal_bytes, json_digest, redact_text, redact_value};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Versioned wire schema for one quality finding.
pub const FINDING_SCHEMA: &str = "kiana.quality-finding.v1";
/// Versioned envelope used when a registry is materialized as an evidence value.
pub const EVALUATION_RESULT_SCHEMA: &str = "kiana.quality-evaluation-result.v1";
/// Versioned shape for explicit expected/actual evaluator inputs.
pub const EVALUATOR_INPUT_SCHEMA: &str = "kiana.quality-evaluator-input.v1";
/// Upper bound for one finding code, in bytes.
pub const MAX_FINDING_CODE_BYTES: usize = 128;
/// Upper bound for a finding message, in bytes.
pub const MAX_FINDING_MESSAGE_BYTES: usize = 2_048;
/// Upper bound for one evidence reference, in bytes.
pub const MAX_FINDING_EVIDENCE_REF_BYTES: usize = 512;
/// Upper bound for either expected or actual finding value after canonical encoding.
pub const MAX_FINDING_VALUE_BYTES: usize = 8 * 1024;
/// Maximum JSON nesting accepted in a finding value.
pub const MAX_FINDING_VALUE_DEPTH: usize = 16;
/// Maximum number of findings emitted by one registry evaluation.
pub const MAX_FINDINGS: usize = 4_096;
/// Maximum number of evaluators in one registry.
pub const MAX_EVALUATORS: usize = 128;
/// Maximum bytes in the evaluator input.  This is a CPU/memory bound, not a storage contract.
pub const MAX_EVALUATOR_INPUT_BYTES: usize = 256 * 1024;

/// A stable, evidence-bound quality finding.
///
/// `expected` and `actual` intentionally remain structured JSON values so an evaluator can
/// explain a mismatch without inventing a second, untyped encoding.  Constructors and validation
/// reject values which would cross the redaction boundary; callers that intentionally receive
/// untrusted values can use [`Finding::redacted`] to sanitize them before construction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub schema: String,
    pub code: String,
    pub expected: Value,
    pub actual: Value,
    pub message: String,
    pub evidence_ref: String,
}

impl Finding {
    /// Construct a finding and validate every field at the boundary.
    pub fn new(
        code: impl Into<String>,
        expected: Value,
        actual: Value,
        message: impl Into<String>,
        evidence_ref: impl Into<String>,
    ) -> Result<Self, FindingError> {
        let finding = Self {
            schema: FINDING_SCHEMA.to_owned(),
            code: code.into(),
            expected,
            actual,
            message: message.into(),
            evidence_ref: evidence_ref.into(),
        };
        finding.validate()?;
        Ok(finding)
    }

    /// Construct a finding after applying the shared redaction boundary to all free-form values.
    ///
    /// Redaction is explicit so a caller cannot accidentally confuse a sanitized display value
    /// with the original evidence.  The resulting finding is still subject to all size and shape
    /// limits.
    pub fn redacted(
        code: impl Into<String>,
        expected: Value,
        actual: Value,
        message: impl AsRef<str>,
        evidence_ref: impl AsRef<str>,
    ) -> Result<Self, FindingError> {
        Self::new(
            code,
            redact_value(&expected),
            redact_value(&actual),
            redact_text(message.as_ref()),
            redact_text(evidence_ref.as_ref()),
        )
    }

    /// Validate the schema, stable identifiers, redaction boundary and bounded JSON values.
    pub fn validate(&self) -> Result<(), FindingError> {
        if self.schema != FINDING_SCHEMA {
            return Err(FindingError::SchemaInvalid);
        }
        validate_stable_code(&self.code)?;
        validate_value(&self.expected, "expected")?;
        validate_value(&self.actual, "actual")?;
        validate_message(&self.message)?;
        validate_evidence_ref(&self.evidence_ref)?;
        Ok(())
    }

    /// Canonical digest of all finding fields, including its evidence reference.
    pub fn digest(&self) -> Result<String, FindingError> {
        self.validate()?;
        Ok(json_digest(&serde_json::json!({
            "schema": self.schema,
            "code": self.code,
            "expected": self.expected,
            "actual": self.actual,
            "message": self.message,
            "evidence_ref": self.evidence_ref,
        })))
    }

    /// Canonical bytes used by the registry's stable ordering and digest binding.
    fn sort_key(&self) -> Result<Vec<u8>, FindingError> {
        self.validate()?;
        canonical_journal_bytes(&serde_json::json!({
            "code": self.code,
            "expected": self.expected,
            "actual": self.actual,
            "message": self.message,
            "evidence_ref": self.evidence_ref,
        }))
        .map_err(|_| FindingError::CanonicalEncodingFailed)
    }
}

/// A bounded pair of values passed to an evaluator that wants explicit expected/actual inputs.
///
/// The core trait accepts [`serde_json::Value`] to keep it object-safe and usable by future
/// evaluator families.  This helper gives those families a stable, versioned shape without
/// coupling EQ-27 to a particular trace or runtime type.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationInput {
    pub schema: String,
    pub expected: Value,
    pub actual: Value,
    pub evidence_ref: String,
}

impl EvaluationInput {
    pub fn new(
        expected: Value,
        actual: Value,
        evidence_ref: impl Into<String>,
    ) -> Result<Self, EvaluatorError> {
        let input = Self {
            schema: EVALUATOR_INPUT_SCHEMA.to_owned(),
            expected,
            actual,
            evidence_ref: evidence_ref.into(),
        };
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<(), EvaluatorError> {
        if self.schema != EVALUATOR_INPUT_SCHEMA {
            return Err(EvaluatorError::InputSchemaInvalid);
        }
        validate_value_bound(&self.expected, "expected")?;
        validate_value_bound(&self.actual, "actual")?;
        validate_evidence_ref(&self.evidence_ref).map_err(EvaluatorError::Finding)?;
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::json!({
            "schema": self.schema,
            "expected": self.expected,
            "actual": self.actual,
            "evidence_ref": self.evidence_ref,
        })
    }
}

/// Errors raised by the strict finding schema.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FindingError {
    #[error("quality_finding_schema_invalid")]
    SchemaInvalid,
    #[error("quality_finding_code_invalid")]
    CodeInvalid,
    #[error("quality_finding_message_invalid")]
    MessageInvalid,
    #[error("quality_finding_evidence_ref_invalid")]
    EvidenceRefInvalid,
    #[error("quality_finding_value_invalid:{0}")]
    ValueInvalid(&'static str),
    #[error("quality_finding_value_too_large:{0}")]
    ValueTooLarge(&'static str),
    #[error("quality_finding_value_too_deep:{0}")]
    ValueTooDeep(&'static str),
    #[error("quality_finding_secret_detected:{0}")]
    SecretDetected(&'static str),
    #[error("quality_finding_canonical_encoding_failed")]
    CanonicalEncodingFailed,
}

/// Errors raised while registering or running deterministic evaluators.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum EvaluatorError {
    #[error("quality_evaluator_input_schema_invalid")]
    InputSchemaInvalid,
    #[error("quality_evaluator_input_invalid:{0}")]
    InputInvalid(&'static str),
    #[error("quality_evaluator_input_too_large")]
    InputTooLarge,
    #[error("quality_evaluator_id_invalid")]
    EvaluatorIdInvalid,
    #[error("quality_evaluator_duplicate")]
    DuplicateEvaluator,
    #[error("quality_evaluator_limit_exceeded")]
    EvaluatorLimitExceeded,
    #[error("quality_evaluator_finding_limit_exceeded")]
    FindingLimitExceeded,
    #[error("quality_evaluator_finding_invalid:{0}")]
    Finding(#[from] FindingError),
    #[error("quality_evaluator_failed")]
    EvaluationFailed,
}

/// Implement this trait for a pure evaluator over caller-supplied JSON.
///
/// The default identifier is useful for a one-off evaluator.  A registry with more than one
/// evaluator should override [`Self::evaluator_id`] with a stable lower-case identifier.  The
/// trait has no access to a daemon, provider, runner, filesystem or capability broker by design.
pub trait DeterministicEvaluator {
    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError>;

    fn evaluator_id(&self) -> &str {
        "default"
    }
}

/// Compatibility spelling for callers that refer to the trait as `Evaluator`.
pub use DeterministicEvaluator as Evaluator;

/// A registry that runs evaluators in stable identifier order and returns stable finding order.
pub struct EvaluatorRegistry {
    evaluators: BTreeMap<String, Box<dyn DeterministicEvaluator>>,
}

impl Default for EvaluatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EvaluatorRegistry {
    pub fn new() -> Self {
        Self {
            evaluators: BTreeMap::new(),
        }
    }

    pub fn register<E>(&mut self, evaluator: E) -> Result<(), EvaluatorError>
    where
        E: DeterministicEvaluator + 'static,
    {
        if self.evaluators.len() >= MAX_EVALUATORS {
            return Err(EvaluatorError::EvaluatorLimitExceeded);
        }
        let id = evaluator.evaluator_id().to_owned();
        validate_evaluator_id(&id)?;
        if self.evaluators.contains_key(&id) {
            return Err(EvaluatorError::DuplicateEvaluator);
        }
        self.evaluators.insert(id, Box::new(evaluator));
        Ok(())
    }

    /// Alias for [`Self::register`] used by adapters that call registry entries "add".
    pub fn add<E>(&mut self, evaluator: E) -> Result<(), EvaluatorError>
    where
        E: DeterministicEvaluator + 'static,
    {
        self.register(evaluator)
    }

    pub fn len(&self) -> usize {
        self.evaluators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.evaluators.is_empty()
    }

    pub fn evaluator_ids(&self) -> Vec<String> {
        self.evaluators.keys().cloned().collect()
    }

    /// Evaluate all registered evaluators in identifier order and canonicalize their findings.
    pub fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        validate_input_value(input)?;
        let mut findings = Vec::new();
        for evaluator in self.evaluators.values() {
            let mut emitted = evaluator.evaluate(input)?;
            if emitted.len() > MAX_FINDINGS.saturating_sub(findings.len()) {
                return Err(EvaluatorError::FindingLimitExceeded);
            }
            for finding in &emitted {
                finding.validate()?;
            }
            findings.append(&mut emitted);
        }
        sort_findings(&mut findings)?;
        Ok(findings)
    }

    /// Alias for [`Self::evaluate`] that makes the execution boundary explicit at call sites.
    pub fn run(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        self.evaluate(input)
    }

    /// Evaluate and return the canonical digest of the complete finding set.
    pub fn evaluate_with_digest(&self, input: &Value) -> Result<EvaluationResult, EvaluatorError> {
        let findings = self.evaluate(input)?;
        EvaluationResult::new(self.evaluator_ids(), findings).map_err(EvaluatorError::from)
    }
}

/// Canonical result envelope for a registry evaluation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationResult {
    pub schema: String,
    pub evaluator_ids: Vec<String>,
    pub findings: Vec<Finding>,
    pub findings_digest: String,
}

impl EvaluationResult {
    pub fn new(
        mut evaluator_ids: Vec<String>,
        mut findings: Vec<Finding>,
    ) -> Result<Self, FindingError> {
        if evaluator_ids.is_empty() || evaluator_ids.len() > MAX_EVALUATORS {
            return Err(FindingError::ValueInvalid("evaluator_ids"));
        }
        evaluator_ids.sort();
        if evaluator_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(FindingError::ValueInvalid("evaluator_ids"));
        }
        for id in &evaluator_ids {
            validate_evaluator_id(id).map_err(|_| FindingError::ValueInvalid("evaluator_ids"))?;
        }
        sort_findings(&mut findings)?;
        if findings.len() > MAX_FINDINGS {
            return Err(FindingError::ValueTooLarge("findings"));
        }
        let findings_digest = digest_findings(&findings)?;
        Ok(Self {
            schema: EVALUATION_RESULT_SCHEMA.to_owned(),
            evaluator_ids,
            findings,
            findings_digest,
        })
    }

    pub fn validate(&self) -> Result<(), FindingError> {
        if self.schema != EVALUATION_RESULT_SCHEMA
            || self.evaluator_ids.is_empty()
            || self.evaluator_ids.len() > MAX_EVALUATORS
            || self.evaluator_ids.windows(2).any(|pair| pair[0] >= pair[1])
            || self.findings.len() > MAX_FINDINGS
        {
            return Err(FindingError::SchemaInvalid);
        }
        for id in &self.evaluator_ids {
            validate_evaluator_id(id).map_err(|_| FindingError::ValueInvalid("evaluator_ids"))?;
        }
        let mut findings = self.findings.clone();
        sort_findings(&mut findings)?;
        if findings != self.findings || self.findings_digest != digest_findings(&findings)? {
            return Err(FindingError::ValueInvalid("findings_digest"));
        }
        Ok(())
    }
}

/// Sort and validate findings by stable code, canonical expected/actual values, message and
/// evidence reference.  The result is independent of evaluator registration order.
pub fn sort_findings(findings: &mut [Finding]) -> Result<(), FindingError> {
    let mut keys = Vec::with_capacity(findings.len());
    for finding in findings.iter() {
        keys.push(finding.sort_key()?);
    }
    // `sort_by` is stable; the canonical key includes every field, so equal keys remain
    // indistinguishable and cannot introduce an observable order difference.
    findings.sort_by(|left, right| {
        let left_key = canonical_sort_key(left);
        let right_key = canonical_sort_key(right);
        left_key.cmp(&right_key)
    });
    // Keep the precomputed keys live so validation cannot accidentally be optimized away from a
    // future refactor that changes `canonical_sort_key` into a fallible operation.
    let _ = keys;
    Ok(())
}

/// Return a sorted, validated copy of a finding collection.
pub fn canonical_findings(mut findings: Vec<Finding>) -> Result<Vec<Finding>, FindingError> {
    sort_findings(&mut findings)?;
    if findings.len() > MAX_FINDINGS {
        return Err(FindingError::ValueTooLarge("findings"));
    }
    Ok(findings)
}

/// Digest a sorted finding collection, binding each evidence reference to the result.
pub fn digest_findings(findings: &[Finding]) -> Result<String, FindingError> {
    let sorted = canonical_findings(findings.to_vec())?;
    Ok(json_digest(&serde_json::json!({
        "schema": FINDING_SCHEMA,
        "findings": sorted,
    })))
}

/// Compatibility spelling for callers that use `findings_digest` as a function name.
pub fn findings_digest(findings: &[Finding]) -> Result<String, FindingError> {
    digest_findings(findings)
}

/// Validate a finding through the same public boundary used by the registry.
pub fn validate_finding(finding: &Finding) -> Result<(), FindingError> {
    finding.validate()
}

fn canonical_sort_key(finding: &Finding) -> Vec<u8> {
    // The caller validates before sorting, so canonical encoding can only fail for a future
    // serialization implementation change.  A deterministic fallback still preserves order.
    finding.sort_key().unwrap_or_default()
}

fn validate_stable_code(value: &str) -> Result<(), FindingError> {
    if value.is_empty()
        || value.len() > MAX_FINDING_CODE_BYTES
        || value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'.' | b':' | b'-'))
        })
        || !value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(FindingError::CodeInvalid);
    }
    Ok(())
}

fn validate_evidence_ref(value: &str) -> Result<(), FindingError> {
    if value.is_empty()
        || value.len() > MAX_FINDING_EVIDENCE_REF_BYTES
        || value.contains(['\0', '\n', '\r'])
        || value.chars().any(char::is_whitespace)
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'-' | b'#'))
        })
        || redact_text(value) != value
    {
        return Err(FindingError::EvidenceRefInvalid);
    }
    Ok(())
}

fn validate_message(value: &str) -> Result<(), FindingError> {
    if value.trim().is_empty()
        || value.len() > MAX_FINDING_MESSAGE_BYTES
        || value.contains(['\0', '\n', '\r'])
    {
        return Err(FindingError::MessageInvalid);
    }
    if redact_text(value) != value {
        return Err(FindingError::SecretDetected("message"));
    }
    Ok(())
}

fn validate_value(value: &Value, field: &'static str) -> Result<(), FindingError> {
    validate_value_shape(value, field, 0)?;
    if redact_value(value) != *value {
        return Err(FindingError::SecretDetected(field));
    }
    let bytes =
        canonical_journal_bytes(value).map_err(|_| FindingError::CanonicalEncodingFailed)?;
    if bytes.len() > MAX_FINDING_VALUE_BYTES {
        return Err(FindingError::ValueTooLarge(field));
    }
    Ok(())
}

fn validate_value_bound(value: &Value, field: &'static str) -> Result<(), EvaluatorError> {
    validate_value_shape(value, field, 0).map_err(EvaluatorError::Finding)?;
    let bytes = serde_json::to_vec(value).map_err(|_| EvaluatorError::InputInvalid(field))?;
    if bytes.len() > MAX_EVALUATOR_INPUT_BYTES {
        return Err(EvaluatorError::InputTooLarge);
    }
    Ok(())
}

fn validate_value_shape(
    value: &Value,
    field: &'static str,
    depth: usize,
) -> Result<(), FindingError> {
    if depth > MAX_FINDING_VALUE_DEPTH {
        return Err(FindingError::ValueTooDeep(field));
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
        Value::String(text) => {
            if text.contains('\0') {
                Err(FindingError::ValueInvalid(field))
            } else {
                Ok(())
            }
        }
        Value::Array(values) => values
            .iter()
            .try_for_each(|value| validate_value_shape(value, field, depth + 1)),
        Value::Object(values) => values.iter().try_for_each(|(key, value)| {
            if key.is_empty() || key.len() > MAX_FINDING_CODE_BYTES || key.contains('\0') {
                return Err(FindingError::ValueInvalid(field));
            }
            validate_value_shape(value, field, depth + 1)
        }),
    }
}

fn validate_evaluator_id(value: &str) -> Result<(), EvaluatorError> {
    if value.is_empty()
        || value.len() > MAX_FINDING_CODE_BYTES
        || value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'.' | b':' | b'-'))
        })
        || !value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || redact_text(value) != value
    {
        return Err(EvaluatorError::EvaluatorIdInvalid);
    }
    Ok(())
}

fn validate_input_value(input: &Value) -> Result<(), EvaluatorError> {
    validate_value_bound(input, "input")
}
