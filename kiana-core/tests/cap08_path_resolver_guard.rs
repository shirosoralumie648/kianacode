//! CAP-08 source guard for shared path resolution and file identity preconditions.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-08 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn shared_path_resolver_contract_and_consumers_are_identity_bound() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let memory = include_str!("../../kiana-daemon/src/harness_memory.rs");
    let scope = include_str!("../../kiana-domain/src/execution_scope.rs");

    require(
        ports,
        &[
            "PathOperation",
            "PathIdentity",
            "PathResolution",
            "PathResolverPort",
            "path_resolver_unsupported",
            "path_resolver_revalidate_unsupported",
            "parent_handle_bound",
            "target_digest",
        ],
        "PathResolver contract",
    );
    require(
        patch,
        &[
            "openat(",
            "renameat(",
            "reject_symlink_components",
            "reject_hardlink",
            "root_identity",
            "descriptor-relative",
            "apply_patch_path_symlink",
            "apply_patch_path_hardlink",
            "workspace_journal_invalid_identity",
        ],
        "patch resolver consumer",
    );
    require(
        workspace,
        &[
            "FileVersion",
            "symlink_metadata",
            "file_identity_changed_or_hardlink",
            "openat(",
            "path_allow",
            "retained",
        ],
        "execution workspace consumer",
    );
    require(
        sandbox,
        &[
            "canonicalize_dir",
            "path_allow",
            "sandbox_external_symlink_requires_staging",
            "sandbox_read_deny_symlink_requires_staging",
            "private_component",
        ],
        "sandbox path consumer",
    );
    require(
        memory,
        &["symlink_metadata", "memory_path_hardlink", "canonicalize"],
        "memory path consumer",
    );
    require(
        scope,
        &["environment_id", "scope_digest", "validate"],
        "scope fence",
    );
}

#[test]
fn path_swap_and_escape_matrix_fails_closed_before_effect() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let workspace = include_str!("../../kiana-daemon/src/execution_workspace.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let guard = include_str!("sc13_path_toctou_guard.rs");
    let cap07 = include_str!("cap07_environment_backend_guard.rs");

    require(
        patch,
        &[
            "path_outside_project",
            "apply_patch_path_symlink",
            "apply_patch_path_hardlink",
            "reject_symlink_components",
            "renameat(",
            "openat(",
        ],
        "TOCTOU rejection",
    );
    require(
        workspace,
        &[
            "directory_identity_changed",
            "file_identity_changed_or_hardlink",
            "symlink",
        ],
        "workspace identity fence",
    );
    require(
        sandbox,
        &[
            "sandbox_external_symlink_requires_staging",
            "sandbox_path_depth_limit",
            "sandbox_scope_entry_limit",
        ],
        "sandbox escape fence",
    );
    require(
        guard,
        &["reject_symlink_components", "reject_hardlink", "openat("],
        "SC-13 regression",
    );
    require(
        cap07,
        &[
            "missing_or_wrong_backend_never_falls_back_to_host",
            "sandbox_unsupported",
        ],
        "CAP-07 regression",
    );
    for source in [patch, workspace, sandbox] {
        for forbidden in [
            "normalize_then_open_without_identity",
            "follow_untrusted_symlink",
            "host_path_fallback",
            "ignore_hardlink",
        ] {
            assert!(
                !source.contains(forbidden),
                "CAP-08 bypass marker present: {forbidden}"
            );
        }
    }
}
