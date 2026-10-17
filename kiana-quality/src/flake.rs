//! EQ-37 retry-once flake classifier, quarantine record and infrastructure-failure taxonomy.
//!
//! A second attempt is a classifier aid, never permission to count a failure as a pass.  One
//! retry is bounded to a single extra attempt of the same case with the same fixed seed and fresh
//! evidence; the retry outcome decides between a real failure (the same failure again), a flake (a
//! different outcome) and an infrastructure attempt, which is not a case result at all.  Flaky
//! cases must stay `needs_review` or `quarantined` behind an expiring owner/evidence record and
//! can never be reported as `pass`; infrastructure outcomes must name a known failure class and
//! can never be presented as a `pass` or a `fail`; out-of-scope attempts are neither.
//!
//! This is a pure value boundary over caller-supplied attempt evidence.  It does not rerun a case,
//! write a quarantine file, mutate a suite or gate, or advance a candidate: every caller-supplied
//! value passes through [`Finding::redacted`] and each finding cites the evidence section it came
//! from (`flake:input`, `flake:attempt:<i>`, `flake:status`, `flake:quarantine`).

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::{json_digest, redact_text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Versioned envelope for one retry-once flake classification input.
pub const FLAKE_INPUT_SCHEMA: &str = "kiana.quality-flake-input.v1";
/// Stable registry identifier for the flake classifier evaluator.
pub const FLAKE_EVALUATOR_ID: &str = "flake-classifier";
/// Versioned schema for one caller-supplied attempt record.
pub const FLAKE_ATTEMPT_SCHEMA: &str = "kiana.quality-flake-attempt.v1";
/// Versioned schema for one caller-supplied quarantine record.
pub const FLAKE_QUARANTINE_SCHEMA: &str = "kiana.quality-flake-quarantine.v1";
/// Maximum attempts accepted for one case: the original attempt plus exactly one retry.
pub const MAX_FLAKE_ATTEMPTS: usize = 2;
/// Maximum bytes accepted in one bounded identity, reason or owner token.
pub const MAX_FLAKE_TEXT_BYTES: usize = 512;
/// Maximum quarantine lifetime (30 days) accepted relative to the evaluation time.
pub const MAX_QUARANTINE_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

const INPUT_EVIDENCE: &str = "flake:input";
const STATUS_EVIDENCE: &str = "flake:status";
const QUARANTINE_EVIDENCE: &str = "flake:quarantine";
const ATTEMPT_EVIDENCE_PREFIX: &str = "flake:attempt:";

/// Outcome observed for one attempt of a case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    /// The attempt produced a passing case result.
    Pass,
    /// The attempt produced a failing case result.
    Fail,
    /// The attempt could not produce a case result for infrastructure reasons.
    Infra,
    /// The attempt was skipped because the case is out of scope.
    OutOfScope,
}

impl AttemptOutcome {
    /// Every outcome accepted by this contract, in stable rank order.
    pub const ALL: [Self; 4] = [
        AttemptOutcome::Pass,
        AttemptOutcome::Fail,
        AttemptOutcome::Infra,
        AttemptOutcome::OutOfScope,
    ];

    /// Stable rank used for deterministic evidence rendering.
    pub const fn rank(self) -> u8 {
        match self {
            AttemptOutcome::Pass => 0,
            AttemptOutcome::Fail => 1,
            AttemptOutcome::Infra => 2,
            AttemptOutcome::OutOfScope => 3,
        }
    }

    /// Whether this outcome carries a case result at all.  `infra` and `out_of_scope` do not.
    pub const fn is_case_result(self) -> bool {
        matches!(self, AttemptOutcome::Pass | AttemptOutcome::Fail)
    }
}

/// Known infrastructure failure classes.  `unknown` is a placeholder that is refused: an infra
/// attempt must be classified before it can be reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfraFailureClass {
    /// The runner/process itself failed.
    Runner,
    /// Fixture loading or fixture integrity failed.
    Fixture,
    /// Network access outside the controlled target failed.
    Network,
    /// A bounded resource (disk, memory, concurrency) was exhausted.
    Resource,
    /// Unclassified; refused by the classifier.
    Unknown,
}

