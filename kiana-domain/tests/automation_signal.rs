//! AUT-18 先拒绝优先夹具：pause/resume/consume signal、审批单次消费与 checkpoint 元数据。
//!
//! 本文件只驱动纯函数（`plan_automation_signal`）与进程内 `AutomationSignalLedger`；不写
//! EventStore、不派发 effect、不暂停/恢复任何线程，也不证明审批人身份或 checkpoint 已落盘。
//! 覆盖卡片要求的拒绝面：审批人越权/自批、参数/path/epoch 漂移、重复 signal/approval、
//! UI 自带 context、checkpoint 缺失或漂移；成功面覆盖重启恢复（owner/definition/action digest
//! 匹配才恢复）、signal 顺序与 dedupe 稳定。

use kiana_domain::{
    plan_automation_signal, AutomationApproverRole, AutomationCheckpoint, AutomationSignalApproval,
    AutomationSignalCommit, AutomationSignalFact, AutomationSignalKind, AutomationSignalLedger,
    AutomationSignalPlan, AutomationSignalRequest, AutomationSignalState, AUTOMATION_SIGNAL_SCHEMA,
};

const DEFINITION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const ACTION: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const PATH: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const OTHER_DEFINITION: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const OTHER_ACTION: &str =
    "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const OTHER_PATH: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
const FOREIGN_APPROVAL: &str =
    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 默认 epoch 与 checkpoint 序号；测试按需覆盖。
const EPOCH: u64 = 4;
const CHECKPOINT_SEQUENCE: u64 = 5;

/// 构造一个默认合法（字段可被测试覆盖）的请求。
fn request(
    kind: AutomationSignalKind,
    signal_id: &str,
    sequence: u64,
    epoch: u64,
    checkpoint_sequence: u64,
    approval_digest: Option<&str>,
) -> AutomationSignalRequest {
    AutomationSignalRequest::new(
        "execution-1",
        signal_id,
        sequence,
        kind,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        epoch,
        checkpoint_sequence,
        approval_digest.map(str::to_owned),
        None,
        "operator-signal",
    )
    .expect("request fixture must be self-consistent")
}

fn pause_request() -> AutomationSignalRequest {
    request(
        AutomationSignalKind::Pause,
        "signal-1",
        1,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        None,
    )
}

/// 构造一个默认合法（字段可被测试覆盖）的 checkpoint。
fn checkpoint(epoch: u64, sequence: u64) -> AutomationCheckpoint {
    AutomationCheckpoint::new(
        "execution-1",
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        epoch,
        sequence,
        7,
        11,
    )
    .expect("checkpoint fixture must be self-consistent")
}

/// 重新计算 checkpoint 摘要，以便在字段被篡改后仍自洽（证明“字段漂移被拒绝”而不是“摘要不同”）。
fn resign_checkpoint(mut value: AutomationCheckpoint) -> AutomationCheckpoint {
    value.recompute_digest();
    value
}

/// 构造一条绑定 `pause_signal_id` 的合法恢复审批（字段可被测试覆盖）。
fn approval(pause_signal_id: &str, epoch: u64) -> AutomationSignalApproval {
    AutomationSignalApproval::new(
        "approval-1",
        "execution-1",
        "owner-1",
        "operator-1",
        AutomationApproverRole::Operator,
        AutomationSignalKind::Resume,
        DEFINITION,
        ACTION,
        PATH,
        pause_signal_id,
        epoch,
        1,
        10,
        100,
    )
    .expect("approval fixture must be self-consistent")
}

/// 重新计算审批摘要，以便在字段被篡改后仍自洽。
fn resign_approval(mut value: AutomationSignalApproval) -> AutomationSignalApproval {
    value.recompute_digest();
    value
}

/// 重新计算请求摘要，以便在字段被篡改后仍自洽。
fn resign_request(mut value: AutomationSignalRequest) -> AutomationSignalRequest {
    value.recompute_digest();
    value
}

