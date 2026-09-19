use kiana_domain::{InteractionId, RunId, StepId, TurnId};
use kiana_runner::{DriverError, DriverIntent, HarnessPhase, RunDriver};

#[test]
fn awaiting_input_binds_one_interaction_and_resumes_one_step() {
    let mut driver = RunDriver::new(RunId::new(), Some(TurnId::new()));
    driver.begin_turn().unwrap();
    driver.begin_step(StepId::new(), 1).unwrap();
    let interaction_id = InteractionId::new();
    driver.await_clarification(interaction_id).unwrap();
    assert_eq!(driver.frame.phase, HarnessPhase::AwaitingInput);
    assert_eq!(driver.frame.pending_interaction_id, Some(interaction_id));
    assert_eq!(
        driver
            .transition(kiana_runner::DriverInput::AnswerClarification { interaction_id })
            .unwrap(),
        vec![DriverIntent::StartModel]
    );
    assert_eq!(driver.frame.phase, HarnessPhase::ModelPending);
    assert_eq!(driver.frame.pending_interaction_id, None);
    assert_eq!(
        driver.answer_clarification(interaction_id),
        Err(DriverError::ClarificationNotPending)
    );
}

#[test]
fn foreign_question_cannot_resume_the_waiting_step() {
    let mut driver = RunDriver::new(RunId::new(), Some(TurnId::new()));
    driver.begin_turn().unwrap();
    driver.begin_step(StepId::new(), 1).unwrap();
    driver.await_clarification(InteractionId::new()).unwrap();
    assert_eq!(
        driver.answer_clarification(InteractionId::new()),
        Err(DriverError::ClarificationNotPending)
    );
    driver.validate().unwrap();
}
