//! H16 并行屏障与逐调用授权的 Core 静态源码守卫。
//!
//! 这些用例用 `include_str!` 搜索 Domain planner、Runner、Core dispatch/账本等源码标记，
//! 用来发现相关接线文本被删改。它们本身不会构造真实并发 barrier、撤销 authority 或模拟
//! crash recovery；运行期行为由 `kiana-runner/tests/h16_bounded_parallel.rs` 与
//! `kiana-domain/tests/h16_tool_scheduling.rs` 驱动，实际证明上限仍以远程 CI 回执为准。

#[test]
/// 检查 Domain/Runner 源码保留只读组与独占屏障的调度合同标记。
///
/// 注意：断言中的英文短语来自 planner 的注释文本；本测试只验证源码包含它，不验证真实调用
/// 是否发生重叠。执行器自身的会合点/计数器证据在 runner 的 H16 行为夹具中。
fn write_barrier_never_overlaps_prior_reads() {
    let scheduling = include_str!("../../kiana-domain/src/tool_scheduling.rs");
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let scheduler = include_str!("../../kiana-runner/src/tool_batch_scheduler.rs");
    for marker in [
        "ParallelRead",
        "Exclusive",
        "max_parallelism",
        "resources",
        "exclusive",
        "parallel_read",
        "plan_tool_batch",
        "call_ids",
        "pending_tools",
    ] {
        assert!(
            scheduling.contains(marker) || authority.contains(marker) || runner.contains(marker),
            "H16 barrier marker missing: {marker}"
        );
    }
    for marker in ["dispatch_width", "join_oldest", "ToolBatchAdmission::NotExecuted"] {
        assert!(scheduler.contains(marker), "H16 scheduler marker missing: {marker}");
    }
    assert!(scheduling.contains("exclusive call always drains the preceding group"));
    assert!(scheduling.contains("ToolSchedulingMode::Exclusive"));
    assert!(scheduler.contains("ToolSchedulingMode::Exclusive"));
    // 独占调用两侧都必须 drain：前一只读组结束前不得启动独占调用，独占调用结束前不得
    // 启动下一组；再加收尾的一次，正好三次 `while !in_flight.is_empty()`。
    let barrier_drains = scheduler.matches("while !in_flight.is_empty()").count();
    assert_eq!(barrier_drains, 3);
}

#[test]
/// 检查 Core 与 Runner 源码仍含“未开始调用在 authority 撤销后不能继续”的处理标记。
///
/// 这是静态 marker 检查，不构造 authority epoch 变化，不派发 capability，也不直接观察 not_executed 事件。
fn revocation_blocks_not_started_parallel_call() {
    let core = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "dispatch_authority_versions",
        "authorize_capability_action",
        "events: vec![dispatching, executing]",
        "invocation.executing",
        "prepare_capability_action",
        "cancel_pending_tools",
        "CapabilityResult::failure",
        "not_executed",
    ] {
        assert!(
            core.contains(marker) || capabilities.contains(marker) || runner.contains(marker),
            "H16 revocation marker missing: {marker}"
        );
    }
}

#[test]
/// 检查恢复代码仍保留已提交后续结果的引用路径，避免 crash 后重新执行已完成调用。
///
/// 守卫只读 H13 ledger、Core projection 和 dispatch 源码；它不模拟进程崩溃或读取持久化数据库。
fn crash_with_later_outcome_ready_does_not_rerun_it() {
    let ledger = include_str!("h13_invocation_ledger.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    assert!(ledger.contains("persisted_outcome_is_reused_without_reexecuting_tool"));
    assert!(projection.contains("execution.result_committed"));
    assert!(dispatch.contains("outcome_ready"));
    assert!(projection.contains("result_receipt"));
}

#[test]
/// 检查 H16 运行期夹具、domain 计划夹具与该 Step 的 focused workflow 仍然接线。
///
/// 这是接线守卫：它把“先拒绝/再成功”的夹具名绑定到文件与 workflow 目标上，避免夹具被删、
/// 被改名或漏出 CI。它不运行夹具，也不检查远程 CI 是否已经执行。
fn h16_behavioural_fixtures_are_wired_and_deny_first() {
    let scheduler = include_str!("../../kiana-runner/src/tool_batch_scheduler.rs");
    let fixtures = include_str!("../../kiana-runner/tests/h16_bounded_parallel.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/h16_tool_scheduling.rs");
    let workflow = include_str!("../../.github/workflows/h16-parallel-barriers-diagnostic.yml");

    // 卡片命名的四条夹具必须各自有运行期断言（不再只是 marker 文本）。
    for name in [
        "write_barrier_never_overlaps_prior_reads",
        "revocation_blocks_not_started_parallel_call",
        "crash_with_later_outcome_ready_does_not_rerun_it",
        "independent_reads_overlap_but_history_is_source_ordered",
        "concurrency_one_matches_the_serial_batch_contract",
    ] {
        assert!(fixtures.contains(name), "H16 runner fixture missing: {name}");
    }
    for name in [
        "parallelism_cap_is_the_intersection_of_run_deployment_and_role",
        "dispatch_width_intersects_group_and_environment_limits",
        "dispatch_units_preserve_source_order_across_barriers",
    ] {
        assert!(domain_fixtures.contains(name), "H16 domain fixture missing: {name}");
    }

    for target in ["h16_tool_scheduling", "h16_parallel_barriers", "h16_bounded_parallel"] {
        assert!(workflow.contains(target), "H16 workflow target missing: {target}");
    }
    assert!(workflow.contains("--test ${{ matrix.target }}"));

    // deny-first：执行器在派发前调用 admit，被拒时不得进入 worker；恢复时不得重跑已提交调用。
    assert!(scheduler.contains("match admit(ordinal, &unit.call_id)"));
    assert!(scheduler.contains("ToolBatchAdmission::NotExecuted(reason)"));
    assert!(scheduler.contains("ToolBatchAdmission::AlreadyCommitted"));
    assert!(scheduler.contains("let mut halted = false;"));
    assert!(!scheduler.contains("execute_without_admission"));
    assert!(fixtures.contains("authority_revoked"));
    assert!(fixtures.contains("reads_did_not_overlap"));
}
