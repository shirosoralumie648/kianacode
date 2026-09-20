#[test]
fn progressive_disclosure_is_explicit_and_fail_closed() {
    let disclosure = include_str!("../../kiana-skills/src/disclosure.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_skills.rs");
    for marker in [
        "SkillCatalogResponse",
        "load_skill_body",
        "read_skill_resource",
        "DisclosureQuota",
        "OverBudget",
        "complete: true",
    ] {
        assert!(
            disclosure.contains(marker) || harness.contains(marker),
            "missing EXT-06 boundary marker: {marker}"
        );
    }
    assert!(harness.contains("Skill body omitted: over_budget"));
    assert!(!harness.contains("fn truncate"));
}
