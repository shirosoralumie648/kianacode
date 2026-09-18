//! CAP-07 source guard for EnvironmentPort and verifiable backend selection.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-07 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn environment_port_has_probe_plan_prepare_execute_quiesce_dispose_phases() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");
    let capability = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let scope = include_str!("../../kiana-domain/src/execution_scope.rs");

    require(
        ports,
        &[
            "pub struct EnvironmentScope",
            "pub struct EnvironmentProbe",
            "pub struct EnvironmentPlan",
            "pub struct PreparedEnvironment",
            "pub struct EnvironmentEffectReceipt",
            "pub trait EnvironmentPort",
            "async fn probe",
            "async fn plan",
            "async fn prepare",
            "async fn execute",
            "async fn quiesce",
            "async fn dispose",
            "environment_probe_unsupported",
            "environment_plan_unsupported",
            "environment_prepare_unsupported",
            "environment_execute_unsupported",
            "environment_quiesce_unsupported",
            "environment_dispose_unsupported",
        ],
        "EnvironmentPort phases",
    );
    require(
        sandbox,
        &[
            "SANDBOX_BACKEND",
            "BwrapPlan",
            "bwrap_plan_scoped",
            "find_bwrap",
            "--unshare-all",
            "--die-with-parent",
            "--cap-drop",
            "--clearenv",
            "KIANA_SANDBOX_NETWORK_DISABLED",
            "sandbox_unsupported:",
            "sandbox_unavailable:bwrap",
            "sandbox_env",
        ],
        "bwrap backend probe/plan",
    );
    require(
        control,
        &[
            "environment.inspect",
            "backend",
            "behavior_verified",
            "isolated_staged",
        ],
        "environment inspection",
    );
    require(
        capability,
        &[
            "sandboxed_command_scoped",
            "effect_known",
            "stop_confirmed",
            "backend",
        ],
        "capability execution boundary",
    );
    require(
        scope,
        &["environment_id", "scope_digest", "validate"],
        "execution scope",
    );
}

#[test]
fn missing_or_unenforced_backend_never_falls_back_to_host() {
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");

    require(
        sandbox,
        &[
            "sandbox_unavailable:bwrap",
            "sandbox_backend_unsupported:bwrap",
            "sandbox_toolchain_root_too_broad",
            "harness_workdir_outside_project",
            "sandbox_external_symlink_requires_staging",
            "sandbox_read_deny_symlink_requires_staging",
            "\"behavior_verified\":false",
        ],
        "fail-closed backend",
    );
    require(
        ports,
        &[
            "environment_probe_unsupported",
            "environment_plan_unsupported",
        ],
        "unsupported environment",
    );
    require(
        control,
        &[
            "SANDBOX_BACKEND",
            "environment.inspect",
            "network",
            "behavior_verified",
        ],
        "daemon report",
    );
    require(
        baseline,
        &[
            "missing_or_wrong_backend_never_falls_back_to_host",
            "backend_cannot_report_unenforced_required_dimension_as_supported",
            "environment_plan_is_deterministic_for_same_scope",
        ],
        "CAP-07 card",
    );
    for forbidden in [
        "fallback_to_host",
        "unsandboxed_fallback",
        "host_environment_fallback",
        "execute_without_bwrap",
    ] {
        assert!(
            !sandbox.contains(forbidden),
            "CAP-07 fallback marker present: {forbidden}"
        );
    }
}
