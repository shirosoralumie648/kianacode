use kiana_quality::{
    canonical_findings, replay_experiment, DeterministicEvaluator, EvalExperiment,
    EvaluatorRegistry, ExperimentEvent, ExperimentEventKind, ExperimentReplayError,
    ExperimentReplayEvaluator, ExperimentReplayInput, ExperimentStatus, Finding,
    EXPERIMENT_EVENT_SCHEMA, EXPERIMENT_REPLAY_EVALUATOR_ID, EXPERIMENT_REPLAY_INPUT_SCHEMA,
    EXPERIMENT_SCHEMA, MAX_EXPERIMENT_EVENTS, MAX_EXPERIMENT_TEXT_BYTES,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn sha(hex: char) -> String {
    format!("sha256:{}", hex.to_string().repeat(64))
}

fn event(sequence: u64, kind: &str, case_id: Option<&str>, result_digest: Option<&str>) -> Value {
    json!({
        "schema": EXPERIMENT_EVENT_SCHEMA,
        "sequence": sequence,
        "kind": kind,
        "case_id": case_id,
        "result_digest": result_digest,
    })
}

fn envelope(events: Vec<Value>) -> Value {
    json!({
        "schema": EXPERIMENT_REPLAY_INPUT_SCHEMA,
        "experiment_id": "experiment:one",
        "suite_ref": "suite:one",
        "events": events,
    })
}

fn decode(value: &Value) -> ExperimentReplayInput {
    serde_json::from_value(value.clone()).expect("experiment replay input")
}

fn evaluated(value: &Value) -> Vec<Finding> {
    ExperimentReplayEvaluator
        .evaluate(value)
        .expect("experiment replay evaluation")
}

/// Every refusal is exactly one stable finding that cites the input section and names the
/// refusal reason as its actual value.
fn refusal(value: &Value) -> String {
    let findings = evaluated(value);
    assert_eq!(findings.len(), 1, "expected one replay refusal");
    assert_eq!(findings[0].code, "experiment.replay_invalid");
    assert_eq!(findings[0].evidence_ref, "experiment:input");
    findings[0]
        .actual
        .as_str()
        .expect("refusal actual is a string")
        .to_owned()
}

#[test]
fn experiment_state_is_replayable_from_events() {
    let digest = sha('a');
    let value = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(4, "completed", None, None),
    ]);
    let experiment = replay_experiment(&decode(&value)).expect("replay");
    assert_eq!(experiment.schema, EXPERIMENT_SCHEMA);
    assert_eq!(experiment.status, ExperimentStatus::Completed);
    assert_eq!(experiment.status.as_str(), "completed");
    assert!(experiment.is_terminal());
    assert_eq!(experiment.terminal_sequence, Some(4));
    let result = experiment.case_result("case:one").expect("indexed result");
    assert_eq!(result.case_id, "case:one");
    assert_eq!(result.result_digest, digest);
    assert_eq!(result.source_sequence, 3);
    assert!(experiment.replay_digest.starts_with("sha256:"));
    assert!(evaluated(&value).is_empty());
}

#[test]
fn sequence_gaps_repeats_and_reordering_are_refused() {
    let gap = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(4, "completed", None, None),
    ]);
    assert_eq!(refusal(&gap), "experiment_event_sequence_invalid");

    let repeated_sequence = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(2, "completed", None, None),
    ]);
    assert_eq!(
        refusal(&repeated_sequence),
        "experiment_event_sequence_invalid"
    );

    let not_starting_at_one = envelope(vec![
        event(0, "admitted", None, None),
        event(1, "completed", None, None),
    ]);
    assert_eq!(
        refusal(&not_starting_at_one),
        "experiment_event_sequence_invalid"
    );
}

#[test]
fn admission_must_be_the_first_event_and_appear_once() {
    let empty = envelope(vec![]);
    assert_eq!(refusal(&empty), "experiment_admission_missing");

    let started_first = envelope(vec![
        event(1, "started", None, None),
        event(2, "completed", None, None),
    ]);
    assert_eq!(refusal(&started_first), "experiment_admission_missing");

    let terminal_first = envelope(vec![event(1, "completed", None, None)]);
    assert_eq!(refusal(&terminal_first), "experiment_admission_missing");

    let admitted_twice = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "admitted", None, None),
        event(3, "completed", None, None),
    ]);
    assert_eq!(refusal(&admitted_twice), "experiment_transition_invalid");
}

