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
        ],
        "fail-closed backend",
    );
    let container = include_str!("../../kiana-daemon/src/container_environment.rs");
    require(
        container,
        &[
            "probe.enforced",
            "dimension_supported",
            "container_required_dimension_unavailable",
            "backend_version",
            "container_backend_version_invalid",
            "required_dimensions",
            "container_environment_identity_mismatch",
        ],
        "container required-dimension enforcement",
    );
    // 【这条断言原来挂在 sandbox 组上，但发报的不是 sandbox】
    // fail-closed 的 `behavior_verified:false` 由
    // kiana-daemon/src/execution_control.rs 发出；harness_sandbox.rs
    // 连 `behavior_verified` 都不含。挪到发它的文件上，而不是删掉它。
    assert!(
        control.contains("\"behavior_verified\":false"),
        "fail-closed behavior_verified report missing from execution_control"
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

/// The card names two deny fixtures; the source guard above only proves shape. Bind the card
/// names to the behavioural target, the bwrap deny unit fixtures and the focused workflow that
/// runs them, so a rename or a dropped lane cannot pass silently.
#[test]
fn cap07_behavioural_fixtures_are_wired() {
    let fixture = include_str!("../../kiana-daemon/tests/cap07_environment_backend.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let workflow = include_str!("../../.github/workflows/cap07-environment-backend.yml");

    for name in [
        "missing_or_wrong_backend_never_falls_back_to_host",
        "backend_cannot_report_unenforced_required_dimension_as_supported",
        "environment_plan_is_deterministic_for_same_scope",
        "two_environments_on_same_host_keep_independent_identity_and_private_tmp",
        "prepared_identity_from_another_environment_is_denied_before_container_effect",
        "plan_rejects_backend_that_reports_itself_disabled",
        "plan_rejects_runsc_when_gvisor_dimension_is_not_enforced",
        "probe_reports_missing_runtime_binary_with_stable_unavailable_code",
    ] {
        assert!(fixture.contains(name), "CAP-07 fixture missing: {name}");
    }
    for name in [
        "find_bwrap_denies_missing_explicit_binary_with_stable_code",
        "find_bwrap_denies_non_executable_explicit_binary_with_stable_code",
        "find_bwrap_denies_non_executable_candidate_on_malicious_path",
        "find_bwrap_denies_missing_binary_on_path_with_stable_code",
        "bwrap_plan_denies_non_executable_explicit_binary_before_planning",
    ] {
        assert!(
            sandbox.contains(name),
            "CAP-07 sandbox deny fixture missing: {name}"
        );
    }
    for marker in [
        "--test cap07_environment_backend",
        "--lib harness_sandbox",
        "cap07_environment_backend_guard",
        "bubblewrap",
    ] {
        assert!(
            workflow.contains(marker),
            "CAP-07 workflow missing: {marker}"
        );
    }
    for forbidden in [
        "fallback_to_host",
        "unsandboxed_fallback",
        "execute_without_bwrap",
    ] {
        assert!(
            !fixture.contains(forbidden),
            "CAP-07 fixture fallback marker present: {forbidden}"
        );
    }
}
