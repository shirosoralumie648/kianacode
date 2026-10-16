//! H17 的 Daemon 源码合同守卫。
//!
//! 这些测试通过 `include_str!` 搜索真实适配器/Core 源码中的拒绝和调用边界标记；它们不
//! 构造 DaemonHost、不 spawn 进程，也不执行重启恢复。运行时行为证据以基线所列的远端
//! focused workflow 为准，不能把字符串存在等同于实际授权流程已被此测试执行。
/// 检查 owner/scope 校验和危险操作分支都留在进程控制链路中，避免外部 owner 写入或取消。
#[test]
fn foreign_job_handle_cannot_receive_stdin_or_cancel() {
    let source = include_str!("../src/execution_control.rs");
    let core = include_str!("../../kiana-core/src/capabilities.rs");
    for marker in [
        "JobHandle",
        "validate_job_handle",
        "job_handle_owner_mismatch",
        "job_handle_scope_mismatch",
        "process.stdin",
        "process.stop",
        "process_authority_revoked",
    ] {
        assert!(
            source.contains(marker) || core.contains(marker),
            "H17 owner marker missing: {marker}"
        );
    }
}

/// 检查无 live entry 时存在 PID 重用防护标记，不能尝试根据持久化 PID 重新 attach。
#[test]
fn pid_reuse_cannot_attach_to_other_process() {
    let source = include_str!("../src/execution_control.rs");
    assert!(source.contains("process_handle_unavailable_after_restart"));
    assert!(source.contains("process_group_id"));
    assert!(source.contains("job_handle_process_identity_missing"));
    assert!(source.contains("process.started"));
    assert!(source.contains("process.prepared"));
}

/// 检查重启后的 poll 只读取已有终态/Unknown 及 cursor，不会重新 spawn 来伪装成轮询。
#[test]
fn poll_does_not_restart_job() {
    let source = include_str!("../src/execution_control.rs");
    assert!(source.contains("request.request.operation != \"process.poll\""));
    assert!(source.contains("process_handle_unavailable_after_restart"));
    assert!(source.contains("process.finished"));
    assert!(source.contains("cursor"));
}

/// 检查长进程只有一个稳定 JobHandle，而每个 poll/stdin/resize/stop 操作仍有独立 Invocation。
#[test]
fn long_command_has_one_job_and_distinct_operation_invocations() {
    let source = include_str!("../src/execution_control.rs");
    let dispatch = include_str!("../../kiana-core/src/dispatch.rs");
    for marker in [
        "job_handle",
        "start_request_id",
        "process_id",
        "process.poll",
        "process.stdin",
        "process.resize",
        "process.stop",
        "events: vec![dispatching, executing]",
        "invocation.executing",
        "InvocationId::from_uuid",
    ] {
        assert!(
            source.contains(marker) || dispatch.contains(marker),
            "H17 job marker missing: {marker}"
        );
    }
}