impl InfraFailureClass {
    /// Every class accepted by this contract, in stable rank order.
    pub const ALL: [Self; 5] = [
        InfraFailureClass::Runner,
        InfraFailureClass::Fixture,
        InfraFailureClass::Network,
        InfraFailureClass::Resource,
        InfraFailureClass::Unknown,
    ];

    /// Stable rank used for deterministic evidence rendering.
    pub const fn rank(self) -> u8 {
        match self {
            InfraFailureClass::Runner => 0,
            InfraFailureClass::Fixture => 1,
            InfraFailureClass::Network => 2,
            InfraFailureClass::Resource => 3,
            InfraFailureClass::Unknown => 4,
        }
    }

    /// A known class is required before an infra attempt can be classified.
    pub const fn is_known(self) -> bool {
        !matches!(self, InfraFailureClass::Unknown)
    }
}

/// Final status claimed for one case.  The classifier checks the claim against attempt evidence
/// instead of trusting it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlakeFinalStatus {
    /// The case passed.
    Pass,
    /// The case failed for functional reasons.
    Fail,
    /// The case has no result because infrastructure failed.
    Infra,
    /// The case has no result because it is out of scope.
    OutOfScope,
    /// The case needs a human decision.
    NeedsReview,
    /// The case is quarantined behind an expiring record.
    Quarantined,
}

/// One caller-supplied attempt record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeAttempt {
    pub schema: String,
    pub attempt: u8,
    pub case_id: String,
    pub seed: u64,
    pub outcome: AttemptOutcome,
    pub infra_class: Option<InfraFailureClass>,
    pub evidence_digest: String,
}

impl FlakeAttempt {
    /// Build one attempt record.  An infra attempt adds its class with
    /// [`Self::with_infra_class`]; a case result or an out-of-scope attempt must not carry one.
    pub fn new(
        attempt: u8,
        case_id: impl Into<String>,
        seed: u64,
        outcome: AttemptOutcome,
        evidence_digest: impl Into<String>,
    ) -> Self {
        Self {
            schema: FLAKE_ATTEMPT_SCHEMA.to_owned(),
            attempt,
            case_id: case_id.into(),
            seed,
            outcome,
            infra_class: None,
            evidence_digest: evidence_digest.into(),
        }
    }

    /// Attach the known infrastructure failure class; only meaningful for `infra` attempts.
    pub fn with_infra_class(mut self, infra_class: InfraFailureClass) -> Self {
        self.infra_class = Some(infra_class);
        self
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("flake attempt is serializable")
    }
}

/// One caller-supplied quarantine record.  It is evidence, not authority: the evaluator only
/// checks that a flake points at a bounded, unexpired, owner-held record whose digest binds the
/// attempt evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeQuarantine {
    pub schema: String,
    pub case_id: String,
    pub reason: String,
    pub owner: String,
    pub expires_at_unix_ms: u64,
    pub evidence_digest: String,
}

impl FlakeQuarantine {
    /// Build one quarantine record.  `evidence_digest` must be
    /// [`flake_attempt_evidence_digest`] of the attempts it was derived from.
    pub fn new(
        case_id: impl Into<String>,
        reason: impl Into<String>,
        owner: impl Into<String>,
        expires_at_unix_ms: u64,
        evidence_digest: impl Into<String>,
    ) -> Self {
        Self {
            schema: FLAKE_QUARANTINE_SCHEMA.to_owned(),
            case_id: case_id.into(),
            reason: reason.into(),
            owner: owner.into(),
            expires_at_unix_ms,
            evidence_digest: evidence_digest.into(),
        }
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("flake quarantine is serializable")
    }
}

