#[test]
fn daemon_hooks_and_skills_keep_trust_snapshot_and_sandbox_boundaries() {
    let hooks = include_str!("../src/pre_tool_hooks.rs");
    let skills = include_str!("../src/harness_skills.rs");
    let lifecycle = include_str!("../../kiana-domain/src/hook_lifecycle.rs");
    for marker in [
        "snapshot",
        "project_trusted",
        "run_confined_cancellable",
        "hook_deadline_exceeded",
        "hook_update_input_unsupported",
        "hook.decision",
        "read-only",
    ] {
        assert!(
            hooks.contains(marker),
            "missing hook boundary marker: {marker}"
        );
    }
    for marker in [
        "load_all_skills_with_trust",
        "ProjectTrust::Trusted",
        "ProjectTrust::Untrusted",
        "PromptAuthority::Context",
        "allowed-tools metadata",
    ] {
        assert!(
            skills.contains(marker),
            "missing skill boundary marker: {marker}"
        );
    }
    for marker in [
        "HookLifecyclePoint",
        "InputAccepted",
        "BeforeModel",
        "BeforeTool",
        "AfterTool",
        "BeforeCompact",
        "BeforeStop",
        "HookExecutionRole",
        "HookFailurePolicy",
        "revalidate_on_change",
        "HookLifecyclePlan",
    ] {
        assert!(
            lifecycle.contains(marker),
            "missing H29 lifecycle marker: {marker}"
        );
    }
    for forbidden in [
        "CapabilityGrant",
        "CapabilityRequest",
        "RunnerCommand::Start",
    ] {
        assert!(
            !lifecycle.contains(forbidden),
            "hook contract gained authority: {forbidden}"
        );
    }
}

#[test]
fn hook_argument_changes_are_revalidation_bound() {
    let lifecycle = include_str!("../../kiana-domain/src/hook_lifecycle.rs");
    let hooks = include_str!("../src/pre_tool_hooks.rs");
    for marker in [
        "hook_tool_change_revalidation_required",
        "requires_revalidation",
        "hook_update_field_denied",
        "hook_configuration_changed",
    ] {
        assert!(lifecycle.contains(marker) || hooks.contains(marker));
    }
    assert!(hooks.contains("hook_snapshot"));
    assert!(hooks.contains("command_digest"));
}
