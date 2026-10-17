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

/// 四个命名验收必须仍落在 daemon lib 行为夹具里（防静默删除或改回纯文本守卫）。
#[test]
fn behavior_fixtures_cover_named_acceptance() {
    let source = include_str!("../src/execution_control.rs");
    for fixture in [
        "h17_behavior_foreign_job_handle_cannot_receive_stdin_or_cancel",
        "h17_behavior_pid_reuse_cannot_attach_to_other_process",
        "h17_behavior_poll_does_not_restart_job",
        "h17_behavior_long_command_has_one_job_and_distinct_operation_invocations",
    ] {
        assert!(source.contains(fixture), "H17 behavior fixture missing: {fixture}");
    }
    // 重启路径只解释已落盘事实的这一函数是行为夹具的唯一入口，不能被就地内联回去。
    assert!(source.contains("fn restart_continuation("));
    assert!(source.contains("restart_continuation(id, &request.request.operation"));
}

/// 接线守卫：把卡片命名的先拒绝/成功行为夹具绑定到 daemon lib 模块、baseline 与 H17
/// focused workflow，避免夹具被删/改名或漏出 CI；它只做源码字符串核对，不运行夹具、
/// 不构造 DaemonHost，也不读取远程回执。
#[test]
fn behavioural_fixtures_are_wired_into_the_h17_lane() {
    let source = include_str!("../src/execution_control.rs");
    let workflow = include_str!("../../.github/workflows/h17-job-handle-diagnostic.yml");
    let baseline = include_str!("../../docs/roadmap/harness-job-handle-baseline.md");

    // 先拒绝三条 + 再成功一条：每一条都必须同时被行为夹具、baseline 目录和 workflow 认领。
    for fixture in [
        "h17_behavior_foreign_job_handle_cannot_receive_stdin_or_cancel",
        "h17_behavior_pid_reuse_cannot_attach_to_other_process",
        "h17_behavior_poll_does_not_restart_job",
        "h17_behavior_long_command_has_one_job_and_distinct_operation_invocations",
    ] {
        assert!(source.contains(fixture), "H17 behavior fixture missing: {fixture}");
        assert!(baseline.contains(fixture), "H17 baseline missing: {fixture}");
        assert!(workflow.contains(fixture), "H17 workflow fixture missing: {fixture}");
    }

    // focused workflow 必须同时复核：domain 句柄边界、daemon 源码守卫、daemon lib 行为夹具，
    // 并在编译步覆盖对应 target（夹具名筛选只在同一 target 内生效）。
    for step in [
        "cargo fmt -p kiana-daemon -p kiana-domain --check",
        "cargo test -p kiana-domain --test h17_job_handle",
        "cargo test -p kiana-daemon --test h17_job_handle",
        "cargo test -p kiana-daemon --lib",
    ] {
        assert!(
            workflow.contains(step),
            "H17 focused workflow step missing: {step}"
        );
    }
    assert!(workflow.contains("-- --test-threads=1 h17"));
    assert!(workflow.contains("--exact"));
    assert!(workflow.contains("--no-run"));

    // deny-first：重启路径只解释已落盘事实，非 poll 操作一律落到 Unknown 而不会 attach。
    let restart_call =
        "return restart_continuation(id, &request.request.operation, &records, arguments);";
    assert!(source.contains(restart_call));
    assert!(source.contains("result_unknown:process_handle_unavailable_after_restart"));
}