/// 走一遍“计划 → 单次落库”，返回提交结果；`now_tick` 固定为 20（在审批窗口内）。
fn apply_signal(
    ledger: &mut AutomationSignalLedger,
    request: &AutomationSignalRequest,
    approval: Option<&AutomationSignalApproval>,
    checkpoint: Option<&AutomationCheckpoint>,
) -> Result<AutomationSignalCommit, &'static str> {
    let plan = plan_automation_signal(request, approval, checkpoint, ledger, 20)?;
    ledger.apply(request, plan)
}

/// 记录一次 pause 并返回已提交事实，作为后续恢复/消费链的起点。
fn pause(ledger: &mut AutomationSignalLedger) -> AutomationSignalFact {
    let request = pause_request();
    let checkpoint = checkpoint(EPOCH, CHECKPOINT_SEQUENCE);
    apply_signal(ledger, &request, None, Some(&checkpoint))
        .expect("pause must be recorded")
        .fact
}

/// 构造一条绑定 `approval` 的合法恢复请求。
fn resume_request(
    sequence: u64,
    epoch: u64,
    approval: &AutomationSignalApproval,
) -> AutomationSignalRequest {
    request(
        AutomationSignalKind::Resume,
        &format!("signal-{sequence}"),
        sequence,
        epoch,
        CHECKPOINT_SEQUENCE,
        Some(approval.approval_digest.as_str()),
    )
}

#[test]
fn request_and_fact_shapes_fail_closed() {
    // 未知字段在 serde 边界直接拒绝，不允许不同版本对同一载荷作不同解释。
    let mut value = serde_json::to_value(pause_request()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AutomationSignalRequest>(value).is_err());

    // 请求字段组合自洽但形状非法（零序号、坏摘要、带空白的 ID）必须返回稳定错误码。
    let mut zero_sequence = pause_request();
    zero_sequence.signal_sequence = 0;
    assert_eq!(
        resign_request(zero_sequence).validate(),
        Err("automation_signal_request_invalid")
    );
    let mut bad_digest = pause_request();
    bad_digest.action_digest = "sha256:short".to_owned();
    assert_eq!(
        resign_request(bad_digest).validate(),
        Err("automation_signal_request_invalid")
    );
    let mut spaced_owner = pause_request();
    spaced_owner.owner_id = "owner 1".to_owned();
    assert_eq!(
        resign_request(spaced_owner).validate(),
        Err("automation_signal_request_invalid")
    );

    // `consumed` 标记只能与终态一致；单条事实不能自称“已消费的暂停”。
    let mut inconsistent = AutomationSignalFact::new(
        "execution-1",
        "signal-1",
        1,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        AutomationSignalState::Paused,
        None,
    )
    .unwrap();
    assert_eq!(inconsistent.schema, AUTOMATION_SIGNAL_SCHEMA);
    inconsistent.consumed = true;
    inconsistent.recompute_digest();
    assert_eq!(
        inconsistent.validate(),
        Err("automation_signal_fact_invalid")
    );

    // 事实载荷同样拒绝未知字段：旧版/外来字段不会被解释成同一条事实。
    let mut fact_value = serde_json::to_value(&inconsistent).unwrap();
    fact_value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AutomationSignalFact>(fact_value).is_err());
}

#[test]
fn approver_role_and_self_approval_are_refused() {
    // owner 自批：审批人不得是执行方。
    let mut self_approval = approval("signal-1", EPOCH);
    self_approval.approver_id = "owner-1".to_owned();
    assert_eq!(
        resign_approval(self_approval).validate(),
        Err("automation_signal_approval_self_approval")
    );
    // 执行方角色与只读审计角色都不能签发恢复审批。
    for role in [
        AutomationApproverRole::OwnerAgent,
        AutomationApproverRole::Auditor,
    ] {
        let mut unauthorized = approval("signal-1", EPOCH);
        unauthorized.approver_role = role;
        assert!(!role.may_authorize_signals());
        assert_eq!(
            resign_approval(unauthorized).validate(),
            Err("automation_signal_approver_unauthorized")
        );
    }
    for role in [
        AutomationApproverRole::Operator,
        AutomationApproverRole::Supervisor,
    ] {
        assert!(role.may_authorize_signals());
    }
}