#[test]
fn illegal_transitions_are_refused() {
    let result_before_start = envelope(vec![
        event(1, "admitted", None, None),
        event(
            2,
            "case_completed",
            Some("case:one"),
            Some(sha('a').as_str()),
        ),
        event(3, "completed", None, None),
    ]);
    assert_eq!(
        refusal(&result_before_start),
        "experiment_transition_invalid"
    );

    let started_twice = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "started", None, None),
        event(4, "completed", None, None),
    ]);
    assert_eq!(refusal(&started_twice), "experiment_transition_invalid");

    let admitted_after_terminal = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "failed", None, None),
        event(3, "admitted", None, None),
    ]);
    assert_eq!(
        refusal(&admitted_after_terminal),
        "experiment_event_after_terminal"
    );
}

#[test]
fn case_results_require_bounded_identity_and_strict_digest() {
    let binding = |sequence: u64, case_id: Option<&str>, result_digest: Option<&str>| {
        envelope(vec![
            event(1, "admitted", None, None),
            event(2, "started", None, None),
            event(sequence, "case_completed", case_id, result_digest),
            event(sequence + 1, "completed", None, None),
        ])
    };

    let digest = sha('a');
    let missing_case = binding(3, None, Some(digest.as_str()));
    assert_eq!(refusal(&missing_case), "experiment_case_result_invalid");

    let missing_digest = binding(3, Some("case:one"), None);
    assert_eq!(refusal(&missing_digest), "experiment_case_result_invalid");

    let invalid_digest = binding(3, Some("case:one"), Some("sha256:abc"));
    assert_eq!(refusal(&invalid_digest), "experiment_case_result_invalid");

    let unprefixed_digest = binding(
        3,
        Some("case:one"),
        Some(digest.trim_start_matches("sha256:")),
    );
    assert_eq!(
        refusal(&unprefixed_digest),
        "experiment_case_result_invalid"
    );

    let whitespace_case = binding(3, Some("case one"), Some(digest.as_str()));
    assert_eq!(refusal(&whitespace_case), "experiment_case_result_invalid");

    let long_case = "c".repeat(MAX_EXPERIMENT_TEXT_BYTES + 1);
    let oversized_case = binding(3, Some(long_case.as_str()), Some(digest.as_str()));
    assert_eq!(refusal(&oversized_case), "experiment_case_result_invalid");
}

#[test]
fn duplicated_and_drifted_case_results_are_refused() {
    let digest = sha('a');
    let repeated = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(4, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(5, "completed", None, None),
    ]);
    assert_eq!(refusal(&repeated), "experiment_case_result_repeated");

    let drift = sha('b');
    let drifted = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(4, "case_completed", Some("case:one"), Some(drift.as_str())),
        event(5, "completed", None, None),
    ]);
    assert_eq!(refusal(&drifted), "experiment_case_result_conflict");
}

#[test]
fn events_after_a_terminal_state_are_refused() {
    let digest = sha('a');
    for terminal in ["completed", "failed", "cancelled"] {
        let value = envelope(vec![
            event(1, "admitted", None, None),
            event(2, "started", None, None),
            event(3, terminal, None, None),
            event(
                4,
                "case_completed",
                Some("case:late"),
                Some(digest.as_str()),
            ),
        ]);
        assert_eq!(
            refusal(&value),
            "experiment_event_after_terminal",
            "terminal {terminal} must fence late events"
        );
    }

    let two_terminals = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
        event(3, "failed", None, None),
    ]);
    assert_eq!(refusal(&two_terminals), "experiment_event_after_terminal");
}

#[test]
fn a_log_without_a_terminal_state_is_refused() {
    let admitted_only = envelope(vec![event(1, "admitted", None, None)]);
    assert_eq!(refusal(&admitted_only), "experiment_terminal_missing");

    let running = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(
            3,
            "case_completed",
            Some("case:one"),
            Some(sha('a').as_str()),
        ),
    ]);
    assert_eq!(refusal(&running), "experiment_terminal_missing");
}

