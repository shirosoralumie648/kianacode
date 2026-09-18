//! CAP-09 source guard for the minimal Linux file view and real write-set limits.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-09 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn bwrap_view_is_explicit_and_workspace_write_is_staged() {
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");
    let capabilities = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let hcap_tests = include_str!("../../kiana-daemon/src/harness_capabilities.rs");

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
            "--ro-bind-fd",
            "--bind-fd",
            "pin_mount",
            "--cap-drop",
            "ALL",
            "--clearenv",
            "KIANA_SANDBOX_NETWORK_DISABLED",
            "No host-root bind",
            "private_component",
            "mask_private_paths",
        ],
        "minimal bwrap view",
    );
    require(
        workspace,
        &[
            "ExecutionWorkspace",
            "isolated_staged",
            "path_allow",
            "create_private_directory",
            "baseline",
            "publish_workspace_files",
            "retained",
        ],
        "staged workspace",
    );
    require(
        control,
        &[
            "read-only",
            "workspace-write",
            "ExecutionWorkspace::prepare",
            "scope[\"path_allow\"]",
            "private_component",
            "network\":\"deny_all\"",
            "file_identity\":\"descriptor_pinned\"",
        ],
        "execution control view",
    );
    require(
        capabilities,
        &[
            "sandboxed_command_scoped",
            "apply_patch_requires_workspace_write",
            "execution_workspace_prepare_join_failed",
            "result_unknown:workspace_publication_join_failed",
            "KIANA_SANDBOX_NETWORK_DISABLED",
        ],
        "capability view",
    );
    require(
        hcap_tests,
        &[
            "shell_exec_read_only_cannot_write_workspace",
            "shell_exec_workspace_write_can_write_workspace",
            "apply_patch_writes_in_workspace_write_sandbox",
        ],
        "view regression fixtures",
    );
}

#[test]
fn private_credentials_sockets_and_mount_swaps_fail_closed() {
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let control = include_str!("../../kiana-daemon/src/execution_control.rs");
    let cap08 = include_str!("cap08_path_resolver_guard.rs");

    require(
        sandbox,
        &[
            ".ssh",
            ".aws",
            ".mcp.json",
            ".env",
            ".git-credentials",
            "No host-root bind",
            "sandbox_external_symlink_requires_staging",
            "sandbox_read_deny_symlink_requires_staging",
            "sandbox_mount_identity_unavailable",
            "sandbox_mount_special_file_denied",
        ],
        "private/socket mount fence",
    );
    require(
        workspace,
        &[
            "file_identity_changed_or_hardlink",
            "directory_identity_changed",
            "symlink",
            "private_component",
            "openat(",
        ],
        "workspace mount swap fence",
    );
    require(
        control,
        &[
            "network\":\"deny_all\"",
            "path_allow",
            "private_component",
            "sandbox",
        ],
        "control mount policy",
    );
    require(
        cap08,
        &[
            "PathResolverPort",
            "reject_symlink_components",
            "reject_hardlink",
        ],
        "CAP-08 identity regression",
    );
    for source in [sandbox, workspace, control] {
        for forbidden in [
            "bind_host_root",
            "mount_entire_host",
            "ignore_private_component",
            "fallback_to_unsandboxed_host",
        ] {
            assert!(
                !source.contains(forbidden),
                "CAP-09 view bypass marker present: {forbidden}"
            );
        }
    }
}
