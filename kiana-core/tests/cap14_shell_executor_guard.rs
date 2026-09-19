//! CAP-14 source guard for strict shell/argv preparation and the shared process executor.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-14 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn shell_and_long_running_process_use_the_same_typed_command_plan() {
    let plan = include_str!("../../kiana-daemon/src/shell_plan.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let execution = include_str!("../../kiana-daemon/src/execution_control.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        plan,
        &[
            "ShellCommandPlan",
            "OpaqueString",
            "ShellCommandKind::Argv",
            "/bin/sh",
            "-c",
            "MAX_COMMAND_BYTES",
            "MAX_ARG_COUNT",
            "MAX_ARG_BYTES",
            "contains('\\0')",
            "side_effect_analysis",
            "login",
        ],
        "typed command plan",
    );
    require(
        harness,
        &["ShellCommandPlan::from_value", "shell_plan", "command_argv"],
        "shell adapter",
    );
    require(
        execution,
        &[
            "ShellCommandPlan::from_value",
            "shell_plan",
            "ProcessSupervisor",
        ],
        "long-running adapter",
    );
    require(
        supervisor,
        &["prepare_command", "ProcessSupervisor"],
        "shared executor",
    );
    require(
        baseline,
        &[
            "compound_command_cannot_borrow_safe_prefix_approval",
            "argv_and_shell_string_have_distinct_execution_semantics",
            "shell_cannot_replace_approved_workdir_or_profile",
        ],
        "CAP-14 card",
    );
    for forbidden in [
        "/bin/bash",
        "sh -l",
        "shell_prefix_approval",
        "regex_side_effect",
    ] {
        assert!(
            !plan.contains(forbidden),
            "CAP-14 unsafe shell marker present: {forbidden}"
        );
    }
}
