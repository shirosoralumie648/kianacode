//! CAP-11 source guard for ambient authority removal and default-deny execution setup.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-11 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn sandbox_uses_os_enforced_namespace_env_and_fd_fences() {
    let sandbox =
        &source_guard::production_source(include_str!("../../kiana-daemon/src/harness_sandbox.rs"));
    let mcp = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let capabilities = include_str!("../../kiana-daemon/src/harness_capabilities.rs");

    require(
        sandbox,
        &[
            "--unshare-all",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--tmpfs",
            "/run",
            "--tmpfs",
            "/sys",
            "--cap-drop",
            "ALL",
            "--clearenv",
            "AMBIENT_AUTHORITY_ENV_VARS",
            "mark_unlisted_fds_cloexec",
            "CLOSE_RANGE_CLOEXEC",
            "PR_SET_NO_NEW_PRIVS",
            "sandbox_fd_fence_unsupported",
            "sandbox_no_new_privs_unsupported",
        ],
        "sandbox boundary",
    );
    require(
        mcp,
        &[
            "retain_mount_fd",
            "--ro-bind-fd",
            "mcp_executable_seal_unavailable",
        ],
        "MCP mount boundary",
    );
    require(
        capabilities,
        &[
            "sandboxed_command_scoped",
            "command.env_clear",
            "KIANA_SANDBOX_NETWORK_DISABLED",
        ],
        "process adapter boundary",
    );
    for forbidden in [
        "--share-net",
        "--share-ipc",
        "--share-pid",
        "--share-uts",
        "--dev-bind",
        "execute_without_bwrap",
        "fallback_to_unsandboxed_host",
    ] {
        assert!(
            !sandbox.contains(forbidden),
            "CAP-11 ambient authority bypass marker present: {forbidden}"
        );
    }
}

#[test]
fn ambient_environment_names_are_explicitly_denied() {
    let sandbox =
        &source_guard::production_source(include_str!("../../kiana-daemon/src/harness_sandbox.rs"));
    for marker in [
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "BASH_ENV",
        "PYTHONPATH",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
    ] {
        assert!(
            sandbox.contains(marker),
            "CAP-11 ambient environment marker missing: {marker}"
        );
    }
}

/// The card's deny-first and success fixtures must be real behaviour fixtures (they spawn the
/// planned boundary), named in the module, wired into the CAP-11 workflow and recorded in the
/// baseline. Fixture bodies live in the test-only module, so this guard reads the raw source.
#[test]
fn cap11_behavioural_fixtures_are_wired_and_deny_first() {
    let fixtures = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let workflow = include_str!("../../.github/workflows/cap11-ambient-authority.yml");
    let baseline = include_str!("../../docs/roadmap/cap11-ambient-authority-baseline.md");

    for fixture in [
        "secret_env_and_inherited_fds_are_unavailable",
        "direct_ipv4_ipv6_dns_and_host_unix_sockets_are_denied",
        "sandbox_cannot_rejoin_host_namespace",
        "offline_local_shell_and_private_ipc_run_without_egress",
        "offline_stdio_mcp_boundary_keeps_pinned_mount_and_no_egress",
    ] {
        assert!(
            fixtures.contains(fixture),
            "CAP-11 behavioural fixture missing from the sandbox boundary: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "CAP-11 behavioural fixture is not wired into the workflow: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "CAP-11 baseline does not record the fixture: {fixture}"
        );
    }

    require(
        fixtures,
        &[
            "CAP11_PROBE",
            "CAP11_ENV_SCRIPT",
            "CAP11_NAMESPACE_SCRIPT",
            "CAP11_LOCAL_SCRIPT",
            "env -i",
            "inherited-fd",
            "host-socket=absent",
            "ipv4=denied",
            "dns=denied",
            "unix=denied",
            "cap11-ipc",
            "interfaces=lo",
            "http://cap11-override.invalid",
            "retain_mount_fd",
            "bwrap_plan_scoped",
            "sandbox_env",
        ],
        "behavioural ambient-authority boundary",
    );
    for bypass in [
        "execute_without_bwrap",
        "fallback_to_unsandboxed_host",
        "--share-net",
    ] {
        assert!(
            !fixtures.contains(bypass),
            "CAP-11 behavioural fixtures introduced a bypass marker: {bypass}"
        );
    }
    assert!(
        workflow.contains("bwrap --version") && workflow.contains("python3 --version"),
        "the CAP-11 workflow must install and verify the default-deny runtime and probe toolchain"
    );
    assert!(
        workflow.contains("--exact"),
        "CAP-11 behavioural fixtures must run individually and exactly"
    );
}
