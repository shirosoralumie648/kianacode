//! AUT-18 先拒绝优先夹具：runner 侧 pause/resume 边界（`RunnerSignalGate::fold`）。
//!
//! 夹具只驱动纯函数：把已提交 signal 事实折叠成 `Continue`/`Hold`/`Resume`/`Closed`。它不调用
//! harness、不发起模型请求、不执行工具、不消费审批，也不写 EventStore。

use kiana_domain::{AutomationSignalFact, AutomationSignalState};
use kiana_runner::{RunnerSignalDirective, RunnerSignalGate, RunnerSignalIdentity};

const DEFINITION: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const ACTION: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const PATH: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const OTHER_DEFINITION: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const OTHER_ACTION: &str =
    "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const OTHER_PATH: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
const APPROVAL: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const FOREIGN_APPROVAL: &str =
    "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

/// 默认身份：epoch 4、checkpoint 序号 5、身份摘要与夹具事实一致。
fn identity() -> RunnerSignalIdentity {
    RunnerSignalIdentity::new("execution-1", "owner-1", DEFINITION, ACTION, PATH, 4, 5)
}

/// 构造一条绑定默认身份的事实；checkpoint 序号固定 5。
fn fact(
    state: AutomationSignalState,
    signal_id: &str,
    sequence: u64,
    epoch: u64,
    approval_digest: Option<&str>,
) -> AutomationSignalFact {
    AutomationSignalFact::new(
        "execution-1",
        signal_id,
        sequence,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        epoch,
        5,
        state,
        approval_digest.map(str::to_owned),
    )
    .expect("runner signal fixture must be self-consistent")
}

fn paused() -> AutomationSignalFact {
    fact(AutomationSignalState::Paused, "signal-1", 1, 4, None)
}

fn resumed(signal_id: &str, sequence: u64, epoch: u64) -> AutomationSignalFact {
    fact(
        AutomationSignalState::Resumed,
        signal_id,
        sequence,
        epoch,
        Some(APPROVAL),
    )
}

#[test]
fn identity_shape_failures_refuse_before_any_fact() {
    let no_owner = RunnerSignalIdentity::new("execution-1", "", DEFINITION, ACTION, PATH, 4, 5);
    let bad_digest =
        RunnerSignalIdentity::new("execution-1", "owner-1", "sha256:short", ACTION, PATH, 4, 5);
    let zero_epoch =
        RunnerSignalIdentity::new("execution-1", "owner-1", DEFINITION, ACTION, PATH, 0, 5);
    let zero_checkpoint =
        RunnerSignalIdentity::new("execution-1", "owner-1", DEFINITION, ACTION, PATH, 4, 0);
    for broken in [&no_owner, &bad_digest, &zero_epoch, &zero_checkpoint] {
        assert_eq!(
            RunnerSignalGate::fold(broken, &[paused()]),
            Err("runner_signal_identity_invalid")
        );
    }
    // 形状合法的身份 + 空历史：Continue，允许模型循环按当前授权继续。
    assert_eq!(
        RunnerSignalGate::fold(&identity(), &[]),
        Ok(RunnerSignalDirective::Continue)
    );
}

#[test]
fn empty_history_continues_and_pause_holds() {
    let identity = identity();
    let directive = RunnerSignalGate::fold(&identity, &[]).unwrap();
    assert_eq!(directive, RunnerSignalDirective::Continue);
    assert!(directive.model_calls_permitted());
    let directive = RunnerSignalGate::fold(&identity, &[paused()]).unwrap();
    assert_eq!(directive, RunnerSignalDirective::Hold);
    assert!(!directive.model_calls_permitted());
    assert!(!directive.is_closed());
}

#[test]
fn resume_and_consume_map_to_resume_and_closed() {
    let identity = identity();
    let facts = [paused(), resumed("signal-2", 2, 4)];
    assert_eq!(
        RunnerSignalGate::fold(&identity, &facts),
        Ok(RunnerSignalDirective::Resume {
            authority_epoch: 4,
            checkpoint_sequence: 5,
        })
    );
    assert!(RunnerSignalGate::fold(&identity, &facts)
        .unwrap()
        .model_calls_permitted());
    let consumed = [
        paused(),
        resumed("signal-2", 2, 4),
        fact(AutomationSignalState::Paused, "signal-3", 3, 4, None),
        fact(
            AutomationSignalState::Resumed,
            "signal-4",
            4,
            4,
            Some(FOREIGN_APPROVAL),
        ),
        fact(
            AutomationSignalState::Consumed,
            "signal-5",
            5,
            4,
            Some(FOREIGN_APPROVAL),
        ),
    ];
    let directive = RunnerSignalGate::fold(&identity, &consumed).unwrap();
    assert_eq!(directive, RunnerSignalDirective::Closed);
    assert!(!directive.model_calls_permitted());
    assert!(directive.is_closed());
}

