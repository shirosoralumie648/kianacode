//! H27 Domain 输出合同与 TurnOutcome 优先级的纯内存合同测试。
//!
//! 输入是手工构造的 JSON、随机 ID 和 flags，不启动 Harness/Core、不写 EventLog，也不证明生产入口
//! 为每类输出提供了真实 pending/approval/clarification 状态。具体的 lifecycle 接线由 Core 源码守卫
//! 检查文本标记，远程 proof level 仍以 H27 baseline/CURRENT_STATUS 记录为准。

use kiana_domain::*;
use serde_json::json;
use std::collections::BTreeMap;

/// 构造要求 summary 字符串与 verification 数组的测试合同。
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

/// 生成“无待处理工作、非回答、无取消/失败/未知效果”的 TurnOutcome 输入 fixture。
///
/// 调用者只替换输出 JSON；其他 flags 固定为 false/0，引用是测试标签，不对应真实 Artifact/Event。
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
/// 合法对象且无阻塞事项会生成 terminal Completed，并通过 outcome 自校验。
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
/// required 字段缺失或类型不符时返回 Failed，而不是 Completed。
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
/// 说明用户澄清、审批和未完成工具 invocation 均先于输出文本决定结果。
///
/// 此用例逐个设置 flag，不覆盖多个 flag 同时为 true 时完整 precedence，也不真的创建审批或工具调用。
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
/// 说明 effect_unknown 与取消都不能被看似成功的 JSON 输出覆盖。
///
/// `effect_unknown` 变成 ResultUnknown，测试不执行真实副作用，也不完成 reconciliation。
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
/// 检查序列化往返保持结果值，且合同摘要与结果自摘要可复算。
///
/// 该断言证明同一内存值的稳定性，不证明 digest 是签名或输出来源认证。
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
