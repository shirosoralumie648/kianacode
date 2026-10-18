//! AUT-17 先拒绝优先夹具：取消代次、stop report、迟到结果 fence 与 result_unknown 的完整状态矩阵。
//!
//! 夹具只构造内存值对象并调用纯转移函数；不写事件、不请求 stop、不接触 worker/provider。

use kiana_domain::{
    automation_cancel_transition_allowed, automation_dispatch_result,
    begin_automation_cancellation, claim_automation_dispatch_command, fence_automation_late_result,
    fold_automation_stop_report, observe_automation_dispatch_command,
    prepare_automation_dispatch_command, AutomationCancelRequest, AutomationCancelState,
    AutomationCancellationFact, AutomationDispatchClaim, AutomationDispatchCommand,
    AutomationDispatchIntent, AutomationDispatchState, AutomationLateResult, AutomationStopReport,
    AutomationStopState, AutomationWorkerEffectOutcome, AUTOMATION_CANCEL_SCHEMA,
    AUTOMATION_DISPATCH_SCHEMA,
};
use serde_json::json;

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fact(state: AutomationCancelState) -> AutomationCancellationFact {
    let mut value = AutomationCancellationFact {
        schema: AUTOMATION_CANCEL_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        attempt: 1,
        cancel_generation: 2,
        authority_epoch: 4,
        state,
        effect_started: true,
        stop_requested: true,
        stop_confirmed: true,
        stop_evidence_digest: Some(A.to_owned()),
        late_result: false,
        reason: "cancelled".to_owned(),
        fact_digest: String::new(),
    };
    // 先给出“已确认停止”的合法基线，再按状态收敛字段组合。
    match state {
        AutomationCancelState::Requested => {
            value.stop_requested = false;
            value.stop_confirmed = false;
            value.stop_evidence_digest = None;
        }
        AutomationCancelState::Stopping => value.stop_confirmed = false,
        AutomationCancelState::NotExecuted => {
            value.effect_started = false;
            value.stop_confirmed = false;
        }
        AutomationCancelState::Stopped => {}
        AutomationCancelState::ResultUnknown => value.stop_confirmed = false,
        AutomationCancelState::Fenced => value.late_result = true,
    }
    value.recompute_digest();
    value
}

fn prepared_intent() -> AutomationDispatchIntent {
    let mut intent = AutomationDispatchIntent {
        schema: AUTOMATION_DISPATCH_SCHEMA.to_owned(),
        execution_id: "execution-1".to_owned(),
        reservation_id: "reservation-1".to_owned(),
        action_digest: A.to_owned(),
        worker_id: None,
        attempt: 1,
        authority_epoch: 4,
        state: AutomationDispatchState::Prepared,
        observation_digest: None,
        intent_digest: String::new(),
    };
    intent.intent_digest = kiana_domain::json_digest(&json!({
        "schema": intent.schema,
        "execution_id": intent.execution_id,
        "reservation_id": intent.reservation_id,
        "action_digest": intent.action_digest,
        "worker_id": intent.worker_id,
        "attempt": intent.attempt,
        "authority_epoch": intent.authority_epoch,
        "state": intent.state,
        "observation_digest": intent.observation_digest,
    }));
    intent
}

fn prepared_command() -> AutomationDispatchCommand {
    prepare_automation_dispatch_command("command-1", "idem-1", &prepared_intent())
        .expect("prepared")
}

fn claimed_command() -> AutomationDispatchCommand {
    match claim_automation_dispatch_command(&prepared_command(), "worker-a", 1, 1).expect("claim") {
        AutomationDispatchClaim::Claimed(command) => command,
        other => panic!("expected a claimed command, got {other:?}"),
    }
}

/// 观察结果（成功或未知）折叠后的终态命令。
fn observed_command(unknown: bool) -> AutomationDispatchCommand {
    let claimed = claimed_command();
    let outcome = if unknown {
        AutomationWorkerEffectOutcome::Unknown {
            result_digest: B.to_owned(),
        }
    } else {
        AutomationWorkerEffectOutcome::Succeeded {
            result_digest: B.to_owned(),
        }
    };
    let result = automation_dispatch_result(&claimed, outcome).expect("result");
    let (command, replayed) =
        observe_automation_dispatch_command(&claimed, &result, claimed.revision).expect("observe");
    assert!(!replayed);
    command
}

fn request(generation: u64) -> AutomationCancelRequest {
    AutomationCancelRequest::new("execution-1", 1, generation, 4, "cancelled").expect("request")
}

