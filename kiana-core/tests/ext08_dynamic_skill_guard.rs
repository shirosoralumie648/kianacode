#[test]
fn dynamic_skills_use_scoped_state_and_structured_arguments() {
    let dynamic = include_str!("../../kiana-skills/src/dynamic.rs");
    for marker in [
        "DynamicSkillScope",
        "snapshot_generation",
        "PathGlobAst",
        "trigger_digest",
        "SkillInvocationRequest",
        "validate_skill_argv",
        "dynamic_skill_revoked",
    ] {
        assert!(dynamic.contains(marker), "missing EXT-08 marker: {marker}");
    }
    assert!(!dynamic.contains("static DYNAMIC_SKILLS"));
    assert!(!dynamic.contains("std::process::Command"));
}
