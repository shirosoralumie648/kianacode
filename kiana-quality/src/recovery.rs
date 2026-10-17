//! EQ-31 recovery and replay evaluation over caller-supplied evidence.
//!
//! The evaluator diagnoses a crash/restart session, fence replacement, restart attempt ordering,
//! logic-version compatibility, replay digest divergence and an explicitly unknown external
//! outcome.  Replay divergence and an unknown result stay separate stable findings, and an
//! unknown outcome never permits a retry before reconciliation has proven the effect absent.
//!
//! This is a pure diagnostic over the evidence it is given: it never restarts a process, reads
//! a durable event log, revokes or issues a fence, queries the external effect, performs a
//! reconciliation, or authorizes a retry.  Every caller-supplied value passes through
//! [`Finding::redacted`] before it can reach a finding, and each finding cites the evidence
//! section it was derived from.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// Versioned envelope for one recovery/replay input.
pub const RECOVERY_REPLAY_INPUT_SCHEMA: &str = "kiana.quality-recovery-replay-input.v1";
/// Stable registry identifier for the recovery/replay evaluator.
pub const RECOVERY_REPLAY_EVALUATOR_ID: &str = "recovery-replay";
const CRASH_SCHEMA: &str = "kiana.quality-crash-restart-evidence.v1";
const FENCE_SCHEMA: &str = "kiana.quality-replay-fence-evidence.v1";
const ATTEMPT_SCHEMA: &str = "kiana.quality-restart-attempt-evidence.v1";
const UNKNOWN_SCHEMA: &str = "kiana.quality-unknown-reconcile-evidence.v1";
const REPLAY_SCHEMA: &str = "kiana.quality-replay-evidence.v1";
const MAX_ATTEMPTS: usize = 64;
const MAX_TEXT_BYTES: usize = 512;
const CRASH_EVIDENCE: &str = "replay:crash-restart";
const FENCE_EVIDENCE: &str = "replay:fence";
const ATTEMPT_EVIDENCE: &str = "replay:attempt:";
const UNKNOWN_EVIDENCE: &str = "replay:unknown";
const REPLAY_EVIDENCE: &str = "replay:replay";
const NOT_ATTEMPTED: &str = "not_attempted";
const RECONCILED_APPLIED: &str = "reconciled_applied";
const RECONCILED_ABSENT: &str = "reconciled_absent";

/// The crash/restart session: monotonic source cursors, the durable checkpoint the restart
/// resumed from, and the explicit resume decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashRestartEvidence {
    pub schema: String,
    pub crash_observed: bool,
    pub restart_observed: bool,
    pub explicit_resume: bool,
    /// Bounded reference of the explicit resume decision.  Required once a restart happened.
    #[serde(default)]
    pub resume_ref: Option<String>,
    pub source_cursor_before: u64,
    pub source_cursor_after: u64,
    /// Strict `sha256:` digest of the durable checkpoint the replay starts from.
    pub checkpoint_digest: String,
    /// Source cursor of that checkpoint; must equal [`Self::source_cursor_before`] on restart.
    pub checkpoint_cursor: u64,
}

/// The old/new fence pair that separates the crashed session from the restarted one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayFenceEvidence {
    pub schema: String,
    pub old_fence_digest: String,
    pub new_fence_digest: String,
    pub old_fence_revoked: bool,
    pub new_fence_bound: bool,
    pub old_epoch: u64,
    pub new_epoch: u64,
    pub old_fence_scope: String,
    pub new_fence_scope: String,
}

/// One restart attempt in fence-epoch order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestartAttemptEvidence {
    pub schema: String,
    pub attempt_id: String,
    pub fence_epoch: u64,
    pub source_cursor: u64,
    /// Whether this attempt recorded a terminal result.  No attempt may follow a terminal one.
    pub terminal: bool,
}

/// The externally unknown outcome and what reconciliation has established about it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownReconcileEvidence {
    pub schema: String,
    pub result_unknown: bool,
    pub reconcile_required: bool,
    pub retry_permitted: bool,
    #[serde(default)]
    pub reconciliation_ref: Option<String>,
    /// `not_attempted`, `reconciled_applied` or `reconciled_absent`.  An empty value is
    /// rejected; an omitted value defaults to `not_attempted`.
    #[serde(default = "default_reconciliation_outcome")]
    pub reconciliation_outcome: String,
}

/// The replayed range, its logic versions, its digests and the cited divergence evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayEvidence {
    pub schema: String,
    pub logic_version_expected: String,
    pub logic_version_observed: String,
    pub expected_digest: String,
    pub observed_digest: String,
    /// Bounded reference to the recorded divergence evidence; required when digests differ.
    #[serde(default)]
    pub divergence_ref: Option<String>,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
}

