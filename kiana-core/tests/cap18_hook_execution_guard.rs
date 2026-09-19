//! CAP-18 source guard for trusted, scoped, cancellable hook execution.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-18 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn hooks_use_trust_snapshot_scope_and_the_shared_process_boundary() {
    let hooks = include_str!("../../kiana-daemon/src/pre_tool_hooks.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let core = include_str!("../src/capabilities.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        hooks,
        &[
            "project_trusted",
            "hook_snapshot",
            "hook_configuration_changed",
            "trusted_project",
            "sandbox",
            "path_allow",
            "run_confined_cancellable",
            "cancelled:hook_not_started",
            "cancelled:hook_stopped",
            "hook_update_input_unsupported",
            "AdapterResultKind::Hook",
            "\"raw_output_retained\":false",
        ],
        "hook adapter",
    );
    require(
        ports,
        &[
            "PreToolHookDecision",
            "decide_cancellable",
            "prepare_action",
            "hook_stopped",
        ],
        "hook port",
    );
    require(
        core,
        &[
            "prepare_action",
            "normalize_capability_action",
            "hook_approval_required",
            "hook_blocked",
        ],
        "control-plane reauthorization",
    );
    require(
        harness,
        &[
            "ProcessSupervisor::stop",
            "stop_report",
            "sandboxed_command_scoped",
        ],
        "shared execution",
    );
    require(
        supervisor,
        &[
            "ProcessSupervisor",
            "prepare_command",
            "pub(crate) async fn stop",
        ],
        "supervisor",
    );
    require(
        baseline,
        &[
            "untrusted_hook_never_spawns",
            "hook_cannot_write_outside_its_scope_or_grant_permissions",
            "hook_input_change_invalidates_prior_approval",
            "hook_cancellation_stops_its_descendants",
        ],
        "CAP-18 card",
    );
    for forbidden in ["Command::new", "std::process::Command", "broker.execute"] {
        assert!(
            !hooks.contains(forbidden),
            "CAP-18 hook bypass marker present: {forbidden}"
        );
    }
}