/// Versioned input envelope for [`FlakeClassifierEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlakeInput {
    pub schema: String,
    pub case_id: String,
    pub retry_once: bool,
    pub evaluated_at_unix_ms: u64,
    pub attempts: Vec<FlakeAttempt>,
    pub final_status: FlakeFinalStatus,
    pub quarantine: Option<FlakeQuarantine>,
}

impl FlakeInput {
    /// Build an input envelope for one case.  `retry_once` is enabled exactly when more than one
    /// attempt is supplied.
    pub fn new(
        case_id: impl Into<String>,
        evaluated_at_unix_ms: u64,
        attempts: Vec<FlakeAttempt>,
        final_status: FlakeFinalStatus,
    ) -> Self {
        let retry_once = attempts.len() > 1;
        Self {
            schema: FLAKE_INPUT_SCHEMA.to_owned(),
            case_id: case_id.into(),
            retry_once,
            evaluated_at_unix_ms,
            attempts,
            final_status,
            quarantine: None,
        }
    }

    /// Declare the retry policy explicitly.  The classifier refuses two attempts without it.
    pub fn with_retry_once(mut self, retry_once: bool) -> Self {
        self.retry_once = retry_once;
        self
    }

    /// Attach the quarantine record for a flaky case.
    pub fn with_quarantine(mut self, quarantine: FlakeQuarantine) -> Self {
        self.quarantine = Some(quarantine);
        self
    }

    /// Structural bounds fail closed before any finding is produced: a drifted schema, a missing
    /// attempt, more than one retry and an unset evaluation time are refusals, not findings.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != FLAKE_INPUT_SCHEMA {
            return Err("flake_input_schema_invalid");
        }
        if self.attempts.is_empty() {
            return Err("flake_input_attempts_missing");
        }
        if self.attempts.len() > MAX_FLAKE_ATTEMPTS {
            return Err("flake_attempt_limit_exceeded");
        }
        if self.evaluated_at_unix_ms == 0 {
            return Err("flake_input_time_invalid");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("flake input is serializable")
    }
}

/// Classification derived from attempt evidence alone.  The caller's `final_status` is a claim
/// checked against this value, never an input to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlakeClassification {
    /// The first attempt passed and no retry happened.
    Pass,
    /// The same failure was observed again; this is a real failure.
    Failure,
    /// Two attempts disagreed without infrastructure involvement; this is a flake.
    Flake,
    /// At least one attempt could not produce a case result for infrastructure reasons.
    Infra,
    /// At least one attempt was skipped as out of scope.
    OutOfScope,
    /// A retry was attempted after the case had already passed.
    RetryAfterPass,
}

impl FlakeClassification {
    /// Stable wire name used in evidence and findings.
    pub const fn as_str(self) -> &'static str {
        match self {
            FlakeClassification::Pass => "pass",
            FlakeClassification::Failure => "failure",
            FlakeClassification::Flake => "flake",
            FlakeClassification::Infra => "infra",
            FlakeClassification::OutOfScope => "out_of_scope",
            FlakeClassification::RetryAfterPass => "retry_after_pass",
        }
    }
}

/// Derive the classification from attempt evidence.  `input.attempts` must be non-empty and
/// bounded; that is enforced by [`FlakeInput::validate`].
///
/// Precedence: a retry after a pass is refused first, infrastructure and out-of-scope attempts
/// never produce a case result, a first failure followed by a pass is a flake, two identical
/// failures are a real failure, and everything else keeps the first attempt's outcome.
pub fn classify_flake(input: &FlakeInput) -> FlakeClassification {
    let Some(first) = input.attempts.first() else {
        return FlakeClassification::Failure;
    };
    if input.attempts.len() > 1 && first.outcome == AttemptOutcome::Pass {
        return FlakeClassification::RetryAfterPass;
    }
    if input
        .attempts
        .iter()
        .any(|attempt| attempt.outcome == AttemptOutcome::Infra)
    {
        return FlakeClassification::Infra;
    }
    if input
        .attempts
        .iter()
        .any(|attempt| attempt.outcome == AttemptOutcome::OutOfScope)
    {
        return FlakeClassification::OutOfScope;
    }
    if input.attempts.len() > 1
        && first.outcome == AttemptOutcome::Fail
        && input.attempts[1].outcome == AttemptOutcome::Pass
    {
        return FlakeClassification::Flake;
    }
    if first.outcome == AttemptOutcome::Pass {
        FlakeClassification::Pass
    } else {
        FlakeClassification::Failure
    }
}