#[test]
fn approval_window_and_kind_drift_fail_closed() {
    let mut closed_window = approval("signal-1", EPOCH);
    closed_window.expires_at_tick = closed_window.issued_tick;
    assert_eq!(
        resign_approval(closed_window).validate(),
        Err("automation_signal_approval_window_invalid")
    );
    let mut pause_kind = approval("signal-1", EPOCH);
    pause_kind.kind = AutomationSignalKind::Pause;
    assert_eq!(
        resign_approval(pause_kind).validate(),
        Err("automation_signal_approval_kind_drift")
    );
}

#[test]
fn ui_context_is_refused_before_any_plan() {
    // 入口自带的 UI/模型上下文摘要只要非空就必须整体拒绝；审批与 checkpoint 不得被并入计划。
    let mut carried = pause_request();
    carried.ui_context_digest = Some(ACTION.to_owned());
    let carried = resign_request(carried);
    assert_eq!(
        carried.validate(),
        Err("automation_signal_ui_context_forbidden")
    );
    let mut ledger = AutomationSignalLedger::new();
    let checkpoint = checkpoint(EPOCH, CHECKPOINT_SEQUENCE);
    assert_eq!(
        apply_signal(&mut ledger, &carried, None, Some(&checkpoint)).unwrap_err(),
        "automation_signal_ui_context_forbidden"
    );
    assert!(ledger.is_empty(), "拒绝路径不得写入任何事实");
}

