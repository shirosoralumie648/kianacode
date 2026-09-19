#[test]
fn h20_runner_keeps_one_step_snapshot_for_prompt_route_and_wire_request() {
    let harness = include_str!("../src/harness.rs");
    let model = include_str!("../src/model.rs");
    let context = include_str!("../../kiana-domain/src/context_plan.rs");
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
            harness.contains(marker) || model.contains(marker) || context.contains(marker),
            "H20 context marker missing: {marker}"
        );
    }
    assert!(harness.contains("StepIdentity::new"));
    assert!(harness.contains("ModelRequest {"));
    assert!(!harness.contains("request_context()") || harness.contains("ModelRequest"));
}
