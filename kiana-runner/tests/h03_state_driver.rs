use kiana_domain::{RunId, StepId};
use kiana_runner::{
    transition, DriverError, DriverInput, DriverIntent, HarnessPhase, RunDriver, RunFrame,
};

#[test]
fn second_driver_for_same_turn_is_rejected() {
    let mut driver = RunDriver::new(RunId::new(), None);
    driver.claim("driver-a").unwrap();
    assert_eq!(driver.claim("driver-b"), Err(DriverError::SecondOwner));
    assert_eq!(driver.frame.driver_owner.as_deref(), Some("driver-a"));
}

#[test]
fn full_mailbox_does_not_drop_accepted_input() {
    let mut driver = RunDriver::new(RunId::new(), None);
    for _ in 0..driver.frame.mailbox_capacity {
        driver.queue_input().unwrap();
    }
    let accepted = driver.frame.accepted_inputs;
    assert_eq!(driver.queue_input(), Err(DriverError::MailboxFull));
    assert_eq!(driver.frame.accepted_inputs, accepted);
}

#[test]
fn blocked_model_does_not_block_other_run_or_cancel() {
    let mut blocked = RunDriver::new(RunId::new(), None);
    blocked.begin_turn().unwrap();
    blocked.request_cancel().unwrap();
    assert_eq!(blocked.frame.phase, HarnessPhase::Cancelling);

    let mut other = RunDriver::new(RunId::new(), None);
    other.begin_turn().unwrap();
    other.claim("model-driver").unwrap();
    assert_eq!(other.frame.phase, HarnessPhase::Preparing);
    assert_eq!(other.frame.driver_owner.as_deref(), Some("model-driver"));
}

#[test]
fn stream_and_buffered_calls_share_transitions() {
    let frame = RunDriver::new(RunId::new(), None).frame;
    let mut buffered = RunDriver {
        frame: frame.clone(),
    };
    buffered.begin_turn().unwrap();
    buffered.begin_step(StepId::new(), 1).unwrap();

    let first_transition = transition(&frame, DriverInput::BeginTurn).unwrap();
    assert!(first_transition.intents.contains(&DriverIntent::StartModel));
    let second_transition = transition(
        &first_transition.frame,
        DriverInput::BeginStep {
            step_id: buffered.frame.step_id.unwrap(),
            step: 1,
        },
    )
    .unwrap();
    assert_eq!(second_transition.frame, buffered.frame);
}

#[test]
fn state_driver_frames_are_versioned_and_validated_without_io() {
    let driver = RunDriver::new(RunId::new(), None);
    driver.validate().unwrap();
    let encoded = serde_json::to_value(&driver.frame).unwrap();
    let decoded: RunFrame = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.phase, HarnessPhase::Idle);
    assert_eq!(decoded.pending_tools, 0);
}
