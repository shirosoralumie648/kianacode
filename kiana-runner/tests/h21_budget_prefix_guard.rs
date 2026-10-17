#[test]
fn h21_runner_budget_and_prefix_inputs_are_observable_and_not_split() {
    let harness = include_str!("../src/harness.rs");
    let budget = include_str!("../src/budget.rs");
    let compact = include_str!("../src/compact.rs");
    let step_context = include_str!("../src/step_context.rs");
    let domain = include_str!("../../kiana-domain/src/request_budget.rs");
    for marker in [
        "WireBudget",
        "output_reserved_tokens",
        "tool_schema_bytes",
        "prompt_sources",
        "route_digest",
        "StablePrefix",
        "ConservativeUtf8",
        "wire_budget_exceeded",
        "budget_coverage",
        "StablePrefixReuse",
        "usage_delta",
        "ExactTokenizer",
    ] {
        assert!(
            harness.contains(marker)
                || budget.contains(marker)
                || compact.contains(marker)
                || step_context.contains(marker)
                || domain.contains(marker),
            "H21 budget/prefix marker missing: {marker}"
        );
    }
    assert!(compact.contains("system"));
    assert!(harness.contains("PromptBundle::decode"));
    assert!(domain.contains("dynamic_suffix_digest"));

    // 输出预留与输出 cap 必须来自同一份有效 `TokenBudget`，而不是两个各算各的固定常量；
    // runner 侧同时提供可注入精确 tokenizer 口径的编译入口，缺省仍是显式保守回退。
    assert!(step_context.contains("prepared.budget.reserved_output"));
    assert!(step_context.contains("prepared.budget.limit"));
    assert!(step_context.contains("pub fn compile_wire_budget_with_accounting"));
    assert!(step_context.contains("CONSERVATIVE_WIRE_ACCOUNTING.clone()"));

    // 缓存复用决策只有一条共享实现：harness 必须调用 `step_context` 的决策函数，并在
    // 发送前用同一份冻结上下文复核绑定（先拒绝，再发送）。
    assert!(step_context.contains("pub fn decide_stable_prefix_reuse"));
    assert!(harness.contains("decide_stable_prefix_reuse"));
    assert!(harness.contains("compile_wire_budget("));
    assert!(harness.contains("recheck_step_context_bindings("));
}

/// 接线守卫：把卡片命名的先拒绝/成功行为夹具绑定到 runner 行为夹具文件、baseline 与
/// H21 focused workflow，避免夹具被删/改名或漏出 CI；它只做源码字符串核对，不运行夹具、
/// 不构造 DaemonHost，也不读取远程回执。
#[test]
fn h21_behaviour_fixtures_are_wired_into_the_diagnostic_lane() {
    let behavior = include_str!("h21_request_budget_behavior.rs");
    let workflow = include_str!("../../.github/workflows/h21-request-budget-diagnostic.yml");
    let baseline = include_str!("../../docs/roadmap/h21-request-budget-baseline.md");

    let deny = [
        "h21_huge_schema_or_system_prompt_exhausts_request_budget",
        "h21_cache_hit_cannot_bypass_data_revocation",
    ];
    let success = "h21_context_fits_actual_wire_budget_and_preserves_stable_prefix";
    for fixture in deny.iter().copied().chain([success]) {
        assert!(
            behavior.contains(fixture),
            "H21 behavior fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "H21 baseline fixture missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "H21 workflow fixture missing: {fixture}"
        );
    }

    // 先拒绝夹具必须排在成功夹具之前（卡片顺序：先证明拒绝路径）。
    let success_at = behavior
        .find(success)
        .expect("H21 success fixture is present");
    for fixture in deny {
        assert!(
            behavior.find(fixture).expect("H21 deny fixture is present") < success_at,
            "H21 deny fixture must precede the success fixture: {fixture}"
        );
    }

    // focused workflow 必须同时复核：域合同、runner 源码守卫与行为夹具，并按名
    // `--exact` 重跑三夹具、为三个 target 提供 `--no-run` 编译步。
    for step in [
        "cargo fmt -p kiana-domain -p kiana-runner --check",
        "cargo test -p kiana-domain --test h21_request_budget",
        "cargo test -p kiana-runner --test h21_budget_prefix_guard",
        "cargo test -p kiana-runner --test h21_request_budget_behavior",
    ] {
        assert!(
            workflow.contains(step),
            "H21 focused workflow step missing: {step}"
        );
    }
    assert!(workflow.contains("--exact"));
    assert!(workflow.contains("--no-run"));
    assert!(workflow.contains("--test-threads=1"));

    // 行为夹具必须是可运行的验收测试：不得被 ignore，且必须真正驱动 harness 与 fake
    // provider 的挂起/恢复数据流、专用记账口径与稳定前缀决策。
    assert!(!behavior.contains("#[ignore"));
    assert!(behavior.contains("prepared_requests"));
    assert!(behavior.contains("sent_requests"));
    assert!(behavior.contains("RunnerCommand::CapabilityResult"));
    assert!(behavior.contains("decide_stable_prefix_reuse"));
    assert!(behavior.contains("REQUEST_BUDGET_COVERAGE_ITEMS"));
    assert!(behavior.contains("compile_wire_budget_with_accounting"));
}
