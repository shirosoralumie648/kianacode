use kiana_domain::*;
use serde_json::json;

fn evidence(state: &str, cursor: Option<u64>, heartbeat: Option<u64>) -> ProgressEvidence {
    ProgressEvidence::new(
        &json!({"observation": state}),
        None,
        None,
        &[],
        cursor,
        heartbeat,
    )
    .unwrap()
}

fn input(evidence: ProgressEvidence) -> ProgressInput {
    ProgressInput::new(evidence)
}

#[test]
fn alternating_failed_calls_hit_a_b_a_no_progress_limit() {
    let mut tracker = ProgressTracker::new(8, 3).unwrap();
    let mut first = input(evidence("A", None, None));
    first.failure_digest = Some(failure_digest("failed-a").unwrap());
    assert_eq!(
        tracker.observe(first).unwrap().action,
        ProgressAction::Continue
    );

    let mut second = input(evidence("B", None, None));
    second.failure_digest = Some(failure_digest("failed-b").unwrap());
    tracker.observe(second).unwrap();

    let mut third = input(evidence("A", None, None));
    third.failure_digest = Some(failure_digest("failed-a").unwrap());
    let decision = tracker.observe(third).unwrap();
    assert!(decision.cycle_detected);
    assert_eq!(decision.action, ProgressAction::FeedbackOnce);

    let mut fourth = input(evidence("B", None, None));
    fourth.failure_digest = Some(failure_digest("failed-b").unwrap());
    assert_eq!(
        tracker.observe(fourth).unwrap().action,
        ProgressAction::RequestClarification
    );

    let mut fifth = input(evidence("A", None, None));
    fifth.failure_digest = Some(failure_digest("failed-a").unwrap());
    assert_eq!(
        tracker.observe(fifth).unwrap().action,
        ProgressAction::Blocked
    );
}

#[test]
fn progressing_job_poll_is_bounded_without_false_loop_failure() {
    let mut tracker = ProgressTracker::default();
    assert_eq!(
        tracker
            .observe(input(evidence("poll", Some(1), Some(10))))
            .unwrap()
            .action,
        ProgressAction::Continue
    );
    let decision = tracker
        .observe(input(evidence("poll", Some(2), Some(11))))
        .unwrap();
    assert!(decision.progressed);
    assert_eq!(decision.action, ProgressAction::Continue);
    assert_eq!(decision.consecutive_stall, 0);
}

#[test]
fn empty_turn_and_stop_hook_feedback_are_bounded() {
    let mut tracker = ProgressTracker::new(8, 3).unwrap();
    let mut empty = input(evidence("empty", None, None));
    empty.empty_turn = true;
    assert_eq!(
        tracker.observe(empty).unwrap().action,
        ProgressAction::FeedbackOnce
    );
    let mut empty = input(evidence("empty", None, None));
    empty.empty_turn = true;
    assert_eq!(
        tracker.observe(empty).unwrap().action,
        ProgressAction::RequestClarification
    );

    let mut hook_tracker = ProgressTracker::new(8, 3).unwrap();
    let mut hook = input(evidence("hook", None, None));
    hook.stop_hook_feedback = Some("try once".to_owned());
    hook.stop_hook_budget_remaining = 1;
    let first = hook_tracker.observe(hook).unwrap();
    assert_eq!(first.action, ProgressAction::FeedbackOnce);
    assert_eq!(first.stop_hook_budget_remaining, 0);
    let mut exhausted = input(evidence("hook", None, None));
    exhausted.stop_hook_feedback = Some("try again".to_owned());
    assert_eq!(
        hook_tracker.observe(exhausted).unwrap().action,
        ProgressAction::Blocked
    );
}

#[test]
fn progress_state_round_trips_with_digests() {
    let mut tracker = ProgressTracker::default();
    tracker.observe(input(evidence("one", None, None))).unwrap();
    tracker.validate().unwrap();
    let value = serde_json::to_value(&tracker).unwrap();
    let decoded: ProgressTracker = serde_json::from_value(value).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded, tracker);
}