#[test]
fn restart_epoch_advance_is_honored_only_on_resume_facts() {
    let identity = identity();
    // 重启后 resume 事实把 epoch 从 4 前推到 5：允许，并成为 runner 的新起点。
    let facts = [paused(), resumed("signal-2", 2, 5)];
    assert_eq!(
        RunnerSignalGate::fold(&identity, &facts),
        Ok(RunnerSignalDirective::Resume {
            authority_epoch: 5,
            checkpoint_sequence: 5,
        })
    );
    // 非 Resumed 事实不允许继续加码 epoch。
    let ahead = [
        paused(),
        resumed("signal-2", 2, 5),
        fact(AutomationSignalState::Paused, "signal-3", 3, 6, None),
    ];
    assert_eq!(
        RunnerSignalGate::fold(&identity, &ahead),
        Err("runner_signal_authority_epoch_ahead")
    );
}

#[test]
fn execution_and_identity_drift_are_refused() {
    let identity = identity();
    // 其他 execution 的事实不参与本 run 的判定。
    let other_execution = AutomationSignalFact::new(
        "execution-2",
        "signal-1",
        1,
        "owner-1",
        DEFINITION,
        ACTION,
        PATH,
        4,
        5,
        AutomationSignalState::Paused,
        None,
    )
    .unwrap();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[other_execution]),
        Err("runner_signal_execution_drift")
    );
    // owner/definition/action/path 逐项漂移各自拒绝。
    let mut owner = paused();
    owner.owner_id = "owner-2".to_owned();
    owner.recompute_digest();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[owner]),
        Err("runner_signal_owner_drift")
    );
    let mut definition = paused();
    definition.definition_digest = OTHER_DEFINITION.to_owned();
    definition.recompute_digest();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[definition]),
        Err("runner_signal_definition_drift")
    );
    let mut action = paused();
    action.action_digest = OTHER_ACTION.to_owned();
    action.recompute_digest();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[action]),
        Err("runner_signal_action_drift")
    );
    let mut path = paused();
    path.path_scope_digest = OTHER_PATH.to_owned();
    path.recompute_digest();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[path]),
        Err("runner_signal_path_scope_drift")
    );
}

#[test]
fn sequences_must_start_at_one_and_stay_contiguous() {
    let identity = identity();
    // 第一条事实序号不是 1。
    assert_eq!(
        RunnerSignalGate::fold(
            &identity,
            &[fact(AutomationSignalState::Paused, "signal-2", 2, 4, None)]
        ),
        Err("runner_signal_sequence_invalid")
    );
    // 跳号（1 → 3）。
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[paused(), resumed("signal-3", 3, 4)]),
        Err("runner_signal_sequence_invalid")
    );
    // 回退（1 → 2 → 1）。
    assert_eq!(
        RunnerSignalGate::fold(
            &identity,
            &[
                paused(),
                resumed("signal-2", 2, 4),
                fact(AutomationSignalState::Paused, "signal-1b", 1, 4, None),
            ]
        ),
        Err("runner_signal_sequence_invalid")
    );
}

#[test]
fn duplicate_facts_dedupe_stably_and_conflicts_are_refused() {
    let identity = identity();
    // 整条重复：稳定去重，判定等于单条 Paused。
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[paused(), paused()]),
        Ok(RunnerSignalDirective::Hold)
    );
    // 重复出现在后续事实之后：仍然去重，判定取自最后一条 distinct 事实。
    let late_duplicate = [paused(), resumed("signal-2", 2, 4), paused()];
    assert_eq!(
        RunnerSignalGate::fold(&identity, &late_duplicate),
        Ok(RunnerSignalDirective::Resume {
            authority_epoch: 4,
            checkpoint_sequence: 5,
        })
    );
    // 同一 signal_id 不同内容：冲突，不折叠成任何方向。
    assert_eq!(
        RunnerSignalGate::fold(
            &identity,
            &[
                paused(),
                fact(AutomationSignalState::Paused, "signal-1", 2, 4, None)
            ]
        ),
        Err("runner_signal_conflict")
    );
    // 折叠是可重复的纯函数：同样输入得到逐字节相同的判定。
    assert_eq!(
        RunnerSignalGate::fold(&identity, &late_duplicate),
        RunnerSignalGate::fold(&identity, &late_duplicate)
    );
}

