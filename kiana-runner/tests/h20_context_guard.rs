#[test]
fn h20_runner_keeps_one_step_snapshot_for_prompt_route_and_wire_request() {
    let harness = include_str!("../src/harness.rs");
    let model = include_str!("../src/model.rs");
    let context = include_str!("../../kiana-domain/src/context_plan.rs");
    let step_context = include_str!("../src/step_context.rs");
    for marker in [
        "PromptBundle::decode",
        "step_identity",
        "ModelRequest",
        "prompt_sources",
        "route_digest",
        "model_profile",
        "ContextPlan",
        "ResolvedStepContext",
        "resolved_context_route_changed",
        "resolved_context_workspace_changed",
    ] {
        assert!(
            harness.contains(marker)
                || model.contains(marker)
                || context.contains(marker)
                || step_context.contains(marker),
            "H20 context marker missing: {marker}"
        );
    }
    // runner 侧单步编译与解释：编译、冻结复核（含 fence 前缀）和两个内容绑定必须是
    // 同一个模块的公开面；harness 必须真正调用编译入口，而不是各步各读环境。
    for marker in [
        "compile_step_context",
        "StepContextExplanation",
        "recheck_step_context_bindings",
        "step_context_fence",
        "derive_workspace_binding",
        "derive_data_binding",
    ] {
        assert!(
            harness.contains(marker) || step_context.contains(marker),
            "H20 step-context marker missing: {marker}"
        );
    }
    assert!(harness.contains("compile_step_context"));
    assert!(harness.contains("StepIdentity::new"));
    assert!(harness.contains("ModelRequest {"));
    assert!(!harness.contains("request_context()") || harness.contains("ModelRequest"));
}

/// 接线守卫：把卡片命名的先拒绝/成功行为夹具绑定到 runner 行为夹具文件、baseline 与
/// H20 focused workflow，避免夹具被删/改名或漏出 CI；它只做源码字符串核对，不运行夹具、
/// 不构造 DaemonHost，也不读取远程回执。
#[test]
fn h20_behaviour_fixtures_are_wired_into_the_diagnostic_lane() {
    let behavior = include_str!("h20_context_behavior.rs");
    let workflow = include_str!("../../.github/workflows/h20-context-plan-diagnostic.yml");
    let baseline = include_str!("../../docs/roadmap/h20-context-plan-baseline.md");

    // 先拒绝两条 + 再成功一条：每一条都必须同时被行为夹具、baseline 目录和 workflow 认领。
    for fixture in [
        "h20_untrusted_project_instruction_never_enters_privileged_section",
        "h20_route_change_between_estimate_and_send_is_fenced",
        "h20_same_step_snapshot_renders_same_request_and_provenance",
    ] {
        assert!(
            behavior.contains(fixture),
            "H20 behavior fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "H20 baseline missing: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "H20 workflow fixture missing: {fixture}"
        );
    }

    // focused workflow 必须同时复核：域合同、runner 源码守卫与行为夹具，并按名
    // `--exact` 重跑三夹具、为三个 target 提供 `--no-run` 编译步。
    for step in [
        "cargo fmt -p kiana-domain -p kiana-runner --check",
        "cargo test -p kiana-domain --test h20_context_plan",
        "cargo test -p kiana-runner --test h20_context_guard",
        "cargo test -p kiana-runner --test h20_context_behavior",
    ] {
        assert!(
            workflow.contains(step),
            "H20 focused workflow step missing: {step}"
        );
    }
    assert!(workflow.contains("--exact"));
    assert!(workflow.contains("--no-run"));
    assert!(workflow.contains("--test-threads=1"));

    // 行为夹具必须是可运行的验收测试：不得被 ignore，且必须真正驱动 harness 与 fake
    // provider 的「冻结请求 == 发送请求」路径及工具回合。
    assert!(!behavior.contains("#[ignore"));
    assert!(behavior.contains("prepared_requests"));
    assert!(behavior.contains("sent_requests"));
    assert!(behavior.contains("RunnerCommand::CapabilityResult"));
}