/// Versioned input envelope for [`RecoveryReplayEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReplayInput {
    pub schema: String,
    pub crash_restart: CrashRestartEvidence,
    pub fence: ReplayFenceEvidence,
    #[serde(default)]
    pub attempts: Vec<RestartAttemptEvidence>,
    pub unknown: UnknownReconcileEvidence,
    pub replay: ReplayEvidence,
}

impl RecoveryReplayInput {
    pub fn new(
        crash_restart: CrashRestartEvidence,
        fence: ReplayFenceEvidence,
        attempts: Vec<RestartAttemptEvidence>,
        unknown: UnknownReconcileEvidence,
        replay: ReplayEvidence,
    ) -> Self {
        Self {
            schema: RECOVERY_REPLAY_INPUT_SCHEMA.to_owned(),
            crash_restart,
            fence,
            attempts,
            unknown,
            replay,
        }
    }

    /// Hard input bounds.  Structural drift fails closed before any finding is produced;
    /// entry shape problems remain findings so the report explains *why* the evidence is
    /// unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != RECOVERY_REPLAY_INPUT_SCHEMA {
            return Err("recovery_replay_input_schema_invalid");
        }
        if self.attempts.len() > MAX_ATTEMPTS {
            return Err("recovery_replay_attempt_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("recovery replay input is serializable")
    }
}

/// Pure recovery/replay evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecoveryReplayEvaluator;

