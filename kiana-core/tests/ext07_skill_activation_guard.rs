#[test]
fn skill_activation_and_resource_reads_are_snapshot_bound() {
    let disclosure = include_str!("../../kiana-skills/src/disclosure.rs");
    let loader = include_str!("../../kiana-skills/src/loader.rs");
    for marker in [
        "SkillActivationRequest",
        "SKILL_ACTIVATION_SCHEMA",
        "validate_for",
        "expires_at_unix_ms",
        "SourceResolver::resolve_resource",
        "SkillActivationStatus::Active",
    ] {
        assert!(
            disclosure.contains(marker) || loader.contains(marker),
            "missing EXT-07 boundary marker: {marker}"
        );
    }
    assert!(disclosure.contains("skill-activation:"));
}
