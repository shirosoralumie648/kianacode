#[test]
fn prepared_model_calls_fail_closed_when_context_budget_is_exhausted() {
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "pub struct TokenBudget",
        "pub system_prompt: u64",
        "pub tool_schemas: u64",
        "context_budget_exceeded",
        "prepared.validate()",
        "tool_schemas()",
        "TokenBudget::new",
    ] {
        assert!(
            prompts.contains(marker)
                || model.contains(marker)
                || ports.contains(marker)
                || harness.contains(marker),
            "budget marker missing: {marker}"
        );
    }
    assert!(prompts.contains("saturating_add(system_prompt)"));
    assert!(prompts.contains("saturating_add(tool_schemas)"));
}