#[test]
fn stopped_and_unknown_are_visible_and_late_requires_fence() {
    assert!(fact(AutomationCancelState::Stopped).validate().is_ok());
    let unknown = fact(AutomationCancelState::ResultUnknown);
    assert!(unknown.validate().is_ok());
    let mut late = fact(AutomationCancelState::Stopped);
    late.late_result = true;
    late.recompute_digest();
    assert_eq!(late.validate(), Err("automation_cancellation_fact_invalid"));
}

#[test]
fn unknown_fields_fail_closed() {
    let mut value = serde_json::to_value(fact(AutomationCancelState::Requested)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationCancellationFact>(value).is_err());
}

#[test]
fn queued_cancel_is_not_executed_and_never_requests_a_stop() {
    let command = prepared_command();
    let plan = begin_automation_cancellation(&command, &request(1), None).expect("plan");
    assert!(!plan.replayed);
    assert!(!plan.stop_required);
    assert_eq!(plan.fact.state, AutomationCancelState::NotExecuted);
    assert!(!plan.fact.effect_started);
    assert!(!plan.fact.stop_requested);
    assert!(!plan.fact.stop_confirmed);
    assert!(plan.fact.stop_evidence_digest.is_none());
    assert!(!plan.fact.recovery_required());
    assert_eq!(plan.fact.cancel_generation, 1);
    assert_eq!(plan.fact.authority_epoch, 4);
    assert!(plan.fact.validate().is_ok());
}

#[test]
fn claimed_cancel_requests_a_stop_before_any_confirmation() {
    let command = claimed_command();
    let plan = begin_automation_cancellation(&command, &request(1), None).expect("plan");
    assert!(plan.stop_required);
    assert!(!plan.replayed);
    assert_eq!(plan.fact.state, AutomationCancelState::Requested);
    assert!(plan.fact.effect_started);
    assert!(!plan.fact.stop_requested);
    assert!(!plan.fact.stop_confirmed);
    assert!(plan.fact.stop_evidence_digest.is_none());
    assert!(plan.fact.validate().is_ok());

    // 同一个尚未确认停止的事实不能被改写成“已取消”。
    let mut premature = plan.fact.clone();
    premature.state = AutomationCancelState::Stopped;
    premature.recompute_digest();
    assert_eq!(
        premature.validate(),
        Err("automation_cancellation_fact_invalid")
    );
}

#[test]
fn unknown_execution_enters_recovery_and_finished_execution_cannot_be_cancelled() {
    let unknown = begin_automation_cancellation(&observed_command(true), &request(1), None)
        .expect("unknown plan");
    assert!(!unknown.stop_required);
    assert_eq!(unknown.fact.state, AutomationCancelState::ResultUnknown);
    assert!(unknown.fact.effect_started);
    assert!(unknown.fact.recovery_required());

    assert_eq!(
        begin_automation_cancellation(&observed_command(false), &request(1), None).unwrap_err(),
        "automation_cancel_execution_terminal"
    );
}

#[test]
fn generation_and_epoch_fences_refuse_stale_cancels() {
    let command = claimed_command();
    let mut current = fact(AutomationCancelState::Stopped);
    current.execution_id = command.intent.execution_id.clone();
    current.cancel_generation = 5;
    current.recompute_digest();

    // 更低代次：陈旧取消，不推进事实。
    assert_eq!(
        begin_automation_cancellation(&command, &request(4), Some(&current)).unwrap_err(),
        "automation_cancel_generation_stale"
    );
    // 同代次同请求：安全重放，不需要新的 stop。
    let replayed =
        begin_automation_cancellation(&command, &request(5), Some(&current)).expect("replay");
    assert!(replayed.replayed);
    assert!(!replayed.stop_required);
    assert_eq!(replayed.fact.fact_digest, current.fact_digest);
    // 同代次不同内容：冲突。
    let mut conflicting = request(5);
    conflicting.reason = "operator-request".to_owned();
    conflicting.recompute_digest();
    assert_eq!(
        begin_automation_cancellation(&command, &conflicting, Some(&current)).unwrap_err(),
        "automation_cancel_generation_conflict"
    );
    // 代次更高但命令 epoch 不同：陈旧授权。
    let mut stale_epoch = request(6);
    stale_epoch.authority_epoch = 3;
    stale_epoch.recompute_digest();
    assert_eq!(
        begin_automation_cancellation(&command, &stale_epoch, None).unwrap_err(),
        "automation_cancel_authority_epoch_stale"
    );
    // attempt / execution 漂移：取消不能跨 attempt 或跨 execution 生效。
    let mut wrong_attempt = request(6);
    wrong_attempt.attempt = 2;
    wrong_attempt.recompute_digest();
    assert_eq!(
        begin_automation_cancellation(&command, &wrong_attempt, None).unwrap_err(),
        "automation_cancel_attempt_drift"
    );
    let mut wrong_execution = request(6);
    wrong_execution.execution_id = "execution-2".to_owned();
    wrong_execution.recompute_digest();
    assert_eq!(
        begin_automation_cancellation(&command, &wrong_execution, None).unwrap_err(),
        "automation_cancel_execution_drift"
    );
    // 在途事实（Requested）的同代次重放要求重新发出同一个 stop。
    let in_flight = begin_automation_cancellation(&command, &request(7), None).expect("in flight");
    let retry =
        begin_automation_cancellation(&command, &request(7), Some(&in_flight.fact)).expect("retry");
    assert!(retry.replayed);
    assert!(retry.stop_required);
}

