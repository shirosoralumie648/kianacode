use kiana_quality::{
    DeterministicEvaluator, RecoveryReplayEvaluator, RECOVERY_REPLAY_INPUT_SCHEMA,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_DIGEST: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn valid_input() -> Value {
    json!({
        "schema": RECOVERY_REPLAY_INPUT_SCHEMA,
        "crash_restart": {
            "schema": "kiana.quality-crash-restart-evidence.v1",
            "crash_observed": true,
            "restart_observed": true,
            "source_cursor_before": 10,
            "source_cursor_after": 12,
            "explicit_resume": true,
        },
        "fence": {
            "schema": "kiana.quality-replay-fence-evidence.v1",
            "old_fence_digest": DIGEST,
            "new_fence_digest": OTHER_DIGEST,
            "old_fence_revoked": true,
            "new_fence_bound": true,
        },
        "unknown": {
            "schema": "kiana.quality-unknown-reconcile-evidence.v1",
            "result_unknown": false,
            "reconcile_required": false,
            "retry_permitted": false,
        },
        "replay": {
            "schema": "kiana.quality-replay-evidence.v1",
            "logic_version_expected": "reducer-v1",
            "logic_version_observed": "reducer-v1",
            "expected_digest": DIGEST,
            "observed_digest": DIGEST,
            "source_cursor_start": 10,
            "source_cursor_end": 12,
        },
    })
}

#[test]
fn equivalent_restart_replay_has_no_findings() {
    let mut input = valid_input();
    input["fence"]["new_fence_digest"] = json!(OTHER_DIGEST);
    let findings = RecoveryReplayEvaluator.evaluate(&input).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
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

    let findings = RecoveryReplayEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"replay.divergence"));
    assert!(codes.contains(&"replay.logic_version_drift"));
    assert!(codes.contains(&"replay.result_unknown"));
    assert!(codes.contains(&"replay.reconcile_missing"));
    assert!(codes.contains(&"replay.unknown_retry_forbidden"));
    assert!(codes.contains(&"replay.reconciliation_ref_missing"));
}

#[test]
fn crash_restart_fence_and_resume_contract_is_fail_closed() {
    let mut input = valid_input();
    input["crash_restart"]["restart_observed"] = json!(false);
    input["crash_restart"]["explicit_resume"] = json!(false);
    input["fence"]["old_fence_revoked"] = json!(false);
    input["fence"]["new_fence_bound"] = json!(false);
    input["fence"]["new_fence_digest"] = json!(DIGEST);
    let findings = RecoveryReplayEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"replay.restart_missing"));
    assert!(codes.contains(&"replay.fence_invalid"));
    assert!(codes.contains(&"replay.fence_reused"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut input = valid_input();
    input["unexpected"] = json!(true);
    assert!(RecoveryReplayEvaluator.evaluate(&input).is_err());
}