#[test]
fn pause_requires_a_bound_checkpoint_and_no_approval() {
    let mut ledger = AutomationSignalLedger::new();
    let request = pause_request();
    // 缺少 checkpoint：pause 不允许“无元数据暂停”。
    assert_eq!(
        apply_signal(&mut ledger, &request, None, None).unwrap_err(),
        "automation_signal_checkpoint_required"
    );
    // pause 携带审批：审批只用于恢复，混入 pause 一律拒绝。
    let approval = approval("signal-1", EPOCH);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &request,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_approval_unexpected"
    );
    // checkpoint 与请求逐项绑定：owner/序号/epoch 漂移各有稳定错误码。
    let mut owner_drift = checkpoint(EPOCH, CHECKPOINT_SEQUENCE);
    owner_drift.owner_id = "owner-2".to_owned();
    assert_eq!(
        apply_signal(
            &mut ledger,
            &request,
            None,
            Some(&resign_checkpoint(owner_drift))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_owner_drift"
    );
    assert_eq!(
        apply_signal(
            &mut ledger,
            &request,
            None,
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE + 1))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_sequence_drift"
    );
    assert_eq!(
        apply_signal(
            &mut ledger,
            &request,
            None,
            Some(&checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_authority_epoch_ahead"
    );
    assert!(ledger.is_empty());
    // 正确绑定后 pause 成功，且事实状态明确为 Paused。
    let fact = pause(&mut ledger);
    assert_eq!(fact.state, AutomationSignalState::Paused);
    assert_eq!(fact.approval_digest, None);
    assert_eq!(fact.checkpoint_sequence, CHECKPOINT_SEQUENCE);
}

#[test]
fn owner_definition_action_and_path_drift_are_refused() {
    // 每个漂移都用“篡改字段后重签摘要”的请求，证明拒绝来自字段漂移而不是摘要不同。
    let mut drifted = pause_request();
    drifted.owner_id = "owner-2".to_owned();
    let owner_drift = resign_request(drifted);

    let mut drifted = pause_request();
    drifted.definition_digest = OTHER_DEFINITION.to_owned();
    let definition_drift = resign_request(drifted);

    let mut drifted = pause_request();
    drifted.action_digest = OTHER_ACTION.to_owned();
    let action_drift = resign_request(drifted);

    let mut drifted = pause_request();
    drifted.path_scope_digest = OTHER_PATH.to_owned();
    let path_drift = resign_request(drifted);

    let resolved = approval("signal-1", EPOCH);
    for (cas, code) in [
        (owner_drift, "automation_signal_owner_drift"),
        (
            definition_drift,
            "automation_signal_definition_digest_drift",
        ),
        (action_drift, "automation_signal_action_digest_drift"),
        (path_drift, "automation_signal_path_scope_drift"),
    ] {
        let mut ledger = AutomationSignalLedger::new();
        pause(&mut ledger);
        let mut resume = resume_request(2, EPOCH, &resolved);
        // 用篡改后的身份字段覆盖恢复请求（signal/序号/意图保持合法）。
        resume.owner_id = cas.owner_id.clone();
        resume.definition_digest = cas.definition_digest.clone();
        resume.action_digest = cas.action_digest.clone();
        resume.path_scope_digest = cas.path_scope_digest.clone();
        let resume = resign_request(resume);
        assert_eq!(
            apply_signal(
                &mut ledger,
                &resume,
                Some(&resolved),
                Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
            )
            .unwrap_err(),
            code
        );
        assert_eq!(ledger.len(), 1, "漂移请求不得追加新事实");
        assert_eq!(
            ledger.latest("execution-1").unwrap().state,
            AutomationSignalState::Paused
        );
    }
}

#[test]
fn sequence_order_and_dedupe_are_stable() {
    let mut ledger = AutomationSignalLedger::new();
    let paused = pause(&mut ledger);
    let checkpoint = checkpoint(EPOCH, CHECKPOINT_SEQUENCE);
    // 已被占用的序号（回退）必须拒绝。
    let mut regression = pause_request();
    regression.signal_id = "signal-0".to_owned();
    let regression = resign_request(regression);
    assert_eq!(
        apply_signal(&mut ledger, &regression, None, Some(&checkpoint)).unwrap_err(),
        "automation_signal_sequence_regression"
    );
    // 跳号必须拒绝。
    let mut gap = pause_request();
    gap.signal_id = "signal-9".to_owned();
    gap.signal_sequence = 9;
    let gap = resign_request(gap);
    assert_eq!(
        apply_signal(&mut ledger, &gap, None, Some(&checkpoint)).unwrap_err(),
        "automation_signal_sequence_gap"
    );
    // 同一 signal_id 同一内容：幂等重放，返回既有副本且不追加事实。
    let commit = apply_signal(&mut ledger, &pause_request(), None, Some(&checkpoint)).unwrap();
    assert!(commit.replayed);
    assert_eq!(commit.fact.signal_digest, paused.signal_digest);
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.next_sequence("execution-1"), 2);
    // 同一 signal_id 不同内容（决策绑定到新的 checkpoint 序号）：冲突，不允许改写已记录事实。
    let mut conflicting = pause_request();
    conflicting.checkpoint_sequence = CHECKPOINT_SEQUENCE + 1;
    let conflicting = resign_request(conflicting);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &conflicting,
            None,
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE + 1))
        )
        .unwrap_err(),
        "automation_signal_signal_conflict"
    );
    assert_eq!(
        ledger.fetch("signal-1").unwrap().signal_digest,
        paused.signal_digest
    );
}