#[test]
fn every_terminal_state_reduces_deterministically() {
    let digest = sha('a');
    let mut digests = BTreeSet::new();
    for (terminal, expected) in [
        ("completed", ExperimentStatus::Completed),
        ("failed", ExperimentStatus::Failed),
        ("cancelled", ExperimentStatus::Cancelled),
    ] {
        let value = envelope(vec![
            event(1, "admitted", None, None),
            event(2, "started", None, None),
            event(3, "case_completed", Some("case:one"), Some(digest.as_str())),
            event(4, terminal, None, None),
        ]);
        let experiment = replay_experiment(&decode(&value)).expect("replay");
        assert_eq!(experiment.status, expected);
        assert_eq!(experiment.status.as_str(), terminal);
        assert_eq!(experiment.terminal_sequence, Some(4));
        assert!(experiment.is_terminal());
        assert!(
            digests.insert(experiment.replay_digest.clone()),
            "terminal states must carry different replay digests: {terminal}"
        );
        assert!(!ExperimentStatus::Admitted.is_terminal());
        assert!(!ExperimentStatus::Running.is_terminal());
    }

    let cancelled_before_start = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "cancelled", None, None),
    ]);
    let experiment = replay_experiment(&decode(&cancelled_before_start)).expect("replay");
    assert_eq!(experiment.status, ExperimentStatus::Cancelled);
    assert_eq!(experiment.terminal_sequence, Some(2));
    assert!(experiment.case_results.is_empty());
}

#[test]
fn replay_digest_binds_identity_events_and_order() {
    let digest = sha('a');
    let events = vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(4, "completed", None, None),
    ];
    let base = envelope(events);
    let first = replay_experiment(&decode(&base)).expect("replay");
    let second = replay_experiment(&decode(&base)).expect("replay");
    assert_eq!(first, second);
    assert_eq!(first.replay_digest, second.replay_digest);

    let mut renamed = base.clone();
    renamed["experiment_id"] = json!("experiment:two");
    let renamed = replay_experiment(&decode(&renamed)).expect("replay");
    assert_ne!(renamed.replay_digest, first.replay_digest);

    let mut suite_drift = base.clone();
    suite_drift["suite_ref"] = json!("suite:two");
    let suite_drift = replay_experiment(&decode(&suite_drift)).expect("replay");
    assert_ne!(suite_drift.replay_digest, first.replay_digest);

    let without_result = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "completed", None, None),
    ]);
    let without_result = replay_experiment(&decode(&without_result)).expect("replay");
    assert_ne!(without_result.replay_digest, first.replay_digest);

    let other_case = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:two"), Some(digest.as_str())),
        event(4, "completed", None, None),
    ]);
    let other_case = replay_experiment(&decode(&other_case)).expect("replay");
    assert_ne!(other_case.replay_digest, first.replay_digest);
}

#[test]
fn envelope_schema_identity_and_event_limit_fail_closed() {
    let mut drifted = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
    ]);
    drifted["schema"] = json!("kiana.quality-experiment-replay-input.v2");
    assert_eq!(
        replay_experiment(&decode(&drifted)).expect_err("drifted envelope schema"),
        ExperimentReplayError::InputSchemaInvalid
    );
    assert!(ExperimentReplayEvaluator.evaluate(&drifted).is_err());

    let mut whitespace_identity = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
    ]);
    whitespace_identity["experiment_id"] = json!("experiment one");
    assert_eq!(
        replay_experiment(&decode(&whitespace_identity)).expect_err("whitespace identity"),
        ExperimentReplayError::IdentityInvalid
    );

    let mut empty_identity = whitespace_identity.clone();
    empty_identity["experiment_id"] = json!("");
    assert_eq!(
        replay_experiment(&decode(&empty_identity)).expect_err("empty identity"),
        ExperimentReplayError::IdentityInvalid
    );

    let mut unbounded_suite = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
    ]);
    unbounded_suite["suite_ref"] = json!("s".repeat(MAX_EXPERIMENT_TEXT_BYTES + 1));
    assert_eq!(
        replay_experiment(&decode(&unbounded_suite)).expect_err("oversized suite identity"),
        ExperimentReplayError::IdentityInvalid
    );

    let oversized: Vec<Value> = (1..=(MAX_EXPERIMENT_EVENTS as u64 + 1))
        .map(|sequence| event(sequence, "started", None, None))
        .collect();
    let oversized = envelope(oversized);
    assert_eq!(
        replay_experiment(&decode(&oversized)).expect_err("event limit"),
        ExperimentReplayError::EventLimitExceeded
    );
}