#[test]
fn checkpoint_binding_is_exact() {
    let identity = identity();
    let mut paused_drifted = paused();
    paused_drifted.checkpoint_sequence = 6;
    paused_drifted.recompute_digest();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[paused_drifted]),
        Err("runner_signal_checkpoint_unknown")
    );
    let mut resume_drifted = resumed("signal-2", 2, 4);
    resume_drifted.checkpoint_sequence = 6;
    resume_drifted.recompute_digest();
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[paused(), resume_drifted]),
        Err("runner_signal_checkpoint_unknown")
    );
}

#[test]
fn transitions_stay_within_the_pause_resume_consume_whitelist() {
    let identity = identity();
    // 第一条不能是 Resumed 或 Consumed。
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[resumed("signal-1", 1, 4)]),
        Err("runner_signal_transition_refused")
    );
    assert_eq!(
        RunnerSignalGate::fold(
            &identity,
            &[fact(
                AutomationSignalState::Consumed,
                "signal-1",
                1,
                4,
                Some(APPROVAL)
            )]
        ),
        Err("runner_signal_transition_refused")
    );
    // Paused → Consumed 不在白名单；相同状态的新事实也不允许。
    assert_eq!(
        RunnerSignalGate::fold(
            &identity,
            &[
                paused(),
                fact(
                    AutomationSignalState::Consumed,
                    "signal-2",
                    2,
                    4,
                    Some(APPROVAL)
                )
            ]
        ),
        Err("runner_signal_transition_refused")
    );
    assert_eq!(
        RunnerSignalGate::fold(
            &identity,
            &[
                paused(),
                fact(AutomationSignalState::Paused, "signal-2", 2, 4, None)
            ]
        ),
        Err("runner_signal_transition_refused")
    );
}

#[test]
fn authority_epoch_never_goes_backwards() {
    // runner 相信 active epoch 是 5，但历史里的 pause 仍是 epoch 4：拒绝，而不是照旧暂停。
    let stricter =
        RunnerSignalIdentity::new("execution-1", "owner-1", DEFINITION, ACTION, PATH, 5, 5);
    assert_eq!(
        RunnerSignalGate::fold(&stricter, &[paused()]),
        Err("runner_signal_authority_epoch_stale")
    );
}

#[test]
fn invalid_facts_are_refused_before_any_directive() {
    let identity = identity();
    // 字段被改但摘要未重算：单条事实不自洽，直接拒绝。
    let mut invalid = paused();
    invalid.signal_sequence = 0;
    assert_eq!(
        RunnerSignalGate::fold(&identity, &[invalid]),
        Err("runner_signal_fact_invalid")
    );
}

/// 源码守卫：模块保持纯边界（无 harness/模型/工具/EventStore 调用），导出清单与拒绝码保持绑定。
#[test]
fn aut18_runner_gate_stays_a_pure_source_boundary() {
    let gate = include_str!("../src/signal.rs");
    let lib = include_str!("../src/lib.rs");
    for marker in [
        "pub struct RunnerSignalGate",
        "pub struct RunnerSignalIdentity",
        "pub enum RunnerSignalDirective",
        "pub fn fold(",
        "model_calls_permitted",
        "is_closed",
        "automation_signal_transition_allowed",
        "runner_signal_identity_invalid",
        "runner_signal_execution_drift",
        "runner_signal_owner_drift",
        "runner_signal_definition_drift",
        "runner_signal_action_drift",
        "runner_signal_path_scope_drift",
        "runner_signal_fact_invalid",
        "runner_signal_conflict",
        "runner_signal_sequence_invalid",
        "runner_signal_transition_refused",
        "runner_signal_authority_epoch_stale",
        "runner_signal_authority_epoch_ahead",
        "runner_signal_checkpoint_unknown",
    ] {
        assert!(
            gate.contains(marker) || lib.contains(marker),
            "AUT-18 runner marker missing: {marker}"
        );
    }
    assert!(
        lib.contains(
            "pub use signal::{RunnerSignalDirective, RunnerSignalGate, RunnerSignalIdentity};"
        ),
        "runner signal exports must stay wired to lib.rs"
    );
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "EventStore::append",
    ] {
        assert!(
            !gate.contains(forbidden),
            "AUT-18 runner gate executes effects: {forbidden}"
        );
    }
}