impl DeterministicEvaluator for RecoveryReplayEvaluator {
    fn evaluator_id(&self) -> &str {
        RECOVERY_REPLAY_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: RecoveryReplayInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("recovery_replay"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_recovery_replay(&decoded)
    }
}

/// Evaluate the deny-first recovery/replay matrix without reading any store.
///
/// Check order is deliberate: every section schema, the crash/restart and resume contract, the
/// old/new fence pair, the restart attempts in fence-epoch order, the replay versions/digests/
/// cursor range, and finally the unknown/reconciliation contract.  Attempts are ordered by
/// fence epoch and attempt identity before findings are emitted, so the finding set does not
/// depend on the caller's array order while duplicate evidence stays visible.
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
        CRASH_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "replay.fence_schema_invalid",
        &fence.schema,
        FENCE_SCHEMA,
        FENCE_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "replay.unknown_schema_invalid",
        &unknown.schema,
        UNKNOWN_SCHEMA,
        UNKNOWN_EVIDENCE,
    )?;
    check_schema(
        &mut findings,
        "replay.evidence_schema_invalid",
        &replay.schema,
        REPLAY_SCHEMA,
        REPLAY_EVIDENCE,
    )?;

    // Crash/restart contract: positive monotonic cursors, a restart after a crash, an explicit
    // resume after a restart, and a durable checkpoint bound to the pre-restart cursor.
    if crash.source_cursor_before == 0 || crash.source_cursor_after < crash.source_cursor_before {
        emit(
            &mut findings,
            "replay.cursor_invalid",
            text("positive_monotonic_restart_cursor"),
            crash_actual(crash),
            CRASH_EVIDENCE,
        )?;
    }
    if crash.crash_observed && !crash.restart_observed {
        emit(
            &mut findings,
            "replay.restart_missing",
            text("restart_after_crash"),
            text("missing"),
            CRASH_EVIDENCE,
        )?;
    }
    if crash.restart_observed && !crash.explicit_resume {
        emit(
            &mut findings,
            "replay.resume_missing",
            text("explicit_resume_after_restart"),
            text("missing"),
            CRASH_EVIDENCE,
        )?;
    }
    let resume_ref_bounded = crash
        .resume_ref
        .as_deref()
        .is_some_and(is_bounded_reference);
    if crash.explicit_resume && !resume_ref_bounded {
        emit(
            &mut findings,
            "replay.resume_ref_missing",
            text("bounded_resume_ref"),
            crash_actual(crash),
            CRASH_EVIDENCE,
        )?;
    } else if !crash.explicit_resume && crash.resume_ref.is_some() {
        emit(
            &mut findings,
            "replay.resume_ref_unexpected",
            text("no_resume_ref_without_explicit_resume"),
            text("present"),
            CRASH_EVIDENCE,
        )?;
    }
    if crash.restart_observed {
        if !is_sha256_digest(&crash.checkpoint_digest) || crash.checkpoint_cursor == 0 {
            emit(
                &mut findings,
                "replay.checkpoint_invalid",
                text("sha256_checkpoint_digest_and_positive_cursor"),
                crash_actual(crash),
                CRASH_EVIDENCE,
            )?;
        } else if crash.checkpoint_cursor != crash.source_cursor_before {
            emit(
                &mut findings,
                "replay.checkpoint_cursor_mismatch",
                text("checkpoint_at_restart_cursor"),
                crash_actual(crash),
                CRASH_EVIDENCE,
            )?;
        }
    }

    // Fence contract: the old fence is revoked, a different new fence is bound, its epoch
    // advances, and the scope stays stable.
    if !is_sha256_digest(&fence.old_fence_digest) || !is_sha256_digest(&fence.new_fence_digest) {
        emit(
            &mut findings,
            "replay.fence_invalid",
            text("sha256_fence_digests"),
            fence_actual(fence),
            FENCE_EVIDENCE,
        )?;
    }
    if crash.restart_observed && (!fence.old_fence_revoked || !fence.new_fence_bound) {
        emit(
            &mut findings,
            "replay.fence_invalid",
            text("old_revoked_new_bound"),
            fence_actual(fence),
            FENCE_EVIDENCE,
        )?;
    }
    if crash.restart_observed && fence.old_fence_digest == fence.new_fence_digest {
        emit(
            &mut findings,
            "replay.fence_reused",
            text("new_fence_differs_from_old"),
            text("reused"),
            FENCE_EVIDENCE,
        )?;
    }
    if crash.restart_observed && fence.new_epoch <= fence.old_epoch {
        emit(
            &mut findings,
            "replay.fence_epoch_not_advanced",
            text("new_epoch_after_old"),
            fence_actual(fence),
            FENCE_EVIDENCE,
        )?;
    }
    let old_scope_bounded = is_bounded_reference(&fence.old_fence_scope);
    let new_scope_bounded = is_bounded_reference(&fence.new_fence_scope);
    if !old_scope_bounded || !new_scope_bounded {
        emit(
            &mut findings,
            "replay.fence_scope_invalid",
            text("bounded_fence_scopes"),
            fence_actual(fence),
            FENCE_EVIDENCE,
        )?;
    } else if fence.old_fence_scope != fence.new_fence_scope {
        emit(
            &mut findings,
            "replay.fence_scope_mismatch",
            text("shared_fence_scope"),
            text(&fence.new_fence_scope),
            FENCE_EVIDENCE,
        )?;
    }

    // Restart attempts are checked in fence-epoch order, not in caller array order.
    let mut attempts: Vec<&RestartAttemptEvidence> = input.attempts.iter().collect();
    attempts.sort_by(|left, right| attempt_sort_key(left).cmp(&attempt_sort_key(right)));
    let mut attempt_ids = BTreeSet::new();
    let mut previous_epoch = 0_u64;
    let mut previous_cursor = 0_u64;
    let mut terminal_seen = false;
    for (index, attempt) in attempts.iter().enumerate() {
        let attempt_ref = format!("{ATTEMPT_EVIDENCE}{index}");
        check_schema(
            &mut findings,
            "replay.attempt_schema_invalid",
            &attempt.schema,
            ATTEMPT_SCHEMA,
            &attempt_ref,
        )?;
        if !is_bounded_reference(&attempt.attempt_id)
            || !attempt_ids.insert(attempt.attempt_id.clone())
        {
            emit(
                &mut findings,
                "replay.attempt_invalid",
                text("unique_bounded_attempt_id"),
                text(&attempt.attempt_id),
                &attempt_ref,
            )?;
        }
        if attempt.fence_epoch == 0 {
            emit(
                &mut findings,
                "replay.attempt_fence_invalid",
                text("positive_fence_epoch"),
                attempt_actual(attempt),
                &attempt_ref,
            )?;
        } else if previous_epoch != 0 && attempt.fence_epoch <= previous_epoch {
            emit(
                &mut findings,
                "replay.attempt_fence_reused",
                text("strictly_increasing_fence_epoch"),
                attempt_actual(attempt),
                &attempt_ref,
            )?;
        }
        if attempt.source_cursor == 0 {
            emit(
                &mut findings,
                "replay.attempt_cursor_invalid",
                text("positive_attempt_cursor"),
                attempt_actual(attempt),
                &attempt_ref,
            )?;
        } else if previous_cursor != 0 && attempt.source_cursor < previous_cursor {
            emit(
                &mut findings,
                "replay.attempt_cursor_regressed",
                text("monotonic_attempt_cursors"),
                attempt_actual(attempt),
                &attempt_ref,
            )?;
        }
        if terminal_seen {
            emit(
                &mut findings,
                "replay.attempt_after_terminal",
                text("no_attempt_after_terminal"),
                attempt_actual(attempt),
                &attempt_ref,
            )?;
        }
        terminal_seen = terminal_seen || attempt.terminal;
        previous_epoch = previous_epoch.max(attempt.fence_epoch);
        previous_cursor = previous_cursor.max(attempt.source_cursor);
    }
    if crash.restart_observed {
        if let Some(last) = attempts.last() {
            if last.fence_epoch != fence.new_epoch {
                emit(
                    &mut findings,
                    "replay.fence_epoch_mismatch",
                    text("last_attempt_epoch_equals_new_fence"),
                    attempt_actual(last),
                    &format!("{ATTEMPT_EVIDENCE}{}", attempts.len() - 1),
                )?;
            }
        }
    }

    // Replay contract: the logic version records compatible machinery, the digest pair is
    // strictly comparable, a divergence cites its evidence, and the replayed cursor range
    // covers the checkpoint and the post-restart cursor.
    let version_expected_bounded = is_bounded_reference(&replay.logic_version_expected);
    let version_observed_bounded = is_bounded_reference(&replay.logic_version_observed);
    if !version_expected_bounded || !version_observed_bounded {
        emit(
            &mut findings,
            "replay.logic_version_invalid",
            text("bounded_logic_versions"),
            replay_actual(replay),
            REPLAY_EVIDENCE,
        )?;
    } else if replay.logic_version_expected != replay.logic_version_observed {
        emit(
            &mut findings,
            "replay.logic_version_drift",
            text(&replay.logic_version_expected),
            text(&replay.logic_version_observed),
            REPLAY_EVIDENCE,
        )?;
    }
    let digests_valid =
        is_sha256_digest(&replay.expected_digest) && is_sha256_digest(&replay.observed_digest);
    if !digests_valid {
        emit(
            &mut findings,
            "replay.digest_invalid",
            text("sha256_replay_digests"),
            replay_actual(replay),
            REPLAY_EVIDENCE,
        )?;
    }
    let diverged = digests_valid && replay.expected_digest != replay.observed_digest;
    if diverged {
        emit(
            &mut findings,
            "replay.divergence",
            text(&replay.expected_digest),
            text(&replay.observed_digest),
            REPLAY_EVIDENCE,
        )?;
    }
    let divergence_ref_bounded = replay
        .divergence_ref
        .as_deref()
        .is_some_and(is_bounded_reference);
    if diverged && !divergence_ref_bounded {
        emit(
            &mut findings,
            "replay.divergence_ref_missing",
            text("bounded_divergence_ref"),
            text("missing"),
            REPLAY_EVIDENCE,
        )?;
    } else if digests_valid && !diverged && replay.divergence_ref.is_some() {
        emit(
            &mut findings,
            "replay.divergence_ref_unexpected",
            text("no_divergence_ref_without_divergence"),
            text("present"),
            REPLAY_EVIDENCE,
        )?;
    }
    if replay.source_cursor_start == 0 || replay.source_cursor_start > replay.source_cursor_end {
        emit(
            &mut findings,
            "replay.cursor_invalid",
            text("positive_ordered_replay_cursor"),
            replay_actual(replay),
            REPLAY_EVIDENCE,
        )?;
    }
    if crash.restart_observed
        && (replay.source_cursor_start > crash.checkpoint_cursor
            || replay.source_cursor_end < crash.source_cursor_after)
    {
        emit(
            &mut findings,
            "replay.cursor_not_covered",
            text("replay_covers_checkpoint_and_restart_cursor"),
            replay_actual(replay),
            REPLAY_EVIDENCE,
        )?;
    }

    // Unknown contract: an unknown outcome stays visible, demands a bounded reconciliation
    // reference, and never permits a retry unless reconciliation proved the effect absent.
    let outcome_known = matches!(
        unknown.reconciliation_outcome.as_str(),
        NOT_ATTEMPTED | RECONCILED_APPLIED | RECONCILED_ABSENT
    );
    if unknown.result_unknown {
        emit(
            &mut findings,
            "replay.result_unknown",
            text("effect_outcome_known_or_reconciled"),
            text("unknown"),
            UNKNOWN_EVIDENCE,
        )?;
        if !unknown.reconcile_required {
            emit(
                &mut findings,
                "replay.reconcile_missing",
                text("reconcile_required"),
                text("not_required"),
                UNKNOWN_EVIDENCE,
            )?;
        }
    } else if unknown.reconcile_required
        || unknown.retry_permitted
        || (outcome_known && unknown.reconciliation_outcome != NOT_ATTEMPTED)
    {
        emit(
            &mut findings,
            "replay.unknown_contract_invalid",
            text("known_result_has_no_unknown_retry_contract"),
            unknown_actual(unknown),
            UNKNOWN_EVIDENCE,
        )?;
    }
    if !outcome_known {
        emit(
            &mut findings,
            "replay.reconciliation_outcome_invalid",
            text("not_attempted_or_reconciled"),
            unknown_actual(unknown),
            UNKNOWN_EVIDENCE,
        )?;
    }
    if unknown.result_unknown && unknown.retry_permitted {
        let retry_sanctioned = outcome_known && unknown.reconciliation_outcome == RECONCILED_ABSENT;
        if !retry_sanctioned {
            emit(
                &mut findings,
                "replay.unknown_retry_forbidden",
                text("retry_only_after_effect_proven_absent"),
                unknown_actual(unknown),
                UNKNOWN_EVIDENCE,
            )?;
        }
    }
    if outcome_known {
        let reference_needed =
            unknown.result_unknown || unknown.reconciliation_outcome != NOT_ATTEMPTED;
        let reference_bounded = unknown
            .reconciliation_ref
            .as_deref()
            .is_some_and(is_bounded_reference);
        if reference_needed && !reference_bounded {
            emit(
                &mut findings,
                "replay.reconciliation_ref_missing",
                text("bounded_reconciliation_ref"),
                text("missing"),
                UNKNOWN_EVIDENCE,
            )?;
        } else if !reference_needed && unknown.reconciliation_ref.is_some() {
            emit(
                &mut findings,
                "replay.reconciliation_ref_unexpected",
                text("no_reconciliation_ref_without_unknown"),
                text("present"),
                UNKNOWN_EVIDENCE,
            )?;
        }
    }

    Ok(findings)
}

