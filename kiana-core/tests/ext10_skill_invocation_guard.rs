#[test]
fn skill_invocation_adapter_is_explicit_and_non_authorizing() {
    let skills = include_str!("../../kiana-commands/src/skills.rs");
    for marker in [
        "skills_catalog",
        "invoke_skill",
        "read_skill_resource_command",
        "SkillInvocationRequest",
        "does_not_grant_tools",
        "does_not_execute",
        "read_only_adapter_no_external_effect",
    ] {
        assert!(skills.contains(marker), "missing EXT-10 marker: {marker}");
    }
    assert!(!skills.contains("std::process::Command"));
    assert!(!skills.contains("Command::new("));
}
