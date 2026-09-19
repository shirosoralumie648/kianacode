use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

fn contract() -> OutputContract {
    OutputContract::new(
        "builder-result",
        1,
        vec!["summary".to_owned(), "verification".to_owned()],
        BTreeMap::from([
            ("summary".to_owned(), OutputValueType::String),
            ("verification".to_owned(), OutputValueType::Array),
        ]),
        false,
    )
    .unwrap()
}

fn input(output: serde_json::Value) -> TurnOutcomeInput {
    TurnOutcomeInput {
        run_id: RunId::new(),
        turn_id: Some(TurnId::new()),
        step_id: Some(StepId::new()),
        output,
        output_contract: Some(contract()),
        answered: false,
        pending_invocations: 0,
        pending_background_jobs: 0,
        pending_steering: false,
        pending_clarification: false,
        pending_approval: false,
        cancelled: false,
        effect_unknown: false,
        failure: None,
        artifact_refs: vec!["artifact:result".to_owned()],
        verification_refs: vec!["event:verified".to_owned()],
    }
}

#[test]
fn valid_contract_is_the_only_path_to_completed() {
    let outcome = TurnOutcome::decide(input(json!({
        "summary": "done",
        "verification": [],
    })))
    .unwrap();
    assert_eq!(outcome.kind, TurnOutcomeKind::Completed);
    assert!(outcome.terminal);
    assert!(!outcome.is_resumable());
    outcome.validate().unwrap();
}

#[test]
fn invalid_final_schema_never_reports_completed() {
    let outcome = TurnOutcome::decide(input(json!({"summary": 42}))).unwrap();
    assert_eq!(outcome.kind, TurnOutcomeKind::Failed);
    assert_eq!(outcome.status, "failed");
    assert_ne!(outcome.kind, TurnOutcomeKind::Completed);
    assert!(outcome
        .reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("output_contract_invalid:")));
}

#[test]
fn pending_work_and_waiting_state_take_precedence_over_text_done() {
    let mut clarification = input(json!({
        "summary": "done",
        "verification": [],
    }));
    clarification.pending_clarification = true;
    let outcome = TurnOutcome::decide(clarification).unwrap();
    assert_eq!(outcome.kind, TurnOutcomeKind::AwaitingInput);
    assert!(outcome.is_resumable());

    let mut approval = input(json!({
        "summary": "done",
        "verification": [],
    }));
    approval.pending_approval = true;
    assert_eq!(
        TurnOutcome::decide(approval).unwrap().kind,
        TurnOutcomeKind::AwaitingApproval
    );

    let mut tools = input(json!({
        "summary": "done",
        "verification": [],
    }));
    tools.pending_invocations = 1;
    assert_eq!(
        TurnOutcome::decide(tools).unwrap().kind,
        TurnOutcomeKind::Blocked
    );
}

#[test]
fn unknown_effect_and_cancel_are_never_success() {
    let mut unknown = input(json!({
        "summary": "done",
        "verification": [],
    }));
    unknown.effect_unknown = true;
    assert_eq!(
        TurnOutcome::decide(unknown).unwrap().kind,
        TurnOutcomeKind::ResultUnknown
    );

    let mut cancelled = input(json!({
        "summary": "done",
        "verification": [],
    }));
    cancelled.cancelled = true;
    assert_eq!(
        TurnOutcome::decide(cancelled).unwrap().kind,
        TurnOutcomeKind::Cancelled
    );
}

#[test]
fn outcome_digest_and_contract_digest_are_stable() {
    let outcome = TurnOutcome::decide(input(json!({
        "summary": "done",
        "verification": [],
    })))
    .unwrap();
    let encoded = serde_json::to_value(&outcome).unwrap();
    let decoded: TurnOutcome = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, outcome);
    assert!(decoded.contract_digest.is_some());
    assert_eq!(decoded.outcome_digest, decoded.digest());
}
