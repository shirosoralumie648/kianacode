//! EQ-31 recovery/replay fixtures.
//!
//! Rejection paths come first. Every dimension named by the card - crash/restart, fence,
//! result_unknown, logic version and divergence - has a dedicated rejection fixture before the
//! success paths that keep the full recovery spine green.

use kiana_quality::{
    canonical_findings, digest_findings, CrashRestartEvidence, DeterministicEvaluator,
    EvaluatorRegistry, Finding, RecoveryReplayEvaluator, RecoveryReplayInput, ReplayEvidence,
    ReplayFenceEvidence, RestartAttemptEvidence, UnknownReconcileEvidence,
    EVALUATION_RESULT_SCHEMA, RECOVERY_REPLAY_EVALUATOR_ID, RECOVERY_REPLAY_INPUT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const CRASH_SCHEMA: &str = "kiana.quality-crash-restart-evidence.v1";
const FENCE_SCHEMA: &str = "kiana.quality-replay-fence-evidence.v1";
const ATTEMPT_SCHEMA: &str = "kiana.quality-restart-attempt-evidence.v1";
const UNKNOWN_SCHEMA: &str = "kiana.quality-unknown-reconcile-evidence.v1";
const REPLAY_SCHEMA: &str = "kiana.quality-replay-evidence.v1";

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

fn evidence_refs(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.evidence_ref.clone())
        .collect()
}

fn evaluate(input: &Value) -> Vec<Finding> {
    RecoveryReplayEvaluator
        .evaluate(input)
        .expect("recovery replay evidence decodes")
}

fn valid_input() -> Value {
    json!({
        "schema": RECOVERY_REPLAY_INPUT_SCHEMA,
        "crash_restart": {
            "schema": CRASH_SCHEMA,
            "crash_observed": true,
            "restart_observed": true,
            "explicit_resume": true,
            "resume_ref": "resume:one",
            "source_cursor_before": 10,
            "source_cursor_after": 12,
            "checkpoint_digest": DIGEST,
            "checkpoint_cursor": 10,
        },
        "fence": {
            "schema": FENCE_SCHEMA,
            "old_fence_digest": DIGEST,
            "new_fence_digest": OTHER_DIGEST,
            "old_fence_revoked": true,
            "new_fence_bound": true,
            "old_epoch": 4,
            "new_epoch": 5,
            "old_fence_scope": "fence:workspace/one",
            "new_fence_scope": "fence:workspace/one",
        },
        "attempts": [
            {
                "schema": ATTEMPT_SCHEMA,
                "attempt_id": "attempt:one",
                "fence_epoch": 4,
                "source_cursor": 10,
                "terminal": false,
            },
            {
                "schema": ATTEMPT_SCHEMA,
                "attempt_id": "attempt:two",
                "fence_epoch": 5,
                "source_cursor": 11,
                "terminal": true,
            },
        ],
        "unknown": {
            "schema": UNKNOWN_SCHEMA,
            "result_unknown": false,
            "reconcile_required": false,
            "retry_permitted": false,
            "reconciliation_outcome": "not_attempted",
        },
        "replay": {
            "schema": REPLAY_SCHEMA,
            "logic_version_expected": "reducer-v1",
            "logic_version_observed": "reducer-v1",
            "expected_digest": DIGEST,
            "observed_digest": DIGEST,
            "source_cursor_start": 9,
            "source_cursor_end": 12,
        },
    })
}

/// An input that drifts every dimension the card names at once, used for the deterministic
/// and registry-wiring fixture.
fn violated_input() -> Value {
    let mut input = valid_input();
    input["crash_restart"]["explicit_resume"] = json!(false);
    input["crash_restart"]["resume_ref"] = json!("resume one");
    input["fence"]["new_fence_digest"] = json!(DIGEST);
    input["replay"]["logic_version_observed"] = json!("reducer-v2");
    input["replay"]["observed_digest"] = json!(OTHER_DIGEST);
    input["unknown"]["result_unknown"] = json!(true);
    input["unknown"]["retry_permitted"] = json!(true);
    input
}

