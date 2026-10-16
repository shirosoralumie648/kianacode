//! H27 Core 源码标记守卫：检查 Domain 合同、Core 包装和 lifecycle/receipt 代码仍保留指定结构。
//!
//! `include_str!` 只把源码作为普通字符串搜索；这些断言不调用 `TurnOutcome::decide()`、不触发事件、
//! 不运行 lifecycle，也不证明源码标记所描述的状态在真实请求中到达。

#[test]
/// 确认 H27 相关源码中仍包含结构化输出、Core 标注和 terminal receipt 路径的约定标记。
fn outcome_contract_and_terminal_gate_are_core_owned() {
    let domain = include_str!("../../kiana-domain/src/turn_outcome.rs");
    let core = include_str!("../src/turn_outcome.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let receipts = include_str!("../src/receipts.rs");
    let events = include_str!("../../kiana-domain/src/event_contracts.rs");
    for marker in [
        "OutputContract",
        "validate_output",
        "TurnOutcomeKind",
        "AwaitingInput",
        "AwaitingApproval",
        "ResultUnknown",
        "pending_invocations",
        "pending_background_jobs",
        "pending_steering",
        "output_contract_invalid",
        "terminal",
        "outcome_digest",
    ] {
        assert!(
            domain.contains(marker),
            "missing H27 domain marker: {marker}"
        );
    }
    for marker in [
        "propose_turn_outcome",
        "annotate_output",
        "TurnOutcomeInput",
        "turn_output_outcome_already_present",
    ] {
        assert!(core.contains(marker), "missing H27 core marker: {marker}");
    }
    for marker in [
        "crate::propose_turn_outcome",
        "crate::annotate_output",
        "run.completed",
        "record_terminal_event",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing lifecycle outcome gate: {marker}"
        );
    }
    assert!(receipts.contains("typed_run_receipt"));
    assert!(receipts.contains("ExecutionStatus::Completed"));
    assert!(events.contains("turn_outcome"));
    assert!(events.contains("outcome_digest"));
    assert!(!domain.contains("model_says_done"));
}

#[test]
/// 确认 lifecycle 与 clarification 源码保留等待路径标记，避免只靠模型文本返回 Completed。
fn pending_question_or_approval_is_not_completed_by_text() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let clarification = include_str!("../src/clarification.rs");
    for marker in [
        "RunnerEvent::ClarificationRequested",
        "CLARIFICATION_WAITING_STATUS",
        "return Ok(CoreResponse",
        "ExecutionStatus::Accepted",
    ] {
        assert!(lifecycle.contains(marker), "missing waiting gate: {marker}");
    }
    assert!(clarification.contains("ClarificationResolution"));
    assert!(!clarification.contains("ExecutionStatus::Completed"));
}
