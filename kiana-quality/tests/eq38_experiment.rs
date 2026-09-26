use kiana_quality::{
    replay_experiment, DeterministicEvaluator, ExperimentReplayEvaluator,
    EXPERIMENT_REPLAY_INPUT_SCHEMA,
};
use serde_json::json;

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn event(
    sequence: u64,
    kind: &str,
    case_id: Option<&str>,
    result_digest: Option<&str>,
) -> serde_json::Value {
    json!({
        "schema": "kiana.quality-experiment-event.v1",
        "sequence": sequence,
        "kind": kind,
        "case_id": case_id,
        "result_digest": result_digest,
    })
}

fn input(events: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "schema": EXPERIMENT_REPLAY_INPUT_SCHEMA,
        "experiment_id": "experiment:one",
        "suite_ref": "suite:one",
        "events": events,
    })
}

#[test]
fn experiment_state_is_replayable_from_events() {
    let value = input(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(DIGEST)),
        event(4, "completed", None, None),
    ]);
    let decoded: kiana_quality::ExperimentReplayInput =
        serde_json::from_value(value.clone()).unwrap();
    let experiment = replay_experiment(&decoded).unwrap();
    assert_eq!(
        experiment.status,
        kiana_quality::ExperimentStatus::Completed
    );
    assert_eq!(experiment.case_results["case:one"].result_digest, DIGEST);
    assert_eq!(experiment.terminal_sequence, Some(4));
    assert!(ExperimentReplayEvaluator
        .evaluate(&value)
        .unwrap()
        .is_empty());
}

#[test]
fn case_result_conflict_and_event_after_terminal_are_blocking() {
    let conflict = input(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(DIGEST)),
        event(
            4,
            "case_completed",
            Some("case:one"),
            Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ),
        event(5, "completed", None, None),
    ]);
    let findings = ExperimentReplayEvaluator.evaluate(&conflict).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.code == "experiment.replay_invalid"));

    let after_terminal = input(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "completed", None, None),
        event(4, "case_completed", Some("case:late"), Some(DIGEST)),
    ]);
    assert!(ExperimentReplayEvaluator
        .evaluate(&after_terminal)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "experiment.replay_invalid"));
}

#[test]
fn missing_admission_terminal_and_unknown_fields_fail_closed() {
    let missing_terminal = input(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
    ]);
    assert!(ExperimentReplayEvaluator
        .evaluate(&missing_terminal)
        .unwrap()
        .iter()
        .any(|finding| finding.code == "experiment.replay_invalid"));

    let mut unknown = input(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
    ]);
    unknown["unexpected"] = json!(true);
    assert!(ExperimentReplayEvaluator.evaluate(&unknown).is_err());
}
