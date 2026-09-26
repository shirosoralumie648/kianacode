//! EQ-31 recovery and replay evaluation over caller-supplied evidence.
//!
//! Replay divergence and an externally unknown result are different findings. This evaluator
//! diagnoses both without restarting a process, changing a fence, reconciling an effect, or
//! authorizing a retry.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};

pub const RECOVERY_REPLAY_INPUT_SCHEMA: &str = "kiana.quality-recovery-replay-input.v1";
pub const RECOVERY_REPLAY_EVALUATOR_ID: &str = "recovery-replay";
const CRASH_SCHEMA: &str = "kiana.quality-crash-restart-evidence.v1";
const FENCE_SCHEMA: &str = "kiana.quality-replay-fence-evidence.v1";
const UNKNOWN_SCHEMA: &str = "kiana.quality-unknown-reconcile-evidence.v1";
const REPLAY_SCHEMA: &str = "kiana.quality-replay-evidence.v1";
const MAX_TEXT_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashRestartEvidence {
    pub schema: String,
    pub crash_observed: bool,
    pub restart_observed: bool,
    pub source_cursor_before: u64,
    pub source_cursor_after: u64,
    pub explicit_resume: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayFenceEvidence {
    pub schema: String,
    pub old_fence_digest: String,
    pub new_fence_digest: String,
    pub old_fence_revoked: bool,
    pub new_fence_bound: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownReconcileEvidence {
    pub schema: String,
    pub result_unknown: bool,
    pub reconcile_required: bool,
    pub retry_permitted: bool,
    #[serde(default)]
    pub reconciliation_ref: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayEvidence {
    pub schema: String,
    pub logic_version_expected: String,
    pub logic_version_observed: String,
    pub expected_digest: String,
    pub observed_digest: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReplayInput {
    pub schema: String,
    pub crash_restart: CrashRestartEvidence,
    pub fence: ReplayFenceEvidence,
    pub unknown: UnknownReconcileEvidence,
    pub replay: ReplayEvidence,
}

impl RecoveryReplayInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != RECOVERY_REPLAY_INPUT_SCHEMA {
            return Err("recovery_replay_input_schema_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RecoveryReplayEvaluator;

impl DeterministicEvaluator for RecoveryReplayEvaluator {
    fn evaluator_id(&self) -> &str {
        RECOVERY_REPLAY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: RecoveryReplayInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("recovery_replay"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_recovery_replay(&decoded)
    }
}

pub fn evaluate_recovery_replay(
    input: &RecoveryReplayInput,
) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let crash = &input.crash_restart;
    let fence = &input.fence;
    let unknown = &input.unknown;
    let replay = &input.replay;

    check_schema(
        &mut findings,
        "replay.crash_schema_invalid",
        &crash.schema,
        CRASH_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "replay.fence_schema_invalid",
        &fence.schema,
        FENCE_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "replay.unknown_schema_invalid",
        &unknown.schema,
        UNKNOWN_SCHEMA,
    )?;
    check_schema(
        &mut findings,
        "replay.evidence_schema_invalid",
        &replay.schema,
        REPLAY_SCHEMA,
    )?;

    if crash.source_cursor_before == 0 || crash.source_cursor_after < crash.source_cursor_before {
        emit(
            &mut findings,
            "replay.cursor_invalid",
            "positive_monotonic_restart_cursor",
            "invalid",
        )?;
    }
    if replay.source_cursor_start == 0 || replay.source_cursor_start > replay.source_cursor_end {
        emit(
            &mut findings,
            "replay.cursor_invalid",
            "positive_ordered_replay_cursor",
            "invalid",
        )?;
    }
    if crash.crash_observed && !crash.restart_observed {
        emit(
            &mut findings,
            "replay.restart_missing",
            "restart_after_crash",
            "missing",
        )?;
    }
    if crash.restart_observed && !crash.explicit_resume {
        emit(
            &mut findings,
            "replay.resume_missing",
            "explicit_resume_after_restart",
            "missing",
        )?;
    }

    if !valid_digest(&fence.old_fence_digest) || !valid_digest(&fence.new_fence_digest) {
        emit(
            &mut findings,
            "replay.fence_invalid",
            "sha256_fence_digests",
            "invalid",
        )?;
    }
    if crash.restart_observed && (!fence.old_fence_revoked || !fence.new_fence_bound) {
        emit(
            &mut findings,
            "replay.fence_invalid",
            "old_revoked_new_bound",
            "missing",
        )?;
    }
    if crash.restart_observed && fence.old_fence_digest == fence.new_fence_digest {
        emit(
            &mut findings,
            "replay.fence_reused",
            "new_fence_differs_from_old",
            "reused",
        )?;
    }

    if replay.logic_version_expected.trim().is_empty()
        || replay.logic_version_observed.trim().is_empty()
        || replay.logic_version_expected.len() > MAX_TEXT_BYTES
        || replay.logic_version_observed.len() > MAX_TEXT_BYTES
    {
        emit(
            &mut findings,
            "replay.logic_version_invalid",
            "bounded_logic_versions",
            "invalid",
        )?;
    } else if replay.logic_version_expected != replay.logic_version_observed {
        emit(
            &mut findings,
            "replay.logic_version_drift",
            "same_logic_version",
            "different",
        )?;
    }
    if !valid_digest(&replay.expected_digest) || !valid_digest(&replay.observed_digest) {
        emit(
            &mut findings,
            "replay.digest_invalid",
            "sha256_replay_digests",
            "invalid",
        )?;
    } else if replay.expected_digest != replay.observed_digest {
        emit(
            &mut findings,
            "replay.divergence",
            "expected_digest_equals_observed",
            "different",
        )?;
    }

    if unknown.result_unknown {
        emit(
            &mut findings,
            "replay.result_unknown",
            "effect_outcome_known_or_reconciled",
            "unknown",
        )?;
        if !unknown.reconcile_required {
            emit(
                &mut findings,
                "replay.reconcile_missing",
                "reconcile_required",
                "not_required",
            )?;
        }
        if unknown.retry_permitted {
            emit(
                &mut findings,
                "replay.unknown_retry_forbidden",
                "retry_not_permitted_before_reconcile",
                "retry_permitted",
            )?;
        }
        if unknown
            .reconciliation_ref
            .as_deref()
            .is_none_or(|reference| !valid_reference(reference))
        {
            emit(
                &mut findings,
                "replay.reconciliation_ref_missing",
                "bounded_reconciliation_ref",
                "missing",
            )?;
        }
    } else if unknown.reconcile_required || unknown.retry_permitted {
        emit(
            &mut findings,
            "replay.unknown_contract_invalid",
            "known_result_has_no_unknown_retry_contract",
            "inconsistent",
        )?;
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
            format!("recovery and replay finding: {code}"),
            "replay:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
