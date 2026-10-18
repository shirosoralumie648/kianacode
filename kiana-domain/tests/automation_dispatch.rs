use kiana_domain::*;
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn intent() -> AutomationDispatchIntent {
    let mut value = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: D.to_owned(),
        worker_id: Some("worker-1".to_owned()),
        attempt: 1,
        authority_epoch: 4,
        state: AutomationDispatchState::Claimed,
        observation_digest: None,
        intent_digest: String::new(),
    };
    value.intent_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "execution_id": value.execution_id,
        "reservation_id": value.reservation_id,
        "action_digest": value.action_digest,
        "worker_id": value.worker_id,
        "attempt": value.attempt,
        "authority_epoch": value.authority_epoch,
        "state": value.state,
        "observation_digest": value.observation_digest,
    }));
    value
}

fn observation(outcome: AutomationObservationOutcome) -> AutomationObservation {
    AutomationObservation {
        execution_id: "execution-1".to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: D.to_owned(),
        worker_id: "worker-1".to_owned(),
        authority_epoch: 4,
        outcome,
        result_digest: D.to_owned(),
    }
}

#[test]
fn observation_is_separate_and_unknown_stays_unknown() {
    let success = validate_automation_observation(
        &intent(),
        &observation(AutomationObservationOutcome::Succeeded),
    )
    .unwrap();
    assert_eq!(success.state, AutomationDispatchState::Observed);
    let unknown = validate_automation_observation(
        &intent(),
        &observation(AutomationObservationOutcome::Unknown),
    )
    .unwrap();
    assert_eq!(unknown.state, AutomationDispatchState::Unknown);
}

#[test]
fn worker_binding_and_prepared_claim_fences_fail_closed() {
    let mut wrong = observation(AutomationObservationOutcome::Succeeded);
    wrong.worker_id = "worker-2".to_owned();
    assert_eq!(
        validate_automation_observation(&intent(), &wrong),
        Err("automation_observation_binding_drift")
    );
    let mut prepared = intent();
    prepared.state = AutomationDispatchState::Prepared;
    prepared.worker_id = None;
    prepared.intent_digest = kiana_domain::json_digest(&json!({
        "schema": prepared.schema,
        "execution_id": prepared.execution_id,
        "reservation_id": prepared.reservation_id,
        "action_digest": prepared.action_digest,
        "worker_id": prepared.worker_id,
        "attempt": prepared.attempt,
        "authority_epoch": prepared.authority_epoch,
        "state": prepared.state,
        "observation_digest": prepared.observation_digest,
    }));
    assert_eq!(
        validate_automation_observation(
            &prepared,
            &observation(AutomationObservationOutcome::Succeeded)
        ),
        Err("automation_observation_binding_drift")
    );
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(intent()).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationDispatchIntent>(value).is_err());
}

// ============================================================================
// AUT-15：可重取 dispatch command 的先拒绝优先夹具
// ============================================================================

/// 按 Domain 私有 digest 的同一字段集合重算命令摘要，用于“篡改后必须被拒”的夹具。
fn recompute_intent(value: &mut AutomationDispatchIntent) {
    value.intent_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "execution_id": value.execution_id,
        "reservation_id": value.reservation_id,
        "action_digest": value.action_digest,
        "worker_id": value.worker_id,
        "attempt": value.attempt,
        "authority_epoch": value.authority_epoch,
        "state": value.state,
        "observation_digest": value.observation_digest,
    }));
}

fn recompute_command(value: &mut AutomationDispatchCommand) {
    let (outcome, result_digest, failure_code, failure_detail_digest, failure_retryable) =
        match &value.result {
            Some(result) => (
                Some(result.observation.outcome),
                Some(result.observation.result_digest.as_str()),
                result.failure.as_ref().map(|failure| failure.code.as_str()),
                result
                    .failure
                    .as_ref()
                    .map(|failure| failure.detail_digest.as_str()),
                result.failure.as_ref().map(|failure| failure.retryable),
            ),
            None => (None, None, None, None, None),
        };
    value.command_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "command_id": value.command_id,
        "idempotency_key": value.idempotency_key,
        "revision": value.revision,
        "intent_digest": value.intent.intent_digest,
        "intent_state": value.intent.state,
        "worker_id": value.intent.worker_id,
        "attempt": value.intent.attempt,
        "observation_digest": value.intent.observation_digest,
        "outcome": outcome,
        "result_digest": result_digest,
        "failure_code": failure_code,
        "failure_detail_digest": failure_detail_digest,
        "failure_retryable": failure_retryable,
    }));
}