#[test]
fn replay_divergence_and_unknown_are_distinct_findings() {
    let mut input = valid_input();
    input["replay"]["observed_digest"] = json!(OTHER_DIGEST);
    input["replay"]["logic_version_observed"] = json!("reducer-v2");
    input["unknown"]["result_unknown"] = json!(true);
    input["unknown"]["reconcile_required"] = json!(false);
    input["unknown"]["retry_permitted"] = json!(true);
    input["unknown"]["reconciliation_ref"] = Value::Null;
    input["unknown"]["reconciliation_outcome"] = json!("not_attempted");

    let findings = evaluate(&input);
    let seen = codes(&findings);
    assert!(seen.contains("replay.divergence"));
    assert!(seen.contains("replay.result_unknown"));
    assert!(seen.contains("replay.logic_version_drift"));
    assert!(seen.contains("replay.divergence_ref_missing"));
    assert!(seen.contains("replay.reconcile_missing"));
    assert!(seen.contains("replay.reconciliation_ref_missing"));
    assert!(seen.contains("replay.unknown_retry_forbidden"));
    assert_eq!(count(&findings, "replay.divergence"), 1);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(findings.len(), 7);

    // Divergence and Unknown are separate findings, bound to separate evidence sections.
    let divergence = findings
        .iter()
        .find(|finding| finding.code == "replay.divergence")
        .unwrap();
    let unknown = findings
        .iter()
        .find(|finding| finding.code == "replay.result_unknown")
        .unwrap();
    assert_ne!(divergence.code, unknown.code);
    assert_eq!(divergence.evidence_ref, "replay:replay");
    assert_eq!(unknown.evidence_ref, "replay:unknown");
    assert_eq!(divergence.expected, json!(DIGEST));
    assert_eq!(divergence.actual, json!(OTHER_DIGEST));
    assert_eq!(
        unknown.expected,
        json!("effect_outcome_known_or_reconciled")
    );
    assert_eq!(unknown.actual, json!("unknown"));

    // An unknown outcome never permits a retry before reconciliation.
    let retry = findings
        .iter()
        .find(|finding| finding.code == "replay.unknown_retry_forbidden")
        .unwrap();
    assert_eq!(retry.evidence_ref, "replay:unknown");
    assert_eq!(retry.actual["retry_permitted"], json!(true));
    assert_eq!(
        retry.expected,
        json!("retry_only_after_effect_proven_absent")
    );
}