#[test]
fn stop_reports_require_evidence_and_matching_effect_state() {
    for (state, effect_started) in [
        (AutomationStopState::NotStarted, false),
        (AutomationStopState::Stopping, true),
        (AutomationStopState::Stopped, true),
        (AutomationStopState::Unknown, false),
        (AutomationStopState::Unknown, true),
    ] {
        let report = AutomationStopReport::new("execution-1", 1, 2, 4, state, effect_started, A)
            .expect("valid report");
        assert!(report.validate().is_ok());
    }
    // state × effect_started 矛盾必须先拒绝。
    assert_eq!(
        AutomationStopReport::new(
            "execution-1",
            1,
            2,
            4,
            AutomationStopState::NotStarted,
            true,
            A
        )
        .unwrap_err(),
        "automation_stop_report_effect_conflict"
    );
    assert_eq!(
        AutomationStopReport::new(
            "execution-1",
            1,
            2,
            4,
            AutomationStopState::Stopped,
            false,
            A
        )
        .unwrap_err(),
        "automation_stop_report_effect_conflict"
    );
    assert_eq!(
        AutomationStopReport::new(
            "execution-1",
            1,
            2,
            4,
            AutomationStopState::Stopping,
            false,
            A
        )
        .unwrap_err(),
        "automation_stop_report_effect_conflict"
    );
    // 形状/摘要/非零字段。
    assert_eq!(
        AutomationStopReport::new(
            "execution-1",
            1,
            2,
            4,
            AutomationStopState::Stopped,
            true,
            "not-a-digest"
        )
        .unwrap_err(),
        "automation_stop_report_invalid"
    );
    assert_eq!(
        AutomationStopReport::new(
            "execution-1",
            0,
            2,
            4,
            AutomationStopState::Stopped,
            true,
            A
        )
        .unwrap_err(),
        "automation_stop_report_invalid"
    );
    let mut tampered = AutomationStopReport::new(
        "execution-1",
        1,
        2,
        4,
        AutomationStopState::Stopped,
        true,
        A,
    )
    .expect("valid report");
    tampered.evidence_digest = B.to_owned();
    assert_eq!(tampered.validate(), Err("automation_stop_report_invalid"));
    let mut value = serde_json::to_value(
        AutomationStopReport::new(
            "execution-1",
            1,
            2,
            4,
            AutomationStopState::Stopped,
            true,
            A,
        )
        .expect("valid report"),
    )
    .unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationStopReport>(value).is_err());
}

