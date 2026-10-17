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
fn alternating_failed_calls_hit_no_progress_limit() {
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
    tracker.validate().unwrap();
}

#[test]
fn progressing_job_poll_is_bounded_without_false_loop_failure() {
    // 合法长任务 poll：同一 JobHandle 的 cursor/heartbeat 每次推进就是证据进展，
    // 即使观察次数远超窗口也不能被误判为循环。
    let mut tracker = ProgressTracker::new(4, 3).unwrap();
    for tick in 1..=32u64 {
        let decision = tracker
            .observe(input(evidence("poll", Some(tick), Some(tick * 10))))
            .unwrap();
        assert!(
            decision.progressed,
            "advancing cursor/heartbeat must count as progress"
        );
        assert_eq!(decision.consecutive_stall, 0);
        assert_eq!(decision.action, ProgressAction::Continue);
    }

    // 停滞 poll（cursor/heartbeat 不再变化）也不是无限等待：沿用同一有界阶梯。
    let stalled = evidence("poll", Some(32), Some(320));
    assert_eq!(
        tracker.observe(input(stalled.clone())).unwrap().action,
        ProgressAction::FeedbackOnce
    );
    assert_eq!(
        tracker.observe(input(stalled.clone())).unwrap().action,
        ProgressAction::RequestClarification
    );
    assert_eq!(
        tracker.observe(input(stalled)).unwrap().action,
        ProgressAction::Blocked
    );
    tracker.validate().unwrap();
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
fn stop_hook_cannot_extend_turn_indefinitely() {
    let mut tracker = ProgressTracker::new(8, 3).unwrap();
    // 有显式剩余额度时，stop hook 每轮只能消耗一份预算做一次有界反馈。
    let mut hook = input(evidence("hook", None, None));
    hook.stop_hook_feedback = Some("keep going".to_owned());
    hook.stop_hook_budget_remaining = 2;
    let first = tracker.observe(hook).unwrap();
    assert_eq!(first.action, ProgressAction::FeedbackOnce);
    assert_eq!(first.stop_hook_budget_remaining, 1);

    let mut hook = input(evidence("hook", None, None));
    hook.stop_hook_feedback = Some("keep going".to_owned());
    hook.stop_hook_budget_remaining = 1;
    let second = tracker.observe(hook).unwrap();
    assert_eq!(second.action, ProgressAction::FeedbackOnce);
    assert_eq!(second.stop_hook_budget_remaining, 0);

    // 额度归零后 stop hook 不能再延长 turn：直接 blocked，而不是无限追问。
    let mut exhausted = input(evidence("hook", None, None));
    exhausted.stop_hook_feedback = Some("keep going".to_owned());
    let third = tracker.observe(exhausted).unwrap();
    assert_eq!(third.action, ProgressAction::Blocked);
    assert_eq!(third.reason, "stop_hook_budget_exhausted");
    assert_eq!(third.stop_hook_budget_remaining, 0);
    tracker.validate().unwrap();
}

#[test]
fn empty_turn_after_tool_evidence_is_not_reported_as_progress() {
    let mut tracker = ProgressTracker::new(8, 3).unwrap();
    tracker
        .observe(input(evidence("tool-observation", None, None)))
        .unwrap();

    // 空回答即使证据形状与上一条不同，也不能算作“模型在推进”。
    let mut empty = input(evidence("empty-answer", None, None));
    empty.empty_turn = true;
    let decision = tracker.observe(empty).unwrap();
    assert!(!decision.progressed);
    assert_eq!(decision.consecutive_stall, 1);
    assert_eq!(decision.action, ProgressAction::FeedbackOnce);

    let mut empty = input(evidence("empty-answer", None, None));
    empty.empty_turn = true;
    assert_eq!(
        tracker.observe(empty).unwrap().action,
        ProgressAction::RequestClarification
    );
}

#[test]
fn repeated_failure_window_resets_after_observable_progress() {
    let mut tracker = ProgressTracker::new(8, 3).unwrap();
    // 失败 → 有进展 → 失败 → 有进展 → 同一失败，前两次失败不应累计成无进展重试。
    for round in 0..2 {
        let mut failure = input(evidence(&format!("attempt-{round}"), None, None));
        failure.failure_digest = Some(failure_digest("same-failure").unwrap());
        tracker.observe(failure).unwrap();
        tracker
            .observe(input(evidence(&format!("progress-{round}"), None, None)))
            .unwrap();
    }
    let mut failure = input(evidence("attempt-2", None, None));
    failure.failure_digest = Some(failure_digest("same-failure").unwrap());
    let decision = tracker.observe(failure).unwrap();
    assert_eq!(decision.repeated_failure_count, 1);
    assert!(!decision.action.is_blocked());
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
