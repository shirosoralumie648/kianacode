use kiana_domain::*;

fn request() -> ClarificationRequest {
    ClarificationRequest::new(
        InteractionId::new(),
        RunId::new(),
        TurnId::new(),
        Some(StepId::new()),
        "选择部署窗口",
        vec![
            ClarificationOption::new("now", "现在"),
            ClarificationOption::new("later", "稍后"),
        ],
        true,
        100,
        1_000,
        ClarificationCancelPolicy::UserOrRuntime,
        "builder",
        vec!["user".to_owned()],
    )
    .unwrap()
}

fn answer(request: &ClarificationRequest, run_id: RunId, turn_id: TurnId) -> ClarificationAnswer {
    ClarificationAnswer::new(
        RequestId::new(),
        request.interaction_id,
        run_id,
        turn_id,
        request.step_id,
        Some("now".to_owned()),
        "现在",
        "human-1",
        "user",
        ClarificationSource::Tty,
        200,
        request.request_digest.clone(),
    )
    .unwrap()
}

#[test]
fn answer_resumes_the_original_turn_once() {
    let request = request();
    let answer = answer(&request, request.run_id, request.turn_id);
    let (answered, resolution) = request.accept_answer(answer.clone(), 200).unwrap();
    assert_eq!(answered.status, ClarificationStatus::Answered);
    assert_eq!(answered.answer, Some(answer));
    assert_eq!(resolution.run_id, request.run_id);
    assert_eq!(resolution.turn_id, request.turn_id);
    assert!(resolution.resume_original_step);
    assert!(!resolution.waiting_for_input);
    assert_eq!(
        answered.accept_answer(answer, 201).unwrap_err(),
        "clarification_not_pending"
    );
}

#[test]
fn foreign_or_expired_answers_never_resume() {
    let request = request();
    let foreign_run = answer(&request, RunId::new(), request.turn_id);
    assert_eq!(
        request.accept_answer(foreign_run, 200).unwrap_err(),
        "clarification_answer_run_mismatch"
    );

    let foreign_turn = answer(&request, request.run_id, TurnId::new());
    assert_eq!(
        request.accept_answer(foreign_turn, 200).unwrap_err(),
        "clarification_answer_turn_mismatch"
    );

    let expired = request.expire(1_000).unwrap();
    assert_eq!(expired.status, ClarificationStatus::Expired);
    let late = answer(&request, request.run_id, request.turn_id);
    assert_eq!(
        request.accept_answer(late, 1_000).unwrap_err(),
        "clarification_expired"
    );
}

#[test]
fn cancel_policy_and_required_wait_are_explicit() {
    let request = request();
    let wait = request.waiting_view().unwrap();
    wait.validate().unwrap();
    assert_eq!(wait.status, CLARIFICATION_WAITING_STATUS);
    assert!(wait.required);
    assert_eq!(wait.actionable_id, request.interaction_id.to_string());
    assert!(request.cancel("reviewer", 200).is_err());
    assert_eq!(
        request.cancel("runtime", 200).unwrap().status,
        ClarificationStatus::Cancelled
    );
    assert!(request.options.iter().all(|option| option.id != "default"));
}

#[test]
fn ordinary_answer_has_no_approval_or_capability_authority() {
    let request = request();
    let answer = answer(&request, request.run_id, request.turn_id);
    let encoded = serde_json::to_value(answer).unwrap();
    assert!(encoded.get("approval_id").is_none());
    assert!(encoded.get("capability_request").is_none());
    assert!(encoded.get("grant_id").is_none());
}