#[test]
fn equivalent_restart_replay_has_no_findings() {
    let findings = evaluate(&valid_input());
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn crash_restart_and_resume_contract_is_fail_closed() {
    // A crash without a restart is denied before any fenced replay is attempted.
    let mut input = valid_input();
    input["crash_restart"]["restart_observed"] = json!(false);
    input["crash_restart"]["explicit_resume"] = json!(false);
    input["crash_restart"]["resume_ref"] = Value::Null;
    let findings = evaluate(&input);
    assert_eq!(count(&findings, "replay.restart_missing"), 1);
    assert_eq!(findings.len(), 1);

    // A restart without an explicit resume is denied as well.
    input["crash_restart"]["restart_observed"] = json!(true);
    let findings = evaluate(&input);
    assert_eq!(count(&findings, "replay.resume_missing"), 1);
    assert_eq!(findings.len(), 1);

    // The explicit resume is accepted once it cites a bounded reference.
    input["crash_restart"]["explicit_resume"] = json!(true);
    input["crash_restart"]["resume_ref"] = json!("resume:two");
    assert!(evaluate(&input).is_empty());

    // A stray resume reference without an explicit resume stays visible.
    input["crash_restart"]["explicit_resume"] = json!(false);
    let findings = evaluate(&input);
    assert_eq!(count(&findings, "replay.resume_missing"), 1);
    assert_eq!(count(&findings, "replay.resume_ref_unexpected"), 1);
    assert_eq!(findings.len(), 2);

    // A restart without a prior crash (for example a planned deployment restart) is allowed.
    let mut planned = valid_input();
    planned["crash_restart"]["crash_observed"] = json!(false);
    assert!(evaluate(&planned).is_empty());
}

#[test]
fn resume_and_checkpoint_refs_are_bounded_and_bound_to_the_restart() {
    let mut input = valid_input();
    input["crash_restart"]["resume_ref"] = json!("resume one");
    let findings = evaluate(&input);
    assert_eq!(count(&findings, "replay.resume_ref_missing"), 1);
    assert_eq!(findings.len(), 1);
    let missing = findings
        .iter()
        .find(|finding| finding.code == "replay.resume_ref_missing")
        .unwrap();
    assert_eq!(missing.evidence_ref, "replay:crash-restart");
    assert_eq!(missing.actual["resume_ref"], json!("resume one"));

    let mut oversized = valid_input();
    oversized["crash_restart"]["resume_ref"] = json!("r".repeat(513));
    assert_eq!(count(&evaluate(&oversized), "replay.resume_ref_missing"), 1);

    // The restart must cite a strict checkpoint digest at the pre-restart cursor.
    let mut bad_digest = valid_input();
    bad_digest["crash_restart"]["checkpoint_digest"] = json!("sha256:short");
    let findings = evaluate(&bad_digest);
    assert_eq!(count(&findings, "replay.checkpoint_invalid"), 1);
    assert_eq!(count(&findings, "replay.checkpoint_cursor_mismatch"), 0);

    let mut drifted_cursor = valid_input();
    drifted_cursor["crash_restart"]["checkpoint_cursor"] = json!(11);
    assert_eq!(
        count(
            &evaluate(&drifted_cursor),
            "replay.checkpoint_cursor_mismatch"
        ),
        1
    );

    let mut zero_cursor = valid_input();
    zero_cursor["crash_restart"]["checkpoint_cursor"] = json!(0);
    assert_eq!(
        count(&evaluate(&zero_cursor), "replay.checkpoint_invalid"),
        1
    );

    // Cursors must stay positive and monotonic across the restart.
    let mut regressed = valid_input();
    regressed["crash_restart"]["source_cursor_after"] = json!(9);
    let findings = evaluate(&regressed);
    assert_eq!(count(&findings, "replay.cursor_invalid"), 1);
    assert_eq!(count(&findings, "replay.cursor_not_covered"), 0);
}

#[test]
fn fence_revocation_reuse_and_epoch_advance_are_enforced() {
    let mut unrevoked = valid_input();
    unrevoked["fence"]["old_fence_revoked"] = json!(false);
    unrevoked["fence"]["new_fence_bound"] = json!(false);
    let findings = evaluate(&unrevoked);
    assert_eq!(count(&findings, "replay.fence_invalid"), 1);
    let invalid = findings
        .iter()
        .find(|finding| finding.code == "replay.fence_invalid")
        .unwrap();
    assert_eq!(invalid.expected, json!("old_revoked_new_bound"));
    assert_eq!(invalid.evidence_ref, "replay:fence");

    let mut bad_digest = valid_input();
    bad_digest["fence"]["old_fence_digest"] = json!("sha256:zz");
    let findings = evaluate(&bad_digest);
    assert_eq!(count(&findings, "replay.fence_invalid"), 1);
    assert_eq!(count(&findings, "replay.fence_reused"), 0);

    // Reusing the old fence digest for the new session is denied.
    let mut reused = valid_input();
    reused["fence"]["new_fence_digest"] = json!(DIGEST);
    let findings = evaluate(&reused);
    assert_eq!(count(&findings, "replay.fence_reused"), 1);
    assert_eq!(findings.len(), 1);

    // The fence epoch must advance; a stale epoch also breaks the attempt/fence binding.
    let mut stale_epoch = valid_input();
    stale_epoch["fence"]["new_epoch"] = json!(4);
    let findings = evaluate(&stale_epoch);
    assert_eq!(count(&findings, "replay.fence_epoch_not_advanced"), 1);
    assert_eq!(count(&findings, "replay.fence_epoch_mismatch"), 1);
    assert_eq!(findings.len(), 2);
}

#[test]
fn fence_scope_must_stay_bounded_and_stable() {
    let mut empty_scope = valid_input();
    empty_scope["fence"]["old_fence_scope"] = json!("");
    let findings = evaluate(&empty_scope);
    assert_eq!(count(&findings, "replay.fence_scope_invalid"), 1);
    assert_eq!(count(&findings, "replay.fence_scope_mismatch"), 0);
    assert_eq!(findings.len(), 1);

    let mut drifted_scope = valid_input();
    drifted_scope["fence"]["new_fence_scope"] = json!("fence:workspace/two");
    let findings = evaluate(&drifted_scope);
    assert_eq!(count(&findings, "replay.fence_scope_mismatch"), 1);
    let mismatch = findings
        .iter()
        .find(|finding| finding.code == "replay.fence_scope_mismatch")
        .unwrap();
    assert_eq!(mismatch.actual, json!("fence:workspace/two"));
    assert_eq!(mismatch.evidence_ref, "replay:fence");

    let mut spaced_scope = valid_input();
    spaced_scope["fence"]["old_fence_scope"] = json!("fence:workspace one");
    assert_eq!(
        count(&evaluate(&spaced_scope), "replay.fence_scope_invalid"),
        1
    );

    let mut oversized_scope = valid_input();
    oversized_scope["fence"]["new_fence_scope"] = json!("f".repeat(513));
    assert_eq!(
        count(&evaluate(&oversized_scope), "replay.fence_scope_invalid"),
        1
    );
}

#[test]
fn attempt_epochs_cursors_and_terminal_order_are_bounded() {
    // Two attempts sharing a fence epoch, with a cursor that moves backwards.
    let mut reused_epoch = valid_input();
    reused_epoch["attempts"] = json!([
        {
            "schema": ATTEMPT_SCHEMA,
            "attempt_id": "attempt:one",
            "fence_epoch": 5,
            "source_cursor": 11,
            "terminal": false,
        },
        {
            "schema": ATTEMPT_SCHEMA,
            "attempt_id": "attempt:two",
            "fence_epoch": 5,
            "source_cursor": 10,
            "terminal": false,
        },
    ]);
    let findings = evaluate(&reused_epoch);
    assert_eq!(count(&findings, "replay.attempt_fence_reused"), 1);
    assert_eq!(count(&findings, "replay.attempt_cursor_regressed"), 1);
    assert_eq!(findings.len(), 2);
    assert!(evidence_refs(&findings).contains("replay:attempt:1"));

    // Zero epoch and zero cursor are structural violations.
    let mut zeroes = valid_input();
    zeroes["attempts"] = json!([{
        "schema": ATTEMPT_SCHEMA,
        "attempt_id": "attempt:one",
        "fence_epoch": 0,
        "source_cursor": 0,
        "terminal": false,
    }]);
    let findings = evaluate(&zeroes);
    assert_eq!(count(&findings, "replay.attempt_fence_invalid"), 1);
    assert_eq!(count(&findings, "replay.attempt_cursor_invalid"), 1);
    assert_eq!(count(&findings, "replay.fence_epoch_mismatch"), 1);
    assert_eq!(findings.len(), 3);

    // No attempt may follow a terminal attempt in fence-epoch order.
    let mut after_terminal = valid_input();
    after_terminal["attempts"][0]["terminal"] = json!(true);
    let findings = evaluate(&after_terminal);
    assert_eq!(count(&findings, "replay.attempt_after_terminal"), 1);
    let late = findings
        .iter()
        .find(|finding| finding.code == "replay.attempt_after_terminal")
        .unwrap();
    assert_eq!(late.evidence_ref, "replay:attempt:1");
    assert_eq!(late.actual["attempt_id"], json!("attempt:two"));
    assert_eq!(findings.len(), 1);

    // Duplicate attempt identities are denied, and an unbounded identity is denied once
    // per offending entry.
    let mut duplicate = valid_input();
    duplicate["attempts"][1]["attempt_id"] = json!("attempt:one");
    let findings = evaluate(&duplicate);
    assert_eq!(count(&findings, "replay.attempt_invalid"), 1);
    assert_eq!(findings.len(), 1);

    let mut invalid_id = valid_input();
    invalid_id["attempts"][0]["attempt_id"] = json!("attempt one");
    let findings = evaluate(&invalid_id);
    assert_eq!(count(&findings, "replay.attempt_invalid"), 1);
    assert_eq!(findings.len(), 1);
}

#[test]
fn last_attempt_epoch_must_match_the_new_fence() {
    let mut input = valid_input();
    input["fence"]["new_epoch"] = json!(6);
    let findings = evaluate(&input);
    assert_eq!(count(&findings, "replay.fence_epoch_mismatch"), 1);
    assert_eq!(findings.len(), 1);
    let mismatch = findings
        .iter()
        .find(|finding| finding.code == "replay.fence_epoch_mismatch")
        .unwrap();
    assert_eq!(mismatch.evidence_ref, "replay:attempt:1");
    assert_eq!(mismatch.actual["fence_epoch"], json!(5));
    assert_eq!(
        mismatch.expected,
        json!("last_attempt_epoch_equals_new_fence")
    );
}

#[test]
fn restart_replay_must_cover_the_checkpoint_and_post_restart_cursor() {
    let mut late_start = valid_input();
    late_start["replay"]["source_cursor_start"] = json!(11);
    let findings = evaluate(&late_start);
    assert_eq!(count(&findings, "replay.cursor_not_covered"), 1);
    assert_eq!(findings.len(), 1);

    let mut early_end = valid_input();
    early_end["replay"]["source_cursor_end"] = json!(11);
    let findings = evaluate(&early_end);
    assert_eq!(count(&findings, "replay.cursor_not_covered"), 1);
    assert_eq!(findings.len(), 1);

    let mut zero_start = valid_input();
    zero_start["replay"]["source_cursor_start"] = json!(0);
    let findings = evaluate(&zero_start);
    assert_eq!(count(&findings, "replay.cursor_invalid"), 1);
    assert_eq!(count(&findings, "replay.cursor_not_covered"), 0);

    let mut inverted = valid_input();
    inverted["replay"]["source_cursor_start"] = json!(13);
    inverted["replay"]["source_cursor_end"] = json!(12);
    let findings = evaluate(&inverted);
    assert_eq!(count(&findings, "replay.cursor_invalid"), 1);
    assert_eq!(count(&findings, "replay.cursor_not_covered"), 1);
    assert_eq!(findings.len(), 2);
}

#[test]
fn logic_version_drift_is_separate_from_digest_divergence() {
    let mut drift = valid_input();
    drift["replay"]["logic_version_observed"] = json!("reducer-v2");
    let findings = evaluate(&drift);
    assert_eq!(count(&findings, "replay.logic_version_drift"), 1);
    assert_eq!(count(&findings, "replay.divergence"), 0);
    assert_eq!(findings.len(), 1);
    let drifted = findings
        .iter()
        .find(|finding| finding.code == "replay.logic_version_drift")
        .unwrap();
    assert_eq!(drifted.expected, json!("reducer-v1"));
    assert_eq!(drifted.actual, json!("reducer-v2"));
    assert_eq!(drifted.evidence_ref, "replay:replay");

    let mut divergent = valid_input();
    divergent["replay"]["observed_digest"] = json!(OTHER_DIGEST);
    divergent["replay"]["divergence_ref"] = json!("divergence:one");
    let findings = evaluate(&divergent);
    assert_eq!(count(&findings, "replay.divergence"), 1);
    assert_eq!(count(&findings, "replay.logic_version_drift"), 0);
    assert_eq!(count(&findings, "replay.divergence_ref_missing"), 0);
    assert_eq!(findings.len(), 1);

    // A divergence must cite its recorded evidence.
    let mut uncited = valid_input();
    uncited["replay"]["observed_digest"] = json!(OTHER_DIGEST);
    let findings = evaluate(&uncited);
    assert_eq!(count(&findings, "replay.divergence"), 1);
    assert_eq!(count(&findings, "replay.divergence_ref_missing"), 1);
    assert_eq!(findings.len(), 2);

    let mut bad_digest = valid_input();
    bad_digest["replay"]["expected_digest"] = json!("sha256:zz");
    let findings = evaluate(&bad_digest);
    assert_eq!(count(&findings, "replay.digest_invalid"), 1);
    assert_eq!(count(&findings, "replay.divergence"), 0);
    assert_eq!(findings.len(), 1);

    let mut bad_version = valid_input();
    bad_version["replay"]["logic_version_observed"] = json!("reducer v2");
    let findings = evaluate(&bad_version);
    assert_eq!(count(&findings, "replay.logic_version_invalid"), 1);
    assert_eq!(count(&findings, "replay.logic_version_drift"), 0);
    assert_eq!(findings.len(), 1);

    let mut empty_version = valid_input();
    empty_version["replay"]["logic_version_observed"] = json!("");
    assert_eq!(
        count(&evaluate(&empty_version), "replay.logic_version_invalid"),
        1
    );

    // A divergence reference without a divergence is stale evidence, not a pass.
    let mut stale_ref = valid_input();
    stale_ref["replay"]["divergence_ref"] = json!("divergence:one");
    let findings = evaluate(&stale_ref);
    assert_eq!(count(&findings, "replay.divergence_ref_unexpected"), 1);
    assert_eq!(findings.len(), 1);
}

#[test]
fn unknown_result_requires_reconciliation_and_never_permits_retry() {
    let mut bare_unknown = valid_input();
    bare_unknown["unknown"]["result_unknown"] = json!(true);
    let findings = evaluate(&bare_unknown);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(count(&findings, "replay.reconcile_missing"), 1);
    assert_eq!(count(&findings, "replay.reconciliation_ref_missing"), 1);
    assert_eq!(findings.len(), 3);

    let mut reconciled_applied = valid_input();
    reconciled_applied["unknown"] = json!({
        "schema": UNKNOWN_SCHEMA,
        "result_unknown": true,
        "reconcile_required": true,
        "retry_permitted": true,
        "reconciliation_ref": "reconcile:one",
        "reconciliation_outcome": "reconciled_applied",
    });
    let findings = evaluate(&reconciled_applied);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 1);
    let retry = findings
        .iter()
        .find(|finding| finding.code == "replay.unknown_retry_forbidden")
        .unwrap();
    assert_eq!(
        retry.actual["reconciliation_outcome"],
        json!("reconciled_applied")
    );
    assert_eq!(findings.len(), 2);

    let mut unreconciled_retry = valid_input();
    unreconciled_retry["unknown"] = json!({
        "schema": UNKNOWN_SCHEMA,
        "result_unknown": true,
        "reconcile_required": true,
        "retry_permitted": true,
        "reconciliation_ref": "reconcile:one",
        "reconciliation_outcome": "not_attempted",
    });
    let findings = evaluate(&unreconciled_retry);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 1);
    assert_eq!(findings.len(), 2);

    let mut invalid_outcome = valid_input();
    invalid_outcome["unknown"] = json!({
        "schema": UNKNOWN_SCHEMA,
        "result_unknown": true,
        "reconcile_required": true,
        "retry_permitted": true,
        "reconciliation_ref": "reconcile:one",
        "reconciliation_outcome": "maybe",
    });
    let findings = evaluate(&invalid_outcome);
    assert_eq!(count(&findings, "replay.reconciliation_outcome_invalid"), 1);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 1);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(findings.len(), 3);

    // A reconciled-absent outcome without a retry still reports the unknown outcome.
    let mut settled_without_retry = valid_input();
    settled_without_retry["unknown"] = json!({
        "schema": UNKNOWN_SCHEMA,
        "result_unknown": true,
        "reconcile_required": true,
        "retry_permitted": false,
        "reconciliation_ref": "reconcile:one",
        "reconciliation_outcome": "reconciled_absent",
    });
    let findings = evaluate(&settled_without_retry);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 0);
    assert_eq!(findings.len(), 1);
}