fn prepared_intent() -> AutomationDispatchIntent {
    let mut value = intent();
    value.state = AutomationDispatchState::Prepared;
    value.worker_id = None;
    value.observation_digest = None;
    recompute_intent(&mut value);
    value
}

fn prepared_command() -> AutomationDispatchCommand {
    prepare_automation_dispatch_command("command-1", "idem-1", &prepared_intent())
        .expect("prepared command")
}

fn claimed_command() -> AutomationDispatchCommand {
    match claim_automation_dispatch_command(&prepared_command(), "worker-1", 1, 1)
        .expect("claim transition")
    {
        AutomationDispatchClaim::Claimed(command) => command,
        other => panic!("expected a fresh claim, got {other:?}"),
    }
}

fn success_result(command: &AutomationDispatchCommand) -> AutomationDispatchResult {
    automation_dispatch_result(
        command,
        AutomationWorkerEffectOutcome::Succeeded {
            result_digest: D.to_owned(),
        },
    )
    .expect("structured success result")
}

#[test]
fn prepared_command_binds_identity_and_refuses_incoherent_payloads() {
    let command = prepared_command();
    assert_eq!(command.state(), AutomationDispatchState::Prepared);
    assert_eq!(command.revision, 1);
    assert!(command.result.is_none());
    assert!(!command.is_terminal());

    // 只有 Prepared intent 能成为命令；已经 claim 的 intent 必须由既有事实迁移，不能重新派生。
    assert_eq!(
        prepare_automation_dispatch_command("command-2", "idem-2", &intent()),
        Err("automation_dispatch_command_intent_not_prepared")
    );

    // 摘要漂移 / 身份漂移都被拒绝。
    let mut tampered = command.clone();
    tampered.command_id = "command-tampered".to_owned();
    assert_eq!(
        tampered.validate(),
        Err("automation_dispatch_command_invalid")
    );

    // Prepared 阶段携带结果、终态缺少结果都是结构错误。
    let mut premature = command.clone();
    premature.result = Some(success_result(&claimed_command()));
    recompute_command(&mut premature);
    assert_eq!(
        premature.validate(),
        Err("automation_dispatch_command_result_unexpected")
    );

    let mut missing = claimed_command();
    missing.intent.state = AutomationDispatchState::Observed;
    missing.intent.observation_digest = Some(D.to_owned());
    missing.intent.intent_digest = String::new();
    recompute_intent(&mut missing.intent);
    recompute_command(&mut missing);
    assert_eq!(
        missing.validate(),
        Err("automation_dispatch_command_result_required")
    );
}

#[test]
fn claim_binds_one_worker_attempt_and_refuses_duplicates() {
    let prepared = prepared_command();
    let claimed = claimed_command();
    assert_eq!(claimed.state(), AutomationDispatchState::Claimed);
    assert_eq!(claimed.revision, 2);
    assert_eq!(claimed.intent.worker_id.as_deref(), Some("worker-1"));
    assert_eq!(claimed.intent.attempt, 1);

    // 同一 worker + attempt 携带过期 revision 重取是安全的：返回既有命令，不产生第二个 attempt。
    match claim_automation_dispatch_command(&claimed, "worker-1", 1, 1).expect("safe re-fetch") {
        AutomationDispatchClaim::AlreadyClaimed(command) => assert_eq!(command, claimed),
        other => panic!("expected safe re-fetch, got {other:?}"),
    }

    // 重复 worker / 第二个 attempt 一律拒绝。
    assert_eq!(
        claim_automation_dispatch_command(&claimed, "worker-2", 1, 2),
        Err("automation_dispatch_worker_conflict")
    );
    assert_eq!(
        claim_automation_dispatch_command(&claimed, "worker-1", 2, 2),
        Err("automation_dispatch_worker_conflict")
    );

    // Prepared 命令的 revision 必须匹配，attempt 必须非零。
    assert_eq!(
        claim_automation_dispatch_command(&prepared, "worker-1", 1, 2),
        Err("automation_dispatch_revision_conflict")
    );
    assert_eq!(
        claim_automation_dispatch_command(&prepared, "worker-1", 0, 1),
        Err("automation_dispatch_command_invalid")
    );

    // 终态命令不能再被 claim。
    let observed = match observe_automation_dispatch_command(&claimed, &success_result(&claimed), 2)
        .expect("observed command")
    {
        (command, false) => command,
        other => panic!("unexpected observation transition: {other:?}"),
    };
    match claim_automation_dispatch_command(&observed, "worker-1", 1, 3)
        .expect("terminal claim decision")
    {
        AutomationDispatchClaim::Terminal(command) => assert_eq!(command, observed),
        other => panic!("expected terminal decision, got {other:?}"),
    }
}