fn default_reconciliation_outcome() -> String {
    NOT_ATTEMPTED.to_owned()
}

fn attempt_sort_key(attempt: &RestartAttemptEvidence) -> (u64, &str, u64) {
    (
        attempt.fence_epoch,
        attempt.attempt_id.as_str(),
        attempt.source_cursor,
    )
}

fn crash_actual(crash: &CrashRestartEvidence) -> Value {
    serde_json::json!({
        "crash_observed": crash.crash_observed,
        "restart_observed": crash.restart_observed,
        "explicit_resume": crash.explicit_resume,
        "resume_ref": crash.resume_ref,
        "source_cursor_before": crash.source_cursor_before,
        "source_cursor_after": crash.source_cursor_after,
        "checkpoint_cursor": crash.checkpoint_cursor,
    })
}

fn fence_actual(fence: &ReplayFenceEvidence) -> Value {
    serde_json::json!({
        "old_fence_digest": fence.old_fence_digest,
        "new_fence_digest": fence.new_fence_digest,
        "old_fence_revoked": fence.old_fence_revoked,
        "new_fence_bound": fence.new_fence_bound,
        "old_epoch": fence.old_epoch,
        "new_epoch": fence.new_epoch,
        "old_fence_scope": fence.old_fence_scope,
        "new_fence_scope": fence.new_fence_scope,
    })
}