#[test]
fn unconfirmed_stop_reports_never_produce_a_confirmed_cancel() {
    let command = claimed_command();
    let cancel = request(1);
    let in_flight = begin_automation_cancellation(&command, &cancel, None)
        .expect("plan")
        .fact;

    let stopping = fold_automation_stop_report(
        &command,
        &cancel,
        &in_flight,
        &AutomationStopReport::new(
            "execution-1",
            1,
            1,
            4,
            AutomationStopState::Stopping,
            true,
            A,
        )
        .expect("report"),
    )
    .expect("fold");
    assert_eq!(stopping.state, AutomationCancelState::Stopping);
    assert!(!stopping.stop_confirmed);
    assert!(stopping.stop_requested);
    assert!(stopping.recovery_required());
    assert_eq!(stopping.stop_evidence_digest.as_deref(), Some(A));

    let unknown = fold_automation_stop_report(
        &command,
        &cancel,
        &in_flight,
        &AutomationStopReport::new(
            "execution-1",
            1,
            1,
            4,
            AutomationStopState::Unknown,
            true,
            B,
        )
        .expect("report"),
    )
    .expect("fold");
    assert_eq!(unknown.state, AutomationCancelState::ResultUnknown);
    assert!(!unknown.stop_confirmed);
    assert!(unknown.recovery_required());

    let not_started = fold_automation_stop_report(
        &command,
        &cancel,
        &in_flight,
        &AutomationStopReport::new(
            "execution-1",
            1,
            1,
            4,
            AutomationStopState::NotStarted,
            false,
            A,
        )
        .expect("report"),
    )
    .expect("fold");
    assert_eq!(not_started.state, AutomationCancelState::NotExecuted);
    assert!(!not_started.effect_started);
    assert!(!not_started.recovery_required());

    let stopped = fold_automation_stop_report(
        &command,
        &cancel,
        &in_flight,
        &AutomationStopReport::new(
            "execution-1",
            1,
            1,
            4,
            AutomationStopState::Stopped,
            true,
            B,
        )
        .expect("report"),
    )
    .expect("fold");
    assert_eq!(stopped.state, AutomationCancelState::Stopped);
    assert!(stopped.stop_confirmed);
    assert_eq!(stopped.stop_evidence_digest.as_deref(), Some(B));
    assert!(!stopped.recovery_required());

    // 旧报告不能推进新代次；已关闭的事实不能再次折叠 stop。
    assert_eq!(
        fold_automation_stop_report(
            &command,
            &cancel,
            &in_flight,
            &AutomationStopReport::new(
                "execution-1",
                1,
                2,
                4,
                AutomationStopState::Stopped,
                true,
                A,
            )
            .expect("report"),
        )
        .unwrap_err(),
        "automation_stop_report_binding_drift"
    );
    assert_eq!(
        fold_automation_stop_report(
            &command,
            &cancel,
            &stopped,
            &AutomationStopReport::new(
                "execution-1",
                1,
                1,
                4,
                AutomationStopState::Stopping,
                true,
                A,
            )
            .expect("report"),
        )
        .unwrap_err(),
        "automation_cancel_stop_not_pending"
    );
}

#[test]
fn late_results_are_fenced_and_cannot_resurrect_any_cancel_state() {
    for state in [
        AutomationCancelState::Requested,
        AutomationCancelState::Stopping,
        AutomationCancelState::NotExecuted,
        AutomationCancelState::Stopped,
        AutomationCancelState::ResultUnknown,
    ] {
        let current = fact(state);
        let late = AutomationLateResult::new("execution-1", 1, 2, 4, B).expect("late");
        let fenced = fence_automation_late_result(&current, &late).expect("fence");
        assert_eq!(fenced.state, AutomationCancelState::Fenced);
        assert!(fenced.late_result);
        assert!(fenced.recovery_required());
        assert_eq!(fenced.stop_confirmed, current.stop_confirmed);
        assert!(fenced.validate().is_ok());
        // 隔离后不能回到任何活着的状态，也不能再次隔离。
        for target in [
            AutomationCancelState::Requested,
            AutomationCancelState::Stopping,
            AutomationCancelState::NotExecuted,
            AutomationCancelState::Stopped,
            AutomationCancelState::ResultUnknown,
        ] {
            assert!(!automation_cancel_transition_allowed(
                AutomationCancelState::Fenced,
                target
            ));
        }
        assert_eq!(
            fence_automation_late_result(&fenced, &late).unwrap_err(),
            "automation_cancel_already_fenced"
        );
    }
    let current = fact(AutomationCancelState::Stopped);
    // 更大的代次/epoch 不是“迟到结果”，必须走正常观察路径。
    let ahead_generation = AutomationLateResult::new("execution-1", 1, 3, 4, B).expect("late");
    assert_eq!(
        fence_automation_late_result(&current, &ahead_generation).unwrap_err(),
        "automation_late_result_generation_ahead"
    );
    let ahead_epoch = AutomationLateResult::new("execution-1", 1, 2, 5, B).expect("late");
    assert_eq!(
        fence_automation_late_result(&current, &ahead_epoch).unwrap_err(),
        "automation_late_result_authority_epoch_ahead"
    );
    let drifted = AutomationLateResult::new("execution-2", 1, 2, 4, B).expect("late");
    assert_eq!(
        fence_automation_late_result(&current, &drifted).unwrap_err(),
        "automation_late_result_binding_drift"
    );
    // 迟到结果本身不能把状态写成别的值：已关闭的状态没有回到“活着”的转移。
    for target in [
        AutomationCancelState::Requested,
        AutomationCancelState::Stopping,
        AutomationCancelState::ResultUnknown,
    ] {
        assert!(!automation_cancel_transition_allowed(
            AutomationCancelState::Stopped,
            target
        ));
        assert!(!automation_cancel_transition_allowed(
            AutomationCancelState::NotExecuted,
            target
        ));
    }
    // 白名单之外的转移在 validate 之外由 store 拒绝：这里保证共享判定的方向一致。
    assert!(automation_cancel_transition_allowed(
        AutomationCancelState::Stopped,
        AutomationCancelState::Fenced
    ));
    assert!(automation_cancel_transition_allowed(
        AutomationCancelState::NotExecuted,
        AutomationCancelState::Fenced
    ));
}