#[test]
fn authority_epoch_rules_are_enforced() {
    let mut ledger = AutomationSignalLedger::new();
    pause(&mut ledger);
    // pause 不允许 epoch 前进（前进只能出现在带匹配 checkpoint 的重启恢复）。
    let mut ahead = pause_request();
    ahead.signal_id = "signal-2".to_owned();
    ahead.signal_sequence = 2;
    ahead.authority_epoch = EPOCH + 1;
    let ahead = resign_request(ahead);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &ahead,
            None,
            Some(&checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_authority_epoch_ahead"
    );
    // epoch 回退一律拒绝。
    let mut stale = pause_request();
    stale.signal_id = "signal-3".to_owned();
    stale.signal_sequence = 2;
    stale.authority_epoch = EPOCH - 1;
    let stale = resign_request(stale);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &stale,
            None,
            Some(&checkpoint(EPOCH - 1, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_authority_epoch_stale"
    );
    assert_eq!(ledger.len(), 1);
}

#[test]
fn restart_resume_requires_matching_identity_and_checkpoint() {
    // 第一次执行：pause 落库。
    let mut ledger = AutomationSignalLedger::new();
    let paused = pause(&mut ledger);
    // “重启”：用已提交事实重建账本（record_recovered 是唯一装填入口）。
    let mut restarted = AutomationSignalLedger::new();
    let recovered = restarted.record_recovered(paused.clone()).unwrap();
    assert_eq!(recovered.signal_digest, paused.signal_digest);
    assert_eq!(restarted.next_sequence("execution-1"), 2);

    // 重启后 epoch 前推的恢复必须携带身份完全匹配的 checkpoint，否则逐项拒绝。
    let approval = approval("signal-1", EPOCH + 1);
    let resume = resume_request(2, EPOCH + 1, &approval);
    let mut owner_drift = checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE);
    owner_drift.owner_id = "owner-2".to_owned();
    assert_eq!(
        apply_signal(
            &mut restarted,
            &resume,
            Some(&approval),
            Some(&resign_checkpoint(owner_drift))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_owner_drift"
    );
    let mut definition_drift = checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE);
    definition_drift.definition_digest = OTHER_DEFINITION.to_owned();
    assert_eq!(
        apply_signal(
            &mut restarted,
            &resume,
            Some(&approval),
            Some(&resign_checkpoint(definition_drift))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_definition_digest_drift"
    );
    let mut action_drift = checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE);
    action_drift.action_digest = OTHER_ACTION.to_owned();
    assert_eq!(
        apply_signal(
            &mut restarted,
            &resume,
            Some(&approval),
            Some(&resign_checkpoint(action_drift))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_action_digest_drift"
    );
    let mut path_drift = checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE);
    path_drift.path_scope_digest = OTHER_PATH.to_owned();
    assert_eq!(
        apply_signal(
            &mut restarted,
            &resume,
            Some(&approval),
            Some(&resign_checkpoint(path_drift))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_path_scope_drift"
    );
    assert_eq!(restarted.len(), 1, "拒绝路径不得写入新事实");

    // 身份与 checkpoint 全部匹配：允许恢复，且事实 epoch 前进、绑定同一审批摘要。
    let committed = apply_signal(
        &mut restarted,
        &resume,
        Some(&approval),
        Some(&checkpoint(EPOCH + 1, CHECKPOINT_SEQUENCE)),
    )
    .unwrap();
    assert_eq!(committed.fact.state, AutomationSignalState::Resumed);
    assert_eq!(committed.fact.authority_epoch, EPOCH + 1);
    assert_eq!(
        committed.fact.approval_digest.as_deref(),
        Some(approval.approval_digest.as_str())
    );
    assert_eq!(restarted.len(), 2);
}

#[test]
fn approval_must_bind_the_pause_epoch_and_digest() {
    let mut ledger = AutomationSignalLedger::new();
    pause(&mut ledger);
    // 审批绑定另一个 pause signal：拒绝。
    let foreign = approval("signal-other", EPOCH);
    let resume = resume_request(2, EPOCH, &foreign);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &resume,
            Some(&foreign),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_approval_pause_drift"
    );
    // 审批 epoch 与请求 epoch 不一致（旧 epoch 审批恢复新 epoch）：拒绝。
    let stale = approval("signal-1", EPOCH - 1);
    let resume = resume_request(2, EPOCH, &stale);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &resume,
            Some(&stale),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_approval_authority_epoch_stale"
    );
    // 请求声明的审批摘要与审批本体不一致：拒绝。
    let approval = approval("signal-1", EPOCH);
    let mut drifted = resume_request(2, EPOCH, &approval);
    drifted.approval_digest = Some(FOREIGN_APPROVAL.to_owned());
    let drifted = resign_request(drifted);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &drifted,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_approval_digest_drift"
    );
    // 请求不带审批却想恢复：请求形状本身就是错的。
    assert_eq!(
        AutomationSignalRequest::new(
            "execution-1",
            "signal-2",
            2,
            AutomationSignalKind::Resume,
            "owner-1",
            DEFINITION,
            ACTION,
            PATH,
            EPOCH,
            CHECKPOINT_SEQUENCE,
            None,
            None,
            "operator-resume",
        ),
        Err("automation_signal_request_approval_mismatch")
    );
    assert_eq!(ledger.len(), 1);
}