#[test]
fn observation_requires_a_claim_and_structured_failure() {
    let prepared = prepared_command();
    assert_eq!(
        observe_automation_dispatch_command(&prepared, &success_result(&claimed_command()), 1),
        Err("automation_dispatch_observation_not_claimed")
    );

    let claimed = claimed_command();
    let failure = automation_dispatch_result(
        &claimed,
        AutomationWorkerEffectOutcome::Failed {
            failure_code: "connector_timeout".to_owned(),
            detail_digest: D.to_owned(),
            retryable: true,
            result_digest: D.to_owned(),
        },
    )
    .expect("structured failure");
    let structured = failure.failure.expect("known failure must be structured");
    assert_eq!(structured.code, "connector_timeout");
    assert!(structured.retryable);
    assert_eq!(failure.validate(), Ok(()));
    let (observed, replayed) =
        observe_automation_dispatch_command(&claimed, &failure, 2).expect("known failure");
    assert!(!replayed);
    assert_eq!(observed.state(), AutomationDispatchState::Observed);

    // 成功/未知不得夹带失败对象；已知失败必须结构化。
    let mut mismatched = success_result(&claimed);
    mismatched.failure = Some(AutomationDispatchFailure {
        code: "connector_timeout".to_owned(),
        detail_digest: D.to_owned(),
        retryable: false,
    });
    assert_eq!(
        mismatched.validate(),
        Err("automation_dispatch_failure_unexpected")
    );
    let unknown = automation_dispatch_result(
        &claimed,
        AutomationWorkerEffectOutcome::Unknown {
            result_digest: D.to_owned(),
        },
    )
    .expect("unknown result");
    assert!(unknown.failure.is_none());
    let mut unclaimed = unknown.clone();
    unclaimed.observation.outcome = AutomationObservationOutcome::Failed;
    assert_eq!(
        unclaimed.validate(),
        Err("automation_dispatch_failure_required")
    );

    // runner 自报身份的绑定必须与命令一致；伪造 worker 被 binding drift 拒绝。
    let mut forged = success_result(&claimed);
    forged.observation.worker_id = "worker-2".to_owned();
    assert_eq!(
        observe_automation_dispatch_command(&claimed, &forged, 2),
        Err("automation_observation_binding_drift")
    );

    // 非法失败码/摘要形状不能进入结构化结果。
    assert_eq!(
        automation_dispatch_result(
            &claimed,
            AutomationWorkerEffectOutcome::Failed {
                failure_code: "bad code".to_owned(),
                detail_digest: D.to_owned(),
                retryable: false,
                result_digest: D.to_owned(),
            },
        ),
        Err("automation_dispatch_failure_invalid")
    );
    assert_eq!(
        AutomationDispatchFailure::new("connector_timeout", "not-a-digest", false),
        Err("automation_dispatch_failure_invalid")
    );
}