#[test]
fn cancel_state_matrix_is_closed() {
    let contradictions: Vec<(&str, AutomationCancellationFact)> = vec![
        ("requested with a stop", {
            let mut value = fact(AutomationCancelState::Requested);
            value.stop_requested = true;
            value
        }),
        ("stopping without evidence", {
            let mut value = fact(AutomationCancelState::Stopping);
            value.stop_evidence_digest = None;
            value
        }),
        ("not executed with a started effect", {
            let mut value = fact(AutomationCancelState::NotExecuted);
            value.effect_started = true;
            value
        }),
        ("stopped without evidence", {
            let mut value = fact(AutomationCancelState::Stopped);
            value.stop_evidence_digest = None;
            value
        }),
        ("stopped without a request", {
            let mut value = fact(AutomationCancelState::Stopped);
            value.stop_requested = false;
            value
        }),
        ("unknown with a confirmed stop", {
            let mut value = fact(AutomationCancelState::ResultUnknown);
            value.stop_confirmed = true;
            value
        }),
        ("fenced without a late result", {
            let mut value = fact(AutomationCancelState::Fenced);
            value.late_result = false;
            value
        }),
        ("late result on a stopping fact", {
            let mut value = fact(AutomationCancelState::Stopping);
            value.late_result = true;
            value
        }),
    ];
    for (label, mut value) in contradictions {
        value.recompute_digest();
        assert_eq!(
            value.validate(),
            Err("automation_cancellation_fact_invalid"),
            "contradiction accepted: {label}"
        );
    }
    // 未重算摘要的改写也必须拒绝。
    let mut stale_digest = fact(AutomationCancelState::Stopped);
    stale_digest.reason = "rewritten".to_owned();
    assert_eq!(
        stale_digest.validate(),
        Err("automation_cancellation_fact_invalid")
    );
}

#[test]
fn request_and_late_result_shapes_fail_closed() {
    for generation in [0u64] {
        assert_eq!(
            AutomationCancelRequest::new("execution-1", 1, generation, 4, "cancelled").unwrap_err(),
            "automation_cancel_request_invalid"
        );
    }
    assert_eq!(
        AutomationCancelRequest::new("execution-1", 0, 1, 4, "cancelled").unwrap_err(),
        "automation_cancel_request_invalid"
    );
    assert_eq!(
        AutomationCancelRequest::new("execution-1", 1, 1, 0, "cancelled").unwrap_err(),
        "automation_cancel_request_invalid"
    );
    assert_eq!(
        AutomationCancelRequest::new("execution-1", 1, 1, 4, "  ").unwrap_err(),
        "automation_cancel_request_invalid"
    );
    let mut tampered = request(1);
    tampered.reason = "rewritten".to_owned();
    assert_eq!(
        tampered.validate(),
        Err("automation_cancel_request_invalid")
    );
    let mut value = serde_json::to_value(request(1)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationCancelRequest>(value).is_err());

    assert_eq!(
        AutomationLateResult::new("execution-1", 1, 0, 4, A).unwrap_err(),
        "automation_late_result_invalid"
    );
    assert_eq!(
        AutomationLateResult::new("execution-1", 1, 1, 4, "not-a-digest").unwrap_err(),
        "automation_late_result_invalid"
    );
    let mut late = AutomationLateResult::new("execution-1", 1, 1, 4, A).expect("late");
    late.result_digest = B.to_owned();
    assert_eq!(late.validate(), Err("automation_late_result_invalid"));
    let mut value =
        serde_json::to_value(AutomationLateResult::new("execution-1", 1, 1, 4, A).expect("late"))
            .unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AutomationLateResult>(value).is_err());
}