#[test]
fn retry_after_a_proven_absent_effect_is_the_only_retry_path() {
    let mut sanctioned = valid_input();
    sanctioned["unknown"] = json!({
        "schema": UNKNOWN_SCHEMA,
        "result_unknown": true,
        "reconcile_required": true,
        "retry_permitted": true,
        "reconciliation_ref": "reconcile:one",
        "reconciliation_outcome": "reconciled_absent",
    });
    let findings = evaluate(&sanctioned);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 0);
    assert_eq!(findings.len(), 1);

    // The same evidence with an applied effect differs only in the retry denial.
    let mut applied = sanctioned.clone();
    applied["unknown"]["reconciliation_outcome"] = json!("reconciled_applied");
    let findings = evaluate(&applied);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 1);
    assert_eq!(findings.len(), 2);
}

#[test]
fn known_result_cannot_carry_unknown_retry_contract() {
    let mut reconcile_required = valid_input();
    reconcile_required["unknown"]["reconcile_required"] = json!(true);
    let findings = evaluate(&reconcile_required);
    assert_eq!(count(&findings, "replay.unknown_contract_invalid"), 1);
    assert_eq!(findings.len(), 1);

    let mut retry_permitted = valid_input();
    retry_permitted["unknown"]["retry_permitted"] = json!(true);
    let findings = evaluate(&retry_permitted);
    assert_eq!(count(&findings, "replay.unknown_contract_invalid"), 1);
    assert_eq!(count(&findings, "replay.unknown_retry_forbidden"), 0);
    assert_eq!(findings.len(), 1);

    let mut reconciled_outcome = valid_input();
    reconciled_outcome["unknown"]["reconciliation_outcome"] = json!("reconciled_absent");
    let findings = evaluate(&reconciled_outcome);
    assert_eq!(count(&findings, "replay.unknown_contract_invalid"), 1);
    assert_eq!(count(&findings, "replay.reconciliation_ref_missing"), 1);
    assert_eq!(findings.len(), 2);

    let mut reconciled_with_ref = valid_input();
    reconciled_with_ref["unknown"]["reconciliation_outcome"] = json!("reconciled_absent");
    reconciled_with_ref["unknown"]["reconciliation_ref"] = json!("reconcile:one");
    let findings = evaluate(&reconciled_with_ref);
    assert_eq!(count(&findings, "replay.unknown_contract_invalid"), 1);
    assert_eq!(count(&findings, "replay.reconciliation_ref_missing"), 0);
    assert_eq!(findings.len(), 1);

    // A reconciliation reference without any reconcile requirement is stale evidence.
    let mut stale_ref = valid_input();
    stale_ref["unknown"]["reconciliation_ref"] = json!("reconcile:one");
    let findings = evaluate(&stale_ref);
    assert_eq!(count(&findings, "replay.reconciliation_ref_unexpected"), 1);
    assert_eq!(findings.len(), 1);
}

