//! H14 Core 审批恢复路径的静态源码守卫。
//!
//! 本文件读取 Core、Daemon、Runner 与 Domain 源文件并检查关键拒绝、去重、重新授权和原
//! Invocation 连续性标记；`include_str!` 不会执行审批 Store、恢复锁、Runner 或 Broker。
//! 同目录的远端 H14 focused workflow 才运行对应 target；此守卫本身不能证明运行时副作用
//! 次数、持久化崩溃恢复或 provider effect exactly-once。
/// 检查审批过期/绑定变化等拒绝信号存在，且重新 prepare 位于 dispatch 之前。
#[test]
fn expired_or_changed_approval_never_dispatches() {
    let approvals = include_str!("../src/approvals.rs");
    let recovery = include_str!("../src/recovery.rs");
    let daemon = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let domain = include_str!("../../kiana-domain/src/invocation_resume.rs");
    for marker in [
        "pending_with_proof",
        "approval_expired",
        "approval_request_hash_mismatch",
        "approval_nonce_mismatch",
        "approval_authority_changed",
        "approval_action_changed",
        "invocation_resume_binding_changed",
        "close_live_pending_approval",
        "invalidate",
        "cancel_pending_tools",
        "prepare_capability_action",
        "authorize_capability_action",
        "DispatchPermit",
        "events: vec![dispatching, executing]",
        "dispatch_capability_action",
    ] {
        assert!(
            approvals.contains(marker)
                || recovery.contains(marker)
                || daemon.contains(marker)
                || dispatch.contains(marker)
                || domain.contains(marker),
            "H14 expiry/change marker missing: {marker}"
        );
    }
    let revalidate = approvals
        .find("prepare_capability_action")
        .expect("resume re-preparation");
    let execute = approvals
        .find("dispatch_capability_action")
        .expect("resume dispatch");
    assert!(revalidate < execute);
    assert!(approvals.contains("finish_rejected_approval"));
}

/// 检查决策 command replay 与 single-use consumption 先于恢复，避免重复审批回复重复派发。
#[test]
fn duplicate_approval_reply_executes_at_most_once() {
    let approvals = include_str!("../src/approvals.rs");
    let journal = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    for marker in [
        "decision_command_id",
        "replay_approval_decision",
        "approval_already_consumed",
        "prepare_consumption",
        "pending_invocations",
        "dispatch_command_id",
    ] {
        assert!(
            approvals.contains(marker) || journal.contains(marker),
            "H14 idempotency marker missing: {marker}"
        );
    }
    assert!(
        approvals.find("replay_approval_decision").unwrap()
            < approvals.find("resume_approved_invocation").unwrap()
    );
    assert!(journal.contains("ApprovalState::Consumed"));
}

/// 检查恢复材料保留 Runner 的 pending batch 和原 Turn/Step/Invocation 身份。
#[test]
fn approval_resume_keeps_turn_step_and_invocation_ids() {
    let domain = include_str!("../../kiana-domain/src/invocation_resume.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let recovery = include_str!("../src/recovery.rs");
    for marker in [
        "InvocationResumeBinding",
        "pending_batch_digest",
        "parameter_digest",
        "catalog_digest",
        "authority_epoch",
        "owner_id",
        "turn_id",
        "step_id",
        "invocation_id",
        "resume_binding",
        "validate_against",
        "runner_state",
        "pending_tools",
        "drive_run",
    ] {
        assert!(
            domain.contains(marker)
                || runner.contains(marker)
                || capabilities.contains(marker)
                || lifecycle.contains(marker)
                || recovery.contains(marker),
            "H14 identity marker missing: {marker}"
        );
    }
    assert!(runner.contains("request.arguments[\"step_id\"]"));
    assert!(runner.contains("request.arguments[\"pending_batch_digest\"]"));
    assert!(capabilities.contains("Some(&resume_binding)"));
    assert!(lifecycle.contains("\"resume_binding\": pending.resume_binding"));
}
