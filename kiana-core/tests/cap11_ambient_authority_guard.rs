//! CAP-11 source guard for ambient authority removal and default-deny execution setup.

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
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
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
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
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