fn attempt_actual(attempt: &RestartAttemptEvidence) -> Value {
    serde_json::json!({
        "attempt_id": attempt.attempt_id,
        "fence_epoch": attempt.fence_epoch,
        "source_cursor": attempt.source_cursor,
        "terminal": attempt.terminal,
    })
}

fn unknown_actual(unknown: &UnknownReconcileEvidence) -> Value {
    serde_json::json!({
        "result_unknown": unknown.result_unknown,
        "reconcile_required": unknown.reconcile_required,
        "retry_permitted": unknown.retry_permitted,
        "reconciliation_ref": unknown.reconciliation_ref,
        "reconciliation_outcome": unknown.reconciliation_outcome,
    })
}

fn replay_actual(replay: &ReplayEvidence) -> Value {
    serde_json::json!({
        "logic_version_expected": replay.logic_version_expected,
        "logic_version_observed": replay.logic_version_observed,
        "expected_digest": replay.expected_digest,
        "observed_digest": replay.observed_digest,
        "source_cursor_start": replay.source_cursor_start,
        "source_cursor_end": replay.source_cursor_end,
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

/// Bounded, single-line, secret-free reference accepted in resume/attempt/fence identities.
fn is_bounded_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
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
/// secret-shaped reference cannot escape through the finding itself.
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
            format!("recovery and replay finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