#[test]
fn schema_drift_on_every_recovery_section_is_rejected() {
    let mut input = valid_input();
    input["crash_restart"]["schema"] = json!("kiana.quality-crash-restart-evidence.v0");
    input["fence"]["schema"] = json!("kiana.quality-replay-fence-evidence.v0");
    input["attempts"][0]["schema"] = json!("kiana.quality-restart-attempt-evidence.v0");
    input["unknown"]["schema"] = json!("kiana.quality-unknown-reconcile-evidence.v0");
    input["replay"]["schema"] = json!("kiana.quality-replay-evidence.v0");

    let findings = evaluate(&input);
    for code in [
        "replay.crash_schema_invalid",
        "replay.fence_schema_invalid",
        "replay.attempt_schema_invalid",
        "replay.unknown_schema_invalid",
        "replay.evidence_schema_invalid",
    ] {
        assert_eq!(count(&findings, code), 1, "missing schema drift: {code}");
    }
    assert_eq!(count(&findings, "replay.attempt_schema_invalid"), 1);
    assert_eq!(findings.len(), 5);
}

#[test]
fn unknown_fields_and_envelope_drift_fail_closed() {
    let mut envelope = valid_input();
    envelope["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&envelope).is_err());

    let mut crash = valid_input();
    crash["crash_restart"]["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&crash).is_err());

    let mut fence = valid_input();
    fence["fence"]["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&fence).is_err());

    let mut attempt = valid_input();
    attempt["attempts"][0]["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&attempt).is_err());

    let mut unknown = valid_input();
    unknown["unknown"]["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&unknown).is_err());

    let mut replay = valid_input();
    replay["replay"]["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&replay).is_err());

    let mut wrong_schema = valid_input();
    wrong_schema["schema"] = json!("kiana.quality-recovery-replay-input.v0");
    assert!(RecoveryReplayEvaluator.evaluate(&wrong_schema).is_err());

    let mut missing_fence = valid_input();
    missing_fence.as_object_mut().unwrap().remove("fence");
    assert!(RecoveryReplayEvaluator.evaluate(&missing_fence).is_err());

    let mut attempts_not_an_array = valid_input();
    attempts_not_an_array["attempts"] = json!("attempt:one");
    assert!(RecoveryReplayEvaluator
        .evaluate(&attempts_not_an_array)
        .is_err());

    let mut epoch_wrong_type = valid_input();
    epoch_wrong_type["fence"]["new_epoch"] = json!("5");
    assert!(RecoveryReplayEvaluator.evaluate(&epoch_wrong_type).is_err());

    let mut resume_ref_wrong_type = valid_input();
    resume_ref_wrong_type["crash_restart"]["resume_ref"] = json!(42);
    assert!(RecoveryReplayEvaluator
        .evaluate(&resume_ref_wrong_type)
        .is_err());

    let mut resumed_wrong_type = valid_input();
    resumed_wrong_type["crash_restart"]["explicit_resume"] = json!("true");
    assert!(RecoveryReplayEvaluator
        .evaluate(&resumed_wrong_type)
        .is_err());

    let mut unknown_wrong_shape = valid_input();
    unknown_wrong_shape["unknown"] = json!(["result_unknown"]);
    assert!(RecoveryReplayEvaluator
        .evaluate(&unknown_wrong_shape)
        .is_err());

    // Attempt evidence is optional, and omitting it keeps the rest of the spine valid.
    let mut omitted_attempts = valid_input();
    omitted_attempts.as_object_mut().unwrap().remove("attempts");
    assert!(evaluate(&omitted_attempts).is_empty());

    // The reconciliation outcome defaults to `not_attempted` when omitted.
    let mut omitted_outcome = valid_input();
    omitted_outcome["unknown"]
        .as_object_mut()
        .unwrap()
        .remove("reconciliation_outcome");
    assert!(evaluate(&omitted_outcome).is_empty());

    assert!(RecoveryReplayEvaluator.evaluate(&json!([])).is_err());
    assert!(RecoveryReplayEvaluator
        .evaluate(&json!("recovery"))
        .is_err());
}

#[test]
fn bounded_references_and_versions_fail_closed() {
    let mut oversized_attempt = valid_input();
    oversized_attempt["attempts"][0]["attempt_id"] = json!("a".repeat(513));
    let findings = evaluate(&oversized_attempt);
    assert_eq!(count(&findings, "replay.attempt_invalid"), 1);
    assert_eq!(findings.len(), 1);

    let mut empty_version = valid_input();
    empty_version["replay"]["logic_version_expected"] = json!("");
    assert_eq!(
        count(&evaluate(&empty_version), "replay.logic_version_invalid"),
        1
    );

    let mut oversized_version = valid_input();
    oversized_version["replay"]["logic_version_observed"] = json!("v".repeat(513));
    assert_eq!(
        count(
            &evaluate(&oversized_version),
            "replay.logic_version_invalid"
        ),
        1
    );

    let mut spaced_ref = valid_input();
    spaced_ref["unknown"] = json!({
        "schema": UNKNOWN_SCHEMA,
        "result_unknown": true,
        "reconcile_required": true,
        "retry_permitted": false,
        "reconciliation_ref": "reconcile one",
        "reconciliation_outcome": "reconciled_absent",
    });
    let findings = evaluate(&spaced_ref);
    assert_eq!(count(&findings, "replay.reconciliation_ref_missing"), 1);
    assert_eq!(count(&findings, "replay.result_unknown"), 1);
    assert_eq!(findings.len(), 2);

    let mut empty_ref = valid_input();
    empty_ref["crash_restart"]["resume_ref"] = json!("");
    assert_eq!(count(&evaluate(&empty_ref), "replay.resume_ref_missing"), 1);
}

#[test]
fn oversized_attempts_fail_closed_before_evaluation() {
    let attempts: Vec<Value> = (0_u64..65)
        .map(|index| {
            json!({
                "schema": ATTEMPT_SCHEMA,
                "attempt_id": format!("attempt:{index}"),
                "fence_epoch": index + 1,
                "source_cursor": index + 1,
                "terminal": false,
            })
        })
        .collect();
    let mut input = valid_input();
    input["attempts"] = Value::Array(attempts);
    assert!(RecoveryReplayEvaluator.evaluate(&input).is_err());
    let decoded: RecoveryReplayInput = serde_json::from_value(input).unwrap();
    assert_eq!(
        decoded.validate(),
        Err("recovery_replay_attempt_limit_exceeded")
    );

    let attempts: Vec<Value> = (0_u64..64)
        .map(|index| {
            json!({
                "schema": ATTEMPT_SCHEMA,
                "attempt_id": format!("attempt:{index}"),
                "fence_epoch": index + 1,
                "source_cursor": index + 1,
                "terminal": false,
            })
        })
        .collect();
    let mut bounded = valid_input();
    bounded["attempts"] = Value::Array(attempts);
    let decoded: RecoveryReplayInput = serde_json::from_value(bounded).unwrap();
    assert_eq!(decoded.validate(), Ok(()));
}

#[test]
fn attempt_order_does_not_change_the_findings() {
    let attempts = json!([
        {
            "schema": ATTEMPT_SCHEMA,
            "attempt_id": "attempt:one",
            "fence_epoch": 4,
            "source_cursor": 10,
            "terminal": false,
        },
        {
            "schema": ATTEMPT_SCHEMA,
            "attempt_id": "attempt:two",
            "fence_epoch": 4,
            "source_cursor": 9,
            "terminal": false,
        },
        {
            "schema": ATTEMPT_SCHEMA,
            "attempt_id": "attempt:three",
            "fence_epoch": 5,
            "source_cursor": 8,
            "terminal": false,
        },
    ]);
    let mut input = valid_input();
    input["attempts"] = attempts;

    let mut reversed = input.clone();
    let mut entries = reversed["attempts"].as_array().unwrap().clone();
    entries.reverse();
    reversed["attempts"] = json!(entries);

    let first = canonical_findings(evaluate(&input)).unwrap();
    let second = canonical_findings(evaluate(&reversed)).unwrap();
    assert_eq!(first, second);
    assert_eq!(count(&first, "replay.attempt_fence_reused"), 1);
    assert_eq!(count(&first, "replay.attempt_cursor_regressed"), 2);
    assert!(evidence_refs(&first).contains("replay:attempt:1"));
    assert!(evidence_refs(&first).contains("replay:attempt:2"));
}

#[test]
fn redaction_boundary_covers_caller_supplied_values() {
    let mut input = valid_input();
    input["crash_restart"]["resume_ref"] = json!("token=supersecret");
    let findings = evaluate(&input);
    let missing = findings
        .iter()
        .find(|finding| finding.code == "replay.resume_ref_missing")
        .unwrap();
    assert_eq!(missing.actual["resume_ref"], json!("token=[REDACTED]"));
    assert!(!missing.actual.to_string().contains("supersecret"));

    let mut attempt = valid_input();
    attempt["attempts"][0]["attempt_id"] = json!("token=supersecret");
    let findings = evaluate(&attempt);
    let invalid = findings
        .iter()
        .find(|finding| finding.code == "replay.attempt_invalid")
        .unwrap();
    assert_eq!(invalid.actual, json!("token=[REDACTED]"));
    assert!(!invalid.actual.to_string().contains("supersecret"));
}

#[test]
fn recovery_contracts_round_trip_through_typed_input() {
    let typed = RecoveryReplayInput::new(
        CrashRestartEvidence {
            schema: CRASH_SCHEMA.to_owned(),
            crash_observed: true,
            restart_observed: true,
            explicit_resume: true,
            resume_ref: Some("resume:one".to_owned()),
            source_cursor_before: 10,
            source_cursor_after: 12,
            checkpoint_digest: DIGEST.to_owned(),
            checkpoint_cursor: 10,
        },
        ReplayFenceEvidence {
            schema: FENCE_SCHEMA.to_owned(),
            old_fence_digest: DIGEST.to_owned(),
            new_fence_digest: OTHER_DIGEST.to_owned(),
            old_fence_revoked: true,
            new_fence_bound: true,
            old_epoch: 4,
            new_epoch: 5,
            old_fence_scope: "fence:workspace/one".to_owned(),
            new_fence_scope: "fence:workspace/one".to_owned(),
        },
        vec![RestartAttemptEvidence {
            schema: ATTEMPT_SCHEMA.to_owned(),
            attempt_id: "attempt:two".to_owned(),
            fence_epoch: 5,
            source_cursor: 12,
            terminal: true,
        }],
        UnknownReconcileEvidence {
            schema: UNKNOWN_SCHEMA.to_owned(),
            result_unknown: false,
            reconcile_required: false,
            retry_permitted: false,
            reconciliation_ref: None,
            reconciliation_outcome: "not_attempted".to_owned(),
        },
        ReplayEvidence {
            schema: REPLAY_SCHEMA.to_owned(),
            logic_version_expected: "reducer-v1".to_owned(),
            logic_version_observed: "reducer-v1".to_owned(),
            expected_digest: DIGEST.to_owned(),
            observed_digest: DIGEST.to_owned(),
            divergence_ref: None,
            source_cursor_start: 9,
            source_cursor_end: 12,
        },
    );
    assert_eq!(typed.validate(), Ok(()));
    let value = typed.as_value();
    assert_eq!(value["schema"], json!(RECOVERY_REPLAY_INPUT_SCHEMA));
    let decoded: RecoveryReplayInput = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, typed);
    assert!(RecoveryReplayEvaluator.evaluate(&value).unwrap().is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-recovery-replay-input.v0");
    assert!(RecoveryReplayEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn recovery_output_is_deterministic_evidence_bound_and_registry_wired() {
    let input = violated_input();
    let first = evaluate(&input);
    let second = evaluate(&input);
    assert_eq!(first, second);
    let digest = digest_findings(&first).unwrap();
    assert_eq!(digest, digest_findings(&second).unwrap());
    assert_eq!(
        codes(&first),
        set(&[
            "replay.resume_missing",
            "replay.resume_ref_unexpected",
            "replay.fence_reused",
            "replay.logic_version_drift",
            "replay.divergence",
            "replay.divergence_ref_missing",
            "replay.result_unknown",
            "replay.reconcile_missing",
            "replay.reconciliation_ref_missing",
            "replay.unknown_retry_forbidden",
        ])
    );

    for finding in &first {
        assert!(
            finding.code.starts_with("replay."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("replay:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry.register(RecoveryReplayEvaluator).unwrap();
    assert_eq!(registry.evaluator_ids(), vec![RECOVERY_REPLAY_EVALUATOR_ID]);
    let canonical = canonical_findings(first.clone()).unwrap();
    assert_eq!(registry.evaluate(&input).unwrap(), canonical);

    let result = registry.evaluate_with_digest(&input).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical);
    assert_eq!(result.findings_digest, digest_findings(&canonical).unwrap());
    assert!(registry
        .evaluate(&json!({"schema": "kiana.quality-recovery-replay-input.v0"}))
        .is_err());
}