#[test]
fn approval_expiry_is_strict_at_the_window_boundary() {
    let mut ledger = AutomationSignalLedger::new();
    pause(&mut ledger);
    // 边界：now_tick == expires_at_tick 已过期；now_tick < issued_tick 未生效；两者都不得恢复。
    for now_tick in [100_u64, 9] {
        let approval = approval("signal-1", EPOCH);
        let resume = resume_request(2, EPOCH, &approval);
        let plan = plan_automation_signal(
            &resume,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE)),
            &ledger,
            now_tick,
        );
        assert_eq!(plan.unwrap_err(), "automation_signal_approval_expired");
    }
    // 窗口内（20）可以恢复，证明确实是窗口边界而不是别的拒绝。
    let approval = approval("signal-1", EPOCH);
    let resume = resume_request(2, EPOCH, &approval);
    assert!(apply_signal(
        &mut ledger,
        &resume,
        Some(&approval),
        Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
    )
    .is_ok());
}

#[test]
fn checkpoint_sequence_cannot_regress() {
    let mut ledger = AutomationSignalLedger::new();
    pause(&mut ledger);
    let approval = approval("signal-1", EPOCH);
    let mut resume = resume_request(2, EPOCH, &approval);
    resume.checkpoint_sequence = CHECKPOINT_SEQUENCE - 1;
    let resume = resign_request(resume);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &resume,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE - 1))
        )
        .unwrap_err(),
        "automation_signal_checkpoint_sequence_regression"
    );
}

#[test]
fn resumed_signal_is_consumed_exactly_once() {
    let mut ledger = AutomationSignalLedger::new();
    pause(&mut ledger);
    let approval = approval("signal-1", EPOCH);
    let resume = resume_request(2, EPOCH, &approval);
    let resumed = apply_signal(
        &mut ledger,
        &resume,
        Some(&approval),
        Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE)),
    )
    .unwrap()
    .fact;
    assert_eq!(resumed.state, AutomationSignalState::Resumed);

    // 同一审批同一 pause 的第二次恢复：状态已不是 Paused，拒绝。
    let replay = resume_request(3, EPOCH, &approval);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &replay,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_not_paused"
    );

    // consume 必须引用恢复时的同一审批摘要，且只能发生一次。
    // 序号必须紧接已记录事实（pause 1、resume 2），被拒绝的 replay 不推进 next_sequence。
    let consume = request(
        AutomationSignalKind::Consume,
        "signal-4",
        3,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(approval.approval_digest.as_str()),
    );
    let consumed = apply_signal(&mut ledger, &consume, None, None)
        .expect("consume must be recorded once")
        .fact;
    assert_eq!(consumed.state, AutomationSignalState::Consumed);
    assert!(consumed.consumed);

    // 第二次消费（新 signal_id）必须拒绝：Consumed 是终态。
    let second = request(
        AutomationSignalKind::Consume,
        "signal-5",
        4,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(approval.approval_digest.as_str()),
    );
    assert_eq!(
        apply_signal(&mut ledger, &second, None, None).unwrap_err(),
        "automation_signal_already_consumed"
    );
    // 消费后再次 pause / resume 也必须拒绝，终态不可复活。
    let mut after_pause = pause_request();
    after_pause.signal_id = "signal-6".to_owned();
    after_pause.signal_sequence = 4;
    let after_pause = resign_request(after_pause);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &after_pause,
            None,
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_already_consumed"
    );
    let after_resume = request(
        AutomationSignalKind::Resume,
        "signal-7",
        4,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(approval.approval_digest.as_str()),
    );
    assert_eq!(
        apply_signal(
            &mut ledger,
            &after_resume,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_already_consumed"
    );
    assert_eq!(ledger.len(), 3);
}

