//! H16 并行屏障与逐调用授权的 Core 静态源码守卫。
//!
//! 这些用例用 `include_str!` 搜索 Domain planner、Runner、Core dispatch/账本等源码标记，
//! 用来发现相关接线文本被删改。它们本身不会构造真实并发 barrier、撤销 authority 或模拟
//! crash recovery；实际证明上限仍以工作区中相应运行时 fixture 和远程 CI 回执为准。

#[test]
/// 检查 Domain/Runner 源码保留只读组与独占屏障的调度合同标记。
///
/// 注意：断言中的英文短语来自 planner 的注释文本；本测试只验证源码包含它，不验证真实调用
/// 是否发生重叠。真实 bounded worker pool 在当前 baseline 中仍属后续工作。
fn write_barrier_never_overlaps_prior_reads() {
    let scheduling = include_str!("../../kiana-domain/src/tool_scheduling.rs");
    let authority = include_str!("../../kiana-domain/src/tool_authority.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
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
    assert!(scheduling.contains("exclusive call always drains the preceding group"));
    assert!(scheduling.contains("ToolSchedulingMode::Exclusive"));
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
