#[test]
fn context_sections_stay_typed_and_provenance_bound() {
    let prompts = include_str!("../../kiana-domain/src/prompts.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let daemon = include_str!("../../kiana-daemon/src/harness_skills.rs");
    for marker in [
        "pub struct PromptSection",
        "pub fn render_prompt",
        "pub fn provenance",
        "PromptAuthority::Product",
        "PromptAuthority::Context",
        "PromptBundle::for_role",
        "prompt_sources",
    ] {
        assert!(
            prompts.contains(marker) || lifecycle.contains(marker) || daemon.contains(marker),
            "ContextPlan section marker missing: {marker}"
        );
    }
    assert!(prompts.contains("prompt_hash(&self.text)"));
    assert!(prompts.contains("sort_by(|a, b|"));
}