#[test]
fn resume_and_consume_require_a_paused_or_resumed_predecessor() {
    // 没有任何已记录事实：先恢复（序号 1）→ 拒绝。
    let mut ledger = AutomationSignalLedger::new();
    let approval = approval("signal-1", EPOCH);
    let resume = request(
        AutomationSignalKind::Resume,
        "signal-1",
        1,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(approval.approval_digest.as_str()),
    );
    assert_eq!(
        apply_signal(
            &mut ledger,
            &resume,
            Some(&approval),
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_not_paused"
    );
    // 已暂停但未恢复：先消费 → 拒绝。
    pause(&mut ledger);
    let consume = request(
        AutomationSignalKind::Consume,
        "signal-2",
        2,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(FOREIGN_APPROVAL),
    );
    assert_eq!(
        apply_signal(&mut ledger, &consume, None, None).unwrap_err(),
        "automation_signal_not_resumed"
    );
    // 暂停中的第二次 pause：状态冲突（序号仍须紧接已记录事实：pause 1 之后 next=2）。
    let mut again = pause_request();
    again.signal_id = "signal-3".to_owned();
    again.signal_sequence = 2;
    let again = resign_request(again);
    assert_eq!(
        apply_signal(
            &mut ledger,
            &again,
            None,
            Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE))
        )
        .unwrap_err(),
        "automation_signal_state_conflict"
    );
}

#[test]
fn ledger_recovery_fails_closed_on_conflicting_history() {
    // 第一条事实必须是 Paused：直接以 Resumed 装填 → 拒绝。
    let resumed_at_one = AutomationSignalFact::new(
        "execution-1",
        "signal-1",
        1,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        AutomationSignalState::Resumed,
        Some(ACTION.to_owned()),
    )
    .unwrap();
    let mut empty = AutomationSignalLedger::new();
    assert_eq!(
        empty.record_recovered(resumed_at_one).unwrap_err(),
        "automation_signal_transition_refused"
    );
    assert!(empty.is_empty());

    // 正常装填 pause 后：跳号的恢复事实、同 signal_id 不同内容、篡改摘要都各自拒绝。
    let mut ledger = AutomationSignalLedger::new();
    let paused = pause(&mut ledger);
    let resumed_gap = AutomationSignalFact::new(
        "execution-1",
        "signal-3",
        3,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        AutomationSignalState::Resumed,
        Some(ACTION.to_owned()),
    )
    .unwrap();
    assert_eq!(
        ledger.record_recovered(resumed_gap).unwrap_err(),
        "automation_signal_sequence_gap"
    );
    let mut conflicting = paused.clone();
    conflicting.action_digest = OTHER_ACTION.to_owned();
    conflicting.recompute_digest();
    assert_eq!(
        ledger.record_recovered(conflicting).unwrap_err(),
        "automation_signal_recovery_conflict"
    );
    let mut tampered = paused.clone();
    tampered.checkpoint_sequence = CHECKPOINT_SEQUENCE + 1;
    assert_eq!(
        ledger.record_recovered(tampered).unwrap_err(),
        "automation_signal_fact_invalid"
    );
    // 同一事实重复重放是幂等的，顺序与数量都不变。
    assert_eq!(ledger.record_recovered(paused.clone()).unwrap(), paused);
    assert_eq!(ledger.len(), 1);
    let order: Vec<&str> = ledger.facts().map(|fact| fact.signal_id.as_str()).collect();
    assert_eq!(order, vec!["signal-1"]);
}

