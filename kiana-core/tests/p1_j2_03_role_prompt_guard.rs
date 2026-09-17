#[test]
fn role_prompt_reaches_the_provider_system_message() {
    let lifecycle = include_str!("../src/lifecycle.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let provider = include_str!("../../kiana-daemon/src/model_client.rs");
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    for marker in [
        "PromptBundle::for_role",
        "prompt_bundle.encode()",
        "bundle.system_prompt()",
        "context.system_prompt",
        "let system = Some(json!(context.system_prompt))",
        "PRODUCT_SYSTEM_PROMPT",
    ] {
        assert!(
            lifecycle.contains(marker)
                || runner.contains(marker)
                || provider.contains(marker)
                || prompts.contains(marker),
            "role prompt route marker missing: {marker}"
        );
    }
    let safety = prompts.find("PRODUCT_SYSTEM_PROMPT").unwrap();
    let role = prompts.find("name: \"role\"").unwrap();
    assert!(
        safety < role,
        "product safety section must precede role section"
    );
    assert!(provider.contains("context.budget().validate()"));
}

#[test]
fn role_prompt_cannot_replace_product_safety_prefix() {
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let provider = include_str!("../../kiana-daemon/src/model_client.rs");
    assert!(prompts.contains("authority: PromptAuthority::Product"));
    assert!(prompts.contains("name: \"product_safety\""));
    assert!(prompts.contains("name: \"role\""));
    assert!(provider.contains("subject to product policy and assigned role"));
    assert!(!provider.contains("role_prompt_as_authority"));
}