/// Canonical digest of the attempt evidence a quarantine record must reference.
///
/// The digest covers the attempt schema, number, case identity, fixed seed, outcome and infra
/// class, in the order the attempts were supplied.  It deliberately excludes the per-attempt
/// evidence digest so the record binds the evidence it was derived from instead of itself.
pub fn flake_attempt_evidence_digest(attempts: &[FlakeAttempt]) -> String {
    json_digest(&Value::Array(
        attempts.iter().map(attempt_evidence_value).collect(),
    ))
}

/// Pure retry-once flake classifier.  All rerun scheduling, quarantine storage and gate
/// authority stays outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlakeClassifierEvaluator;

impl DeterministicEvaluator for FlakeClassifierEvaluator {
    fn evaluator_id(&self) -> &str {
        FLAKE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: FlakeInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("flake"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_flake(&decoded)
    }
}

/// Evaluate one retry-once envelope without running or rerunning anything.
///
/// Findings are produced in a fixed order: envelope case identity, per-attempt evidence, retry
/// policy, derived classification against the claimed status, then the quarantine record.
pub fn evaluate_flake(input: &FlakeInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();

    if !is_bounded_text(&input.case_id) {
        emit(
            &mut findings,
            "flake.case_invalid",
            text("bounded_case_identity"),
            json!({ "case_bounded": false }),
            INPUT_EVIDENCE,
        )?;
    }

    let mut previous: Option<&FlakeAttempt> = None;
    for (index, attempt) in input.attempts.iter().enumerate() {
        let evidence = format!("{ATTEMPT_EVIDENCE_PREFIX}{index}");
        let actual = attempt_actual(attempt);

        if attempt.schema != FLAKE_ATTEMPT_SCHEMA {
            emit(
                &mut findings,
                "flake.attempt_schema_invalid",
                text(FLAKE_ATTEMPT_SCHEMA),
                actual.clone(),
                &evidence,
            )?;
        }
        if attempt.attempt != (index + 1) as u8 {
            emit(
                &mut findings,
                "flake.attempt_order_invalid",
                text("sequential_attempts"),
                actual.clone(),
                &evidence,
            )?;
        }
        if !is_bounded_text(&attempt.case_id) || attempt.case_id != input.case_id {
            emit(
                &mut findings,
                "flake.attempt_case_mismatch",
                text("same_bounded_case_identity"),
                actual.clone(),
                &evidence,
            )?;
        }
        if !is_digest(&attempt.evidence_digest) {
            emit(
                &mut findings,
                "flake.attempt_evidence_invalid",
                text("sha256_attempt_evidence"),
                actual.clone(),
                &evidence,
            )?;
        }
        if attempt.outcome == AttemptOutcome::Infra {
            let classified = attempt
                .infra_class
                .map(InfraFailureClass::is_known)
                .unwrap_or(false);
            if !classified {
                emit(
                    &mut findings,
                    "flake.infra_unclassified",
                    text("known_infra_failure_class"),
                    actual.clone(),
                    &evidence,
                )?;
            }
        } else if attempt.infra_class.is_some() {
            emit(
                &mut findings,
                "flake.infra_class_unexpected",
                text("infra_class_only_for_infra_attempts"),
                actual.clone(),
                &evidence,
            )?;
        }

        if let Some(previous) = previous {
            if previous.seed != attempt.seed {
                emit(
                    &mut findings,
                    "flake.seed_mismatch",
                    text("one_fixed_seed_per_case"),
                    actual.clone(),
                    &evidence,
                )?;
            }
            if previous.evidence_digest == attempt.evidence_digest {
                emit(
                    &mut findings,
                    "flake.attempt_evidence_repeated",
                    text("fresh_evidence_per_attempt"),
                    actual.clone(),
                    &evidence,
                )?;
            }
            if !input.retry_once {
                emit(
                    &mut findings,
                    "flake.retry_policy_invalid",
                    text("retry_once_enabled"),
                    actual.clone(),
                    &evidence,
                )?;
            }
            if previous.outcome == AttemptOutcome::Pass {
                emit(
                    &mut findings,
                    "flake.retry_after_pass",
                    text("no_retry_after_pass"),
                    actual.clone(),
                    &evidence,
                )?;
            }
        }
        previous = Some(attempt);
    }

    let classification = classify_flake(input);
    let status_actual = json!({
        "classification": classification.as_str(),
        "final_status": input.final_status,
        "attempts": input.attempts.len(),
    });
    match classification {
        FlakeClassification::Pass | FlakeClassification::RetryAfterPass => {
            if input.final_status != FlakeFinalStatus::Pass {
                emit(
                    &mut findings,
                    "flake.status_attempt_mismatch",
                    text("pass"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
        }
        FlakeClassification::Failure => {
            if input.final_status != FlakeFinalStatus::Fail {
                emit(
                    &mut findings,
                    "flake.status_attempt_mismatch",
                    text("fail"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
        }
        FlakeClassification::Flake => match input.final_status {
            FlakeFinalStatus::NeedsReview | FlakeFinalStatus::Quarantined => {}
            FlakeFinalStatus::Pass => {
                emit(
                    &mut findings,
                    "flake.flaky_counted_as_pass",
                    text("needs_review_or_quarantine"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
            _ => {
                emit(
                    &mut findings,
                    "flake.flaky_counted_as_result",
                    text("needs_review_or_quarantine"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
        },
        FlakeClassification::Infra => match input.final_status {
            FlakeFinalStatus::Infra
            | FlakeFinalStatus::NeedsReview
            | FlakeFinalStatus::Quarantined => {}
            FlakeFinalStatus::Pass | FlakeFinalStatus::Fail => {
                emit(
                    &mut findings,
                    "flake.infra_counted_as_result",
                    text("infra_needs_review_or_quarantine"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
            FlakeFinalStatus::OutOfScope => {
                emit(
                    &mut findings,
                    "flake.status_attempt_mismatch",
                    text("infra"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
        },
        FlakeClassification::OutOfScope => match input.final_status {
            FlakeFinalStatus::OutOfScope | FlakeFinalStatus::NeedsReview => {}
            FlakeFinalStatus::Pass | FlakeFinalStatus::Fail | FlakeFinalStatus::Infra => {
                emit(
                    &mut findings,
                    "flake.out_of_scope_counted_as_result",
                    text("out_of_scope"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
            FlakeFinalStatus::Quarantined => {
                emit(
                    &mut findings,
                    "flake.status_attempt_mismatch",
                    text("out_of_scope"),
                    status_actual,
                    STATUS_EVIDENCE,
                )?;
            }
        },
    }

    let quarantine_expected = classification == FlakeClassification::Flake
        || input.final_status == FlakeFinalStatus::Quarantined;
    match (&input.quarantine, quarantine_expected) {
        (None, true) => {
            emit(
                &mut findings,
                "flake.quarantine_missing",
                text("quarantine_record"),
                json!({ "quarantine": "missing" }),
                QUARANTINE_EVIDENCE,
            )?;
        }
        (Some(record), _) => {
            let actual = quarantine_actual(record);
            if !quarantine_expected
                && matches!(
                    classification,
                    FlakeClassification::Pass
                        | FlakeClassification::Failure
                        | FlakeClassification::RetryAfterPass
                )
            {
                emit(
                    &mut findings,
                    "flake.quarantine_unexpected",
                    text("no_quarantine_for_stable_outcomes"),
                    actual.clone(),
                    QUARANTINE_EVIDENCE,
                )?;
            }
            if record.schema != FLAKE_QUARANTINE_SCHEMA {
                emit(
                    &mut findings,
                    "flake.quarantine_schema_invalid",
                    text(FLAKE_QUARANTINE_SCHEMA),
                    actual.clone(),
                    QUARANTINE_EVIDENCE,
                )?;
            }
            if !is_bounded_text(&record.case_id)
                || !is_bounded_text(&record.reason)
                || !is_bounded_text(&record.owner)
                || !is_digest(&record.evidence_digest)
            {
                emit(
                    &mut findings,
                    "flake.quarantine_invalid",
                    text("bounded_owner_reason_digest"),
                    actual.clone(),
                    QUARANTINE_EVIDENCE,
                )?;
            }
            if record.case_id != input.case_id {
                emit(
                    &mut findings,
                    "flake.quarantine_case_mismatch",
                    text("same_case_identity"),
                    actual.clone(),
                    QUARANTINE_EVIDENCE,
                )?;
            }
            if record.expires_at_unix_ms <= input.evaluated_at_unix_ms {
                emit(
                    &mut findings,
                    "flake.quarantine_expired",
                    text("expiry_after_evaluation_time"),
                    actual.clone(),
                    QUARANTINE_EVIDENCE,
                )?;
            } else if record.expires_at_unix_ms - input.evaluated_at_unix_ms > MAX_QUARANTINE_TTL_MS
            {
                emit(
                    &mut findings,
                    "flake.quarantine_expiry_unbounded",
                    text("bounded_quarantine_lifetime"),
                    actual.clone(),
                    QUARANTINE_EVIDENCE,
                )?;
            }
            if record.evidence_digest != flake_attempt_evidence_digest(&input.attempts) {
                emit(
                    &mut findings,
                    "flake.quarantine_evidence_mismatch",
                    text("attempt_evidence_digest"),
                    actual,
                    QUARANTINE_EVIDENCE,
                )?;
            }
        }
        (None, false) => {}
    }

    Ok(findings)
}

/// Attempt fields that may appear in a finding; the raw case identity is replaced by its shape.
fn attempt_actual(attempt: &FlakeAttempt) -> Value {
    json!({
        "attempt": attempt.attempt,
        "outcome": attempt.outcome,
        "seed": attempt.seed,
        "case_bounded": is_bounded_text(&attempt.case_id),
        "schema_current": attempt.schema == FLAKE_ATTEMPT_SCHEMA,
        "evidence_valid": is_digest(&attempt.evidence_digest),
        "infra_class": attempt.infra_class,
    })
}

/// Quarantine fields that may appear in a finding; free-form text is replaced by its shape.
fn quarantine_actual(record: &FlakeQuarantine) -> Value {
    json!({
        "schema_current": record.schema == FLAKE_QUARANTINE_SCHEMA,
        "case_bounded": is_bounded_text(&record.case_id),
        "reason_bounded": is_bounded_text(&record.reason),
        "owner_bounded": is_bounded_text(&record.owner),
        "expires_at_unix_ms": record.expires_at_unix_ms,
        "evidence_valid": is_digest(&record.evidence_digest),
    })
}

/// Attempt evidence a quarantine digest is derived from.
fn attempt_evidence_value(attempt: &FlakeAttempt) -> Value {
    json!({
        "schema": attempt.schema,
        "attempt": attempt.attempt,
        "case_id": attempt.case_id,
        "seed": attempt.seed,
        "outcome": attempt.outcome,
        "infra_class": attempt.infra_class,
    })
}

/// Bounded, single-line, secret-free identity accepted for a case, reason or owner token.
fn is_bounded_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FLAKE_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
}

fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a secret-shaped
/// identity cannot escape through the finding itself.
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
            format!("flake classifier finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