#[test]
fn consumed_fact_must_reuse_the_resume_approval_digest() {
    let mut ledger = AutomationSignalLedger::new();
    pause(&mut ledger);
    let approval = approval("signal-1", EPOCH);
    let resume = resume_request(2, EPOCH, &approval);
    apply_signal(
        &mut ledger,
        &resume,
        Some(&approval),
        Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE)),
    )
    .unwrap();
    assert!(ledger.approval_consumed(&approval.approval_digest));

    // 第二次暂停（Resumed → Paused 合法），随后伪造一条复用已消费审批 A 的新恢复事实：
    // 状态转移本身合法，但“一条审批只能支撑一次恢复”必须在 ledger 边界被拒绝。
    let mut second_pause = pause_request();
    second_pause.signal_id = "signal-3".to_owned();
    second_pause.signal_sequence = 3;
    let second_pause = resign_request(second_pause);
    apply_signal(
        &mut ledger,
        &second_pause,
        None,
        Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE)),
    )
    .unwrap();

    let forged = AutomationSignalFact::new(
        "execution-1",
        "signal-4",
        4,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        AutomationSignalState::Resumed,
        Some(approval.approval_digest.clone()),
    )
    .unwrap();
    let forged_request = request(
        AutomationSignalKind::Resume,
        "signal-4",
        4,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(approval.approval_digest.as_str()),
    );
    let plan = AutomationSignalPlan {
        fact: forged,
        replayed: false,
        dispatch_permitted: false,
    };
    assert_eq!(
        ledger.apply(&forged_request, plan).unwrap_err(),
        "automation_signal_approval_consumed"
    );
    assert_eq!(ledger.len(), 3);

    // 同一 signal_id 的新事实（不同摘要）仍是冲突，不能被当成重放。
    let mut forged_plan = AutomationSignalPlan {
        fact: ledger.fetch("signal-2").unwrap().clone(),
        replayed: false,
        dispatch_permitted: false,
    };
    forged_plan.fact.approval_digest = Some(FOREIGN_APPROVAL.to_owned());
    forged_plan.fact.recompute_digest();
    assert_eq!(
        ledger.apply(&resume, forged_plan).unwrap_err(),
        "automation_signal_signal_conflict"
    );
    // 恢复正常重放仍然是幂等的。
    let commit = apply_signal(
        &mut ledger,
        &resume,
        Some(&approval),
        Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE)),
    )
    .unwrap();
    assert!(commit.replayed);
    assert_eq!(commit.fact.state, AutomationSignalState::Resumed);

    // 新审批恢复第二段暂停后，consume 引用新审批摘要：终态引用同一摘要被允许且只发生一次。
    let second_approval = AutomationSignalApproval::new(
        "approval-2",
        "execution-1",
        "owner-1",
        "operator-2",
        AutomationApproverRole::Supervisor,
        AutomationSignalKind::Resume,
        DEFINITION,
        ACTION,
        PATH,
        "signal-3",
        EPOCH,
        1,
        10,
        100,
    )
    .unwrap();
    let second_resume = resume_request(4, EPOCH, &second_approval);
    apply_signal(
        &mut ledger,
        &second_resume,
        Some(&second_approval),
        Some(&checkpoint(EPOCH, CHECKPOINT_SEQUENCE)),
    )
    .unwrap();
    let consume = request(
        AutomationSignalKind::Consume,
        "signal-5",
        5,
        EPOCH,
        CHECKPOINT_SEQUENCE,
        Some(second_approval.approval_digest.as_str()),
    );
    let consumed = apply_signal(&mut ledger, &consume, None, None).unwrap();
    assert_eq!(consumed.fact.state, AutomationSignalState::Consumed);
    assert_eq!(
        consumed.fact.approval_digest.as_deref(),
        Some(second_approval.approval_digest.as_str())
    );
    assert_eq!(ledger.len(), 5);
}

#[test]
fn plan_never_permits_dispatch() {
    let ledger = AutomationSignalLedger::new();
    let checkpoint = checkpoint(EPOCH, CHECKPOINT_SEQUENCE);
    let plan =
        plan_automation_signal(&pause_request(), None, Some(&checkpoint), &ledger, 20).unwrap();
    assert!(!plan.dispatch_permitted);
    assert!(plan.validate().is_ok());
    // 任何声称允许派发的计划都必须被拒绝，防止 signal 决策变成第二条执行入口。
    let forged = AutomationSignalPlan {
        fact: plan.fact,
        replayed: false,
        dispatch_permitted: true,
    };
    assert_eq!(
        forged.validate(),
        Err("automation_signal_dispatch_forbidden")
    );
    assert!(ledger.is_empty(), "plan_automation_signal 不得写账本");
}