#[test]
fn terminal_results_cannot_be_overwritten_or_resurrected() {
    let claimed = claimed_command();
    let success = success_result(&claimed);
    let (observed, replayed) =
        observe_automation_dispatch_command(&claimed, &success, 2).expect("first observation");
    assert!(!replayed);
    assert_eq!(observed.state(), AutomationDispatchState::Observed);
    assert_eq!(observed.revision, 3);

    // 完全相同的结果携带过期 revision 重放：安全重取，不写第二个结果。
    let (again, replayed) =
        observe_automation_dispatch_command(&observed, &success, 2).expect("identical replay");
    assert!(replayed);
    assert_eq!(again, observed);

    // 不同结果：当前 revision 是观察冲突，陈旧 revision 是版本冲突；终态永远不被覆盖。
    let replacement = automation_dispatch_result(
        &claimed,
        AutomationWorkerEffectOutcome::Failed {
            failure_code: "late_failure".to_owned(),
            detail_digest: D.to_owned(),
            retryable: false,
            result_digest: D.to_owned(),
        },
    )
    .expect("structured failure");
    assert_eq!(
        observe_automation_dispatch_command(&observed, &replacement, 3),
        Err("automation_dispatch_observation_conflict")
    );
    assert_eq!(
        observe_automation_dispatch_command(&observed, &replacement, 2),
        Err("automation_dispatch_revision_conflict")
    );
    assert_eq!(observed.state(), AutomationDispatchState::Observed);
    assert_eq!(observed.result.as_ref(), Some(&success));

    // Unknown 是独立终态：不能事后被“成功”覆盖，也不能在成功终态上追加 Unknown。
    let unknown = automation_dispatch_result(
        &claimed,
        AutomationWorkerEffectOutcome::Unknown {
            result_digest: D.to_owned(),
        },
    )
    .expect("unknown result");
    let (unknown_command, _) =
        observe_automation_dispatch_command(&claimed, &unknown, 2).expect("unknown observation");
    assert_eq!(unknown_command.state(), AutomationDispatchState::Unknown);
    assert_eq!(
        observe_automation_dispatch_command(&unknown_command, &success, 3),
        Err("automation_dispatch_observation_conflict")
    );
    assert_eq!(
        observe_automation_dispatch_command(&observed, &unknown, 3),
        Err("automation_dispatch_observation_conflict")
    );

    // claim 阶段同样拒绝：终态不能被第二次派发“复活”。
    assert_eq!(
        claim_automation_dispatch_command(&unknown_command, "worker-1", 1, 3)
            .map(|claim| matches!(claim, AutomationDispatchClaim::Terminal(_))),
        Ok(true)
    );
}

#[test]
fn command_payloads_fail_closed_on_unknown_fields_and_drift() {
    // 未知字段在命令、结果、失败与请求 DTO 上全部拒绝。
    let mut command_value = serde_json::to_value(prepared_command()).unwrap();
    command_value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationDispatchCommand>(command_value).is_err());

    let claimed = claimed_command();
    let mut result_value = serde_json::to_value(success_result(&claimed)).unwrap();
    result_value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationDispatchResult>(result_value).is_err());

    let mut failure_value = json!({
        "code": "connector_timeout",
        "detail_digest": D,
        "retryable": false,
        "unexpected": true,
    });
    assert!(serde_json::from_value::<AutomationDispatchFailure>(failure_value.clone()).is_err());
    failure_value.as_object_mut().unwrap().remove("unexpected");
    assert!(serde_json::from_value::<AutomationDispatchFailure>(failure_value).is_ok());

    let mut request_value = serde_json::to_value(AutomationDispatchClaimRequest {
        command_id: "command-1".to_owned(),
        worker_id: "worker-1".to_owned(),
        attempt: 1,
        expected_revision: 1,
    })
    .unwrap();
    request_value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationDispatchClaimRequest>(request_value).is_err());

    // 终态命令的结果摘要必须与 intent 记录的观察摘要一致，状态与 outcome 必须对应。
    let success = success_result(&claimed);
    let (observed, _) =
        observe_automation_dispatch_command(&claimed, &success, 2).expect("observed command");
    let mut drifted = observed.clone();
    drifted.result.as_mut().unwrap().observation.result_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    recompute_command(&mut drifted);
    assert_eq!(
        drifted.validate(),
        Err("automation_dispatch_command_result_drift")
    );

    let mut swapped = observed.clone();
    swapped.result.as_mut().unwrap().observation.outcome = AutomationObservationOutcome::Unknown;
    recompute_command(&mut swapped);
    assert_eq!(
        swapped.validate(),
        Err("automation_dispatch_command_state_outcome_drift")
    );

    // 命令摘要覆盖 revision：篡改 revision 而不重算摘要必须被拒。
    let mut bumped = observed.clone();
    bumped.revision = 99;
    assert_eq!(
        bumped.validate(),
        Err("automation_dispatch_command_invalid")
    );

    // 幂等键/命令 ID 的有界文本与 revision 非零同样 fail-closed。
    let mut blank = prepared_command();
    blank.idempotency_key = " ".to_owned();
    recompute_command(&mut blank);
    assert_eq!(blank.validate(), Err("automation_dispatch_command_invalid"));

    let mut zero = prepared_command();
    zero.revision = 0;
    recompute_command(&mut zero);
    assert_eq!(zero.validate(), Err("automation_dispatch_command_invalid"));
}