#[test]
fn event_schema_drift_and_unknown_fields_are_refused() {
    let mut drift = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "completed", None, None),
    ]);
    drift["events"][1]["schema"] = json!("kiana.quality-experiment-event.v2");
    assert_eq!(refusal(&drift), "experiment_event_schema_invalid");
    assert_eq!(
        replay_experiment(&decode(&drift)).expect_err("event schema drift"),
        ExperimentReplayError::EventSchemaInvalid
    );

    let mut unknown_event_field = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
    ]);
    unknown_event_field["events"][0]["unexpected"] = json!(true);
    assert!(ExperimentReplayEvaluator
        .evaluate(&unknown_event_field)
        .is_err());

    let mut unknown_envelope_field = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "completed", None, None),
    ]);
    unknown_envelope_field["unexpected"] = json!(true);
    assert!(ExperimentReplayEvaluator
        .evaluate(&unknown_envelope_field)
        .is_err());
}

#[test]
fn evaluator_findings_are_deterministic_and_registry_wired() {
    let digest = sha('a');
    let value = envelope(vec![
        event(1, "admitted", None, None),
        event(2, "started", None, None),
        event(3, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(4, "case_completed", Some("case:one"), Some(digest.as_str())),
        event(5, "completed", None, None),
    ]);
    let first = evaluated(&value);
    let second = evaluated(&value);
    assert_eq!(first, second);
    assert_eq!(
        canonical_findings(first.clone()).expect("canonical findings"),
        canonical_findings(second).expect("canonical findings")
    );
    assert_eq!(refusal(&value), "experiment_case_result_repeated");

    let mut registry = EvaluatorRegistry::new();
    registry
        .register(ExperimentReplayEvaluator)
        .expect("register experiment replay evaluator");
    assert_eq!(
        registry.evaluator_ids(),
        vec![EXPERIMENT_REPLAY_EVALUATOR_ID.to_owned()]
    );
    assert_eq!(
        canonical_findings(registry.run(&value).expect("registry run")).expect("registry findings"),
        canonical_findings(evaluated(&value)).expect("direct findings")
    );
}

#[test]
fn typed_constructors_round_trip_and_the_index_keeps_source_sequences() {
    let events = vec![
        ExperimentEvent::new(1, ExperimentEventKind::Admitted),
        ExperimentEvent::new(2, ExperimentEventKind::Started),
        ExperimentEvent::new(3, ExperimentEventKind::CaseCompleted)
            .with_case_result("case:two", sha('b')),
        ExperimentEvent::new(4, ExperimentEventKind::CaseCompleted)
            .with_case_result("case:one", sha('a')),
        ExperimentEvent::new(5, ExperimentEventKind::Completed),
    ];
    let input = ExperimentReplayInput::new("experiment:one", "suite:one", events);
    let experiment = replay_experiment(&input).expect("replay typed input");
    assert_eq!(
        experiment
            .case_result("case:two")
            .expect("case two")
            .source_sequence,
        3
    );
    assert_eq!(
        experiment
            .case_result("case:one")
            .expect("case one")
            .source_sequence,
        4
    );

    let decoded: ExperimentReplayInput =
        serde_json::from_value(input.as_value()).expect("round trip input");
    assert_eq!(decoded, input);
    let decoded_experiment: EvalExperiment =
        serde_json::from_value(experiment.as_value()).expect("round trip experiment");
    assert_eq!(decoded_experiment, experiment);

    // The index is ordered by stable case identity, not by arrival order.
    let cases: Vec<&str> = experiment.case_results.keys().map(String::as_str).collect();
    assert_eq!(cases, vec!["case:one", "case:two"]);
}
